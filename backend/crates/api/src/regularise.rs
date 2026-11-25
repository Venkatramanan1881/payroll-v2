//! Regularisation RG1–RG6, HR corrections C1–C2, bulk waivers W1 and outage fill O1
//! (API_CONTRACTS Package 2 §8, SM-AT-5, US-AT-32–35). Nothing here edits a punch or a day result: an approved
//! request writes manual punches and adjudications, a correction writes a day override, and the day is
//! recalculated from the event (BR-01).
// shortcut: limits (RG-03) count per calendar month, not per attendance period; switch to periods with S9.
// shortcut: a decision or correction on a date that has since locked is stored but not yet posted as an
// adjustment (§1.3 "Adjust"); that arrives with periods and lock (S9, on hold). Cancel refuses (RG6).

use crate::ctx::{if_match, stale, Ctx, Scope};
use crate::employees::{allowed, bad_param, em_settings, push_from, push_scope, query_pairs, require_edit, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, NaiveDate, NaiveTime};
use payroll_core::policy::{AtSettings, NoAction, OverLimit};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

const REQUEST: &str = "attendance.regularisation.request";
const APPROVE: &str = "attendance.regularisation.approve";
const CANCEL: &str = "attendance.regularisation.cancel";
const CORRECT: &str = "attendance.day.correct";
const WAIVE: &str = "attendance.waiver.manage";
const OUTAGE: &str = "attendance.outage.regularise";
/// Request kinds (the table's names) and their RG-02 setting names.
const KINDS: [(&str, &str); 5] = [("missed_punch", "missed_punch"), ("late", "late_arrival"), ("early", "early_check_out"), ("wrong_punch", "wrong_punch"), ("whole_day", "whole_day")];
const DAY_STATUSES: [&str; 8] = ["present", "half_day", "absent", "absent_pending", "weekly_off", "holiday", "holiday_unpaid", "long_leave"];

fn disabled(setting: &str, detail: &str) -> crate::error::ApiError {
    AppError::new(403, "feature_disabled", "Switched off", detail.to_string()).with_meta(json!({ "setting": setting })).into()
}

fn need_reason(r: Option<String>) -> ApiResult<String> {
    r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).ok_or_else(|| AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into())
}

/// An approver above the team level: an HR Manager or Org Admin (branch, department or organisation scope).
fn hr_level(s: &Scope) -> bool {
    s.org || !s.branches.is_empty() || !s.departments.is_empty()
}

/// The reporting manager's employee id on the request date, when the manager has an active user (else NULL).
const MANAGER: &str = "(SELECT mj.reporting_manager_id FROM app.employee_job_versions mj
      JOIN app.users mu ON mu.employee_id = mj.reporting_manager_id AND mu.state = 'active'
      WHERE mj.employee_id = r.employee_id AND mj.state = 'active' AND mj.valid @> r.work_date LIMIT 1)";

/// The request is waiting for the reporting manager (level 1 of SM-AT-5); otherwise HR decides (DEC-31).
fn manager_stage() -> String {
    format!("(r.state = 'pending' AND r.current_level = 1 AND NOT r.over_limit AND {MANAGER} IS NOT NULL)")
}

fn doc() -> String {
    format!(
        "jsonb_build_object('id', r.id, 'employee', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
        'work_date', r.work_date, 'kind', r.kind, 'requested', r.requested, 'reason', r.reason, 'attachment', r.attachment_ref, 'state', r.state,
        'level', r.current_level, 'levels', r.levels, 'over_limit', r.over_limit, 'on_behalf', r.on_behalf, 'raised_by', r.raised_by,
        'escalated_at', r.escalated_at, 'cancel_reason', r.cancel_reason, 'created_at', r.created_at, 'version', r.row_version,
        'current_approver', CASE WHEN r.state NOT IN ('pending', 'escalated') THEN NULL WHEN {} THEN 'manager' ELSE 'hr' END,
        'decisions', (SELECT coalesce(jsonb_agg(jsonb_build_object('level', d.level, 'action', d.action, 'actor', d.actor_id, 'reason', d.reason,
                      'at', d.decided_at) ORDER BY d.id), '[]') FROM app.regularisation_decisions d WHERE d.request_id = r.id))",
        manager_stage()
    )
}

async fn request_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    Ok(sqlx::query_scalar(&format!("SELECT {} FROM app.regularisation_requests r JOIN app.employees e ON e.id = r.employee_id WHERE r.id = $1", doc()))
        .bind(id)
        .fetch_one(&mut **tx)
        .await?)
}

/// Master branch and its time zone on the date.
async fn placed(tx: &mut Tx, employee: Uuid, date: NaiveDate) -> ApiResult<Option<(Uuid, String)>> {
    Ok(sqlx::query_as(
        "SELECT jv.master_branch_id, b.time_zone FROM app.employee_job_versions jv JOIN app.branches b ON b.id = jv.master_branch_id
         WHERE jv.employee_id = $1 AND jv.state = 'active' AND jv.valid @> $2::date",
    )
    .bind(employee)
    .bind(date)
    .fetch_optional(&mut **tx)
    .await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Requested {
    in_time: Option<NaiveTime>,
    out_time: Option<NaiveTime>,
    punch_id: Option<Uuid>,
    direction: Option<String>,
}

/// What each kind must carry (US-AT-32): times for missed, late, early and whole day; a punch for wrong punch.
fn check_requested(kind: &str, r: &Requested) -> Option<&'static str> {
    match kind {
        "missed_punch" if r.in_time.is_none() && r.out_time.is_none() => Some("Give the missing in_time or out_time"),
        "late" if r.in_time.is_none() => Some("Give the actual in_time"),
        "early" if r.out_time.is_none() => Some("Give the actual out_time"),
        "whole_day" if r.in_time.is_none() || r.out_time.is_none() => Some("Give in_time and out_time"),
        "wrong_punch" if r.punch_id.is_none() => Some("Give the punch_id to fix"),
        "wrong_punch" if r.direction.as_deref().is_some_and(|d| d != "in" && d != "out") => Some("direction is in or out (leave it out to exclude the punch)"),
        "wrong_punch" if r.in_time.is_some() || r.out_time.is_some() => Some("A wrong punch fix takes punch_id and direction only"),
        _ => None,
    }
}

/// Writes what an approved request asks for: manual punches for times, an adjudication for a punch fix.
async fn apply(ctx: &mut Ctx, id: Uuid) -> ApiResult<()> {
    let row = sqlx::query("SELECT employee_id, work_date, requested, reason FROM app.regularisation_requests WHERE id = $1").bind(id).fetch_one(&mut *ctx.tx).await?;
    let (employee, date, reason): (Uuid, NaiveDate, String) = (row.get("employee_id"), row.get("work_date"), row.get("reason"));
    let req: Requested = serde_json::from_value(row.get("requested")).map_err(|_| AppError::internal())?;
    let (master, zone) = placed(&mut ctx.tx, employee, date).await?.ok_or_else(|| AppError::unprocessable("outside_employment", "Not employed then", "There is no employment on that date"))?;
    let next_day = matches!((req.in_time, req.out_time), (Some(i), Some(o)) if o <= i);
    for (dir, t) in [("in", req.in_time), ("out", req.out_time)] {
        let Some(t) = t else { continue };
        let local = date.and_time(t) + Duration::days((dir == "out" && next_day) as i64);
        sqlx::query(
            "INSERT INTO app.manual_punches (org_id, employee_id, punched_at, direction, branch_id, origin, reason, regularisation_id)
             VALUES ($1, $2, $3 AT TIME ZONE $4, $5::app.punch_direction, $6, 'regularisation', $7, $8)",
        )
        .bind(ctx.org_id)
        .bind(employee)
        .bind(local)
        .bind(&zone)
        .bind(dir)
        .bind(master)
        .bind(&reason)
        .bind(id)
        .execute(&mut *ctx.tx)
        .await?;
    }
    if let Some(p) = req.punch_id {
        let kind = if req.direction.is_some() { "change_direction" } else { "exclude" };
        sqlx::query(
            "INSERT INTO app.punch_adjudications (org_id, punch_id, punched_at, employee_id, kind, new_direction, origin, regularisation_id, reason)
             SELECT $1, p.id, p.punched_at, p.employee_id, $3::app.adjudication_kind, $4::app.punch_direction, 'regularisation', $5, $6
             FROM app.raw_punches p WHERE p.id = $2",
        )
        .bind(ctx.org_id)
        .bind(p)
        .bind(kind)
        .bind(&req.direction)
        .bind(id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
    }
    ctx.event("PunchRecorded", "employee", employee, json!({ "employee_id": employee, "work_date": date, "regularisation_id": id })).await
}

async fn decision(ctx: &mut Ctx, id: Uuid, level: i16, action: &str, actor: Option<Uuid>, reason: Option<&str>) -> ApiResult<()> {
    sqlx::query("INSERT INTO app.regularisation_decisions (org_id, request_id, level, action, actor_id, reason) VALUES ($1, $2, $3, $4::app.decision_action, $5, $6)")
        .bind(ctx.org_id)
        .bind(id)
        .bind(level)
        .bind(action)
        .bind(actor)
        .bind(reason)
        .execute(&mut *ctx.tx)
        .await?;
    Ok(())
}

async fn set_state(ctx: &mut Ctx, id: Uuid, state: &str, level: i16) -> ApiResult<()> {
    sqlx::query(
        "UPDATE app.regularisation_requests SET state = $2::app.regularisation_state, current_level = $3, row_version = row_version + 1, updated_at = now(),
           escalated_at = CASE WHEN $2 = 'escalated' THEN now() ELSE escalated_at END WHERE id = $1",
    )
    .bind(id)
    .bind(state)
    .bind(level)
    .execute(&mut *ctx.tx)
    .await?;
    Ok(())
}

// ---------- RG1 POST /regularisations ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewRequest {
    employee_id: Option<Uuid>,
    work_date: NaiveDate,
    kind: String,
    requested: Value,
    reason: Option<String>,
    attachment: Option<String>,
}

// shortcut: `attachment` is a reference to a file stored elsewhere; an upload route for it comes with the app screens.
pub async fn raise(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(REQUEST)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/regularisations", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewRequest = parse(raw)?;
    let reason = need_reason(req.reason)?;
    let Some((_, setting_kind)) = KINDS.iter().find(|k| k.0 == req.kind) else {
        return Err(AppError::validation(vec![FieldError::new("kind", "invalid_value", "missed_punch, late, early, wrong_punch or whole_day")]).into());
    };
    let wanted: Requested = serde_json::from_value(req.requested.clone()).map_err(|e| AppError::validation(vec![FieldError::new("requested", "invalid_value", e.to_string())]))?;
    if let Some(why) = check_requested(&req.kind, &wanted) {
        return Err(AppError::validation(vec![FieldError::new("requested", "invalid_value", why)]).into());
    }
    if req.attachment.as_ref().is_some_and(|a| a.trim().is_empty() || a.len() > 500) {
        return Err(AppError::validation(vec![FieldError::new("attachment", "invalid_value", "A file reference up to 500 characters")]).into());
    }
    // Own request, or on behalf of someone in edit scope (REL-09).
    let employee = req.employee_id.or(ctx.employee_id).ok_or_else(|| AppError::validation(vec![FieldError::new("employee_id", "required", "Give the employee")]))?;
    let on_behalf = ctx.employee_id != Some(employee);
    if on_behalf {
        let em = em_settings(&mut ctx.tx, today()).await?;
        if !allowed(&mut ctx, employee, REQUEST, Access::Edit, &em).await? {
            return Err(AppError::not_found().into());
        }
    }
    let date = req.work_date;
    if date > today() {
        return Err(AppError::validation(vec![FieldError::new("work_date", "invalid_value", "A date that has passed or is today")]).into());
    }
    if placed(&mut ctx.tx, employee, date).await?.is_none() {
        return Err(AppError::unprocessable("outside_employment", "Not employed then", "There is no employment on that date").into());
    }
    let s = crate::policies::settings_for(&mut ctx.tx, employee, date).await?;
    if !s.regularisation {
        return Err(disabled("RG-01", "Regularisation is switched off"));
    }
    if !s.rg_types.iter().any(|t| t == setting_kind) || (req.kind == "early" && !s.early_regularisation) {
        return Err(disabled(if req.kind == "early" && !s.early_regularisation { "EC-16" } else { "RG-02" }, "This request type is switched off"));
    }
    if today() - date > Duration::days(s.rg_window_days as i64) {
        return Err(AppError::unprocessable("window_passed", "Too late to ask", format!("Requests are allowed within {} days of the date", s.rg_window_days)).into());
    }
    if let Some(l) = crate::periods::lock_on(&mut ctx.tx, employee, date).await? {
        return Err(crate::periods::date_locked(l, date));
    }
    if s.rg_attachment_required_types.iter().any(|t| t == setting_kind) && req.attachment.is_none() {
        return Err(AppError::unprocessable("attachment_required", "Attachment required", "This request type needs an attachment").into());
    }
    if let Some(p) = wanted.punch_id {
        let mine: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.raw_punches WHERE id = $1 AND employee_id = $2)").bind(p).bind(employee).fetch_one(&mut *ctx.tx).await?;
        if !mine {
            return Err(AppError::validation(vec![FieldError::new("requested.punch_id", "not_found", "No such punch for this employee")]).into());
        }
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM app.regularisation_requests WHERE employee_id = $1 AND work_date = $2 AND kind = $3::app.regularisation_kind AND state IN ('pending', 'escalated'))",
    )
    .bind(employee)
    .bind(date)
    .bind(&req.kind)
    .fetch_one(&mut *ctx.tx)
    .await?;
    if pending {
        return Err(AppError::conflict("request_already_pending", "Already asked", "A request of this type for this date is waiting for a decision").into());
    }
    let (overall, of_kind): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE kind = $3::app.regularisation_kind) FROM app.regularisation_requests
         WHERE employee_id = $1 AND date_trunc('month', work_date) = date_trunc('month', $2::date)
           AND state NOT IN ('withdrawn', 'rejected', 'auto_rejected', 'cancelled')",
    )
    .bind(employee)
    .bind(date)
    .bind(&req.kind)
    .fetch_one(&mut *ctx.tx)
    .await?;
    // A limit of 0 means no limit.
    let over = |n: i64, limit: u32| limit > 0 && n >= limit as i64;
    let over_limit = over(overall, s.rg_limit_overall) || s.rg_limit_per_type.get(*setting_kind).is_some_and(|l| over(of_kind, *l));
    if over_limit && s.rg_over_limit == OverLimit::Block {
        return Err(AppError::unprocessable("limit_reached", "Limit reached", "The request limit for this month is used up").into());
    }
    let auto = s.rg_auto_approve_types.iter().any(|t| t == setting_kind);
    let levels = s.rg_levels.clamp(1, 2) as i16;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.regularisation_requests (org_id, employee_id, work_date, kind, requested, reason, attachment_ref, raised_by, on_behalf, levels, over_limit, state)
         VALUES ($1, $2, $3, $4::app.regularisation_kind, $5, $6, $7, $8, $9, $10, $11, $12::app.regularisation_state) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(employee)
    .bind(date)
    .bind(&req.kind)
    .bind(&req.requested)
    .bind(&reason)
    .bind(&req.attachment)
    .bind(ctx.user_id)
    .bind(on_behalf)
    .bind(levels)
    .bind(over_limit)
    .bind(if auto { "approved" } else { "pending" })
    .fetch_one(&mut *ctx.tx)
    .await?;
    if auto {
        decision(&mut ctx, id, levels, "auto_approve", None, Some("RG-06")).await?;
        apply(&mut ctx, id).await?;
    }
    let body = request_doc(&mut ctx.tx, id).await?;
    ctx.audit(REQUEST, "regularisation_request", id, None, Some(body.clone()), Some(&reason)).await?;
    // NT-05: the reporting manager hears about a request waiting for them.
    if body["current_approver"] == "manager" {
        let mgr: Option<Uuid> = sqlx::query_scalar(&format!("SELECT u.id FROM app.regularisation_requests r JOIN app.users u ON u.employee_id = {MANAGER} AND u.state = 'active' WHERE r.id = $1 LIMIT 1"))
            .bind(id)
            .fetch_optional(&mut *ctx.tx)
            .await?;
        if let Some(u) = mgr {
            let key = json!({ "request_id": id });
            crate::notify::notify(&mut ctx.tx, ctx.org_id, u, "RegularisationSubmitted", "manager", &key, &json!({ "request_id": id, "work_date": date, "kind": req.kind })).await?;
        }
    }
    ctx.idempotency_finish(&headers, 201, &body).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

// ---------- RG2 / RG3 ----------

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let approver = ctx.scope(APPROVE).ok();
    let mut qb = QueryBuilder::new(format!("SELECT coalesce(jsonb_agg(doc ORDER BY at DESC), '[]') FROM (SELECT {} AS doc, r.created_at AS at", doc()));
    let as_of = today();
    push_from(&mut qb, as_of);
    qb.push(" JOIN app.regularisation_requests r ON r.employee_id = e.id WHERE (r.raised_by = ").push_bind(ctx.user_id);
    if let Some(me) = ctx.employee_id {
        qb.push(" OR r.employee_id = ").push_bind(me);
    }
    if let Some(s) = &approver {
        let em = em_settings(&mut ctx.tx, as_of).await?;
        qb.push(" OR (");
        push_scope(&mut qb, s, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
        qb.push(")");
    }
    qb.push(")");
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "state" => {
                qb.push(" AND r.state::text = ").push_bind(v);
            }
            "employee_id" => {
                qb.push(" AND r.employee_id = ").push_bind(v.parse::<Uuid>().map_err(|_| bad_param("employee_id", "Not an id"))?);
            }
            "from" => {
                qb.push(" AND r.work_date >= ").push_bind(v.parse::<NaiveDate>().map_err(|_| bad_param("from", "Use YYYY-MM-DD"))?);
            }
            "to" => {
                qb.push(" AND r.work_date <= ").push_bind(v.parse::<NaiveDate>().map_err(|_| bad_param("to", "Use YYYY-MM-DD"))?);
            }
            "awaiting_me" if v == "true" => {
                let hr = approver.as_ref().is_some_and(hr_level);
                qb.push(format!(" AND r.state IN ('pending', 'escalated') AND CASE WHEN {} THEN {MANAGER} IS NOT DISTINCT FROM ", manager_stage()))
                    .push_bind(ctx.employee_id)
                    .push(" ELSE ")
                    .push_bind(hr)
                    .push(" END AND r.raised_by <> ")
                    .push_bind(ctx.user_id);
                if let Some(me) = ctx.employee_id {
                    qb.push(" AND r.employee_id <> ").push_bind(me);
                }
            }
            "awaiting_me" => {}
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    // shortcut: the latest 200, no paging yet.
    qb.push(" ORDER BY r.created_at DESC LIMIT 200) x");
    let items: Value = qb.build_query_scalar().fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

/// Locks a request: (employee, raised_by, state, level, levels, over_limit, waiting for the manager, manager, date, version).
type ReqRow = (Uuid, Uuid, String, i16, i16, bool, bool, Option<Uuid>, NaiveDate, i32);

async fn lock_request(tx: &mut Tx, id: Uuid) -> ApiResult<ReqRow> {
    let row: Option<ReqRow> = sqlx::query_as(&format!(
        "SELECT r.employee_id, r.raised_by, r.state::text, r.current_level, r.levels, r.over_limit, {}, {MANAGER}, r.work_date, r.row_version
         FROM app.regularisation_requests r WHERE r.id = $1 FOR UPDATE",
        manager_stage()
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| AppError::not_found().into())
}

/// Seen by the employee, the raiser, and approvers whose scope holds the employee.
async fn visible(ctx: &mut Ctx, employee: Uuid, raised_by: Uuid) -> ApiResult<bool> {
    if ctx.employee_id == Some(employee) || ctx.user_id == raised_by {
        return Ok(true);
    }
    let em = em_settings(&mut ctx.tx, today()).await?;
    allowed(ctx, employee, APPROVE, Access::View, &em).await
}

pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (employee, raised_by, ..) = lock_request(&mut ctx.tx, id).await?;
    if !visible(&mut ctx, employee, raised_by).await? {
        return Err(AppError::not_found().into());
    }
    let doc = request_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc))
}

// ---------- RG4 approve / reject ----------

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Note {
    reason: Option<String>,
    note: Option<String>,
}

async fn decide(mut ctx: Ctx, id: Uuid, headers: HeaderMap, raw: Value, approve: bool) -> ApiResult<Response> {
    let scope = ctx.scope(APPROVE)?;
    let body: Note = if raw.is_null() { Note::default() } else { parse(raw)? };
    let (employee, raised_by, state, level, levels, over_limit, manager_stage, manager, _, version) = lock_request(&mut ctx.tx, id).await?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, employee, APPROVE, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    if state != "pending" && state != "escalated" {
        return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {state}")).into());
    }
    if if_match(&headers)? != version {
        return Err(stale(version));
    }
    if raised_by == ctx.user_id || ctx.employee_id == Some(employee) {
        return Err(AppError::new(403, "cannot_decide_own_request", "Not your own request", "Someone else must decide a request you raised or that is yours").into());
    }
    let mine = if manager_stage { ctx.employee_id.is_some() && ctx.employee_id == manager } else { hr_level(&scope) };
    if !mine {
        return Err(AppError::new(403, "wrong_approver", "Not your turn", if manager_stage { "The reporting manager decides this request first" } else { "HR decides this request" }).into());
    }
    let reason = body.reason.or(body.note).map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let old = request_doc(&mut ctx.tx, id).await?;
    let me = ctx.user_id;
    let mut reason = reason;
    if approve {
        decision(&mut ctx, id, level, "approve", Some(me), reason.as_deref()).await?;
        // Level 1 by the manager with two levels goes on to HR; any HR decision is the last one.
        if manager_stage && level < levels && !over_limit {
            set_state(&mut ctx, id, "pending", level + 1).await?;
        } else {
            set_state(&mut ctx, id, "approved", level).await?;
            apply(&mut ctx, id).await?;
        }
    } else {
        let r = need_reason(reason)?;
        decision(&mut ctx, id, level, "reject", Some(me), Some(&r)).await?;
        reason = Some(r);
        set_state(&mut ctx, id, "rejected", level).await?;
    }
    let doc = request_doc(&mut ctx.tx, id).await?;
    ctx.audit(APPROVE, "regularisation_request", id, Some(json!({ "state": old["state"], "level": old["level"] })), Some(json!({ "state": doc["state"], "level": doc["level"] })), reason.as_deref()).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

pub async fn approve(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, true).await
}
pub async fn reject(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, false).await
}

// ---------- RG5 withdraw, RG6 cancel ----------

pub async fn withdraw(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap) -> ApiResult<Response> {
    let (employee, raised_by, state, level, .., version) = lock_request(&mut ctx.tx, id).await?;
    if raised_by != ctx.user_id {
        return Err(if visible(&mut ctx, employee, raised_by).await? { AppError::forbidden() } else { AppError::not_found() }.into());
    }
    if state != "pending" && state != "escalated" {
        return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {state}")).into());
    }
    if if_match(&headers)? != version {
        return Err(stale(version));
    }
    set_state(&mut ctx, id, "withdrawn", level).await?;
    ctx.audit(REQUEST, "regularisation_request", id, Some(json!({ "state": state })), Some(json!({ "state": "withdrawn" })), None).await?;
    let doc = request_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

pub async fn cancel(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(CANCEL)?;
    let reason = need_reason(parse::<Reason>(raw)?.reason)?;
    let (employee, _, state, .., date, version) = lock_request(&mut ctx.tx, id).await?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, employee, CANCEL, &em).await?;
    if state != "approved" {
        return Err(AppError::conflict("not_approved", "Not approved", format!("Only an approved request can be cancelled; this one is {state}")).into());
    }
    if let Some(l) = crate::periods::lock_on(&mut ctx.tx, employee, date).await? {
        return Err(crate::periods::date_locked(l, date));
    }
    if if_match(&headers)? != version {
        return Err(stale(version));
    }
    sqlx::query(
        "UPDATE app.regularisation_requests SET state = 'cancelled', cancelled_by = $2, cancelled_at = now(), cancel_reason = $3,
           row_version = row_version + 1, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(ctx.user_id)
    .bind(&reason)
    .execute(&mut *ctx.tx)
    .await?;
    // Undo what approval wrote: void its manual punches, and add the reverse of each adjudication (latest wins).
    sqlx::query("UPDATE app.manual_punches SET voided_at = now(), voided_by = $2, void_reason = $3 WHERE regularisation_id = $1 AND voided_at IS NULL")
        .bind(id)
        .bind(ctx.user_id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
    sqlx::query(
        "INSERT INTO app.punch_adjudications (org_id, punch_id, punched_at, employee_id, kind, new_direction, origin, regularisation_id, reason)
         SELECT a.org_id, a.punch_id, a.punched_at, a.employee_id,
                CASE a.kind WHEN 'exclude' THEN 'include'::app.adjudication_kind ELSE 'change_direction'::app.adjudication_kind END,
                CASE a.kind WHEN 'exclude' THEN NULL ELSE p.direction END, 'regularisation', a.regularisation_id, $2
         FROM app.punch_adjudications a JOIN app.raw_punches p ON p.id = a.punch_id AND p.punched_at = a.punched_at
         WHERE a.regularisation_id = $1 AND a.kind IN ('exclude', 'change_direction')",
    )
    .bind(id)
    .bind(&reason)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.audit(CANCEL, "regularisation_request", id, Some(json!({ "state": "approved" })), Some(json!({ "state": "cancelled" })), Some(&reason)).await?;
    ctx.event("PunchRecorded", "employee", employee, json!({ "employee_id": employee, "work_date": date, "regularisation_id": id })).await?;
    let doc = request_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

// ---------- RG-07 (run by J-AT3) ----------

/// Requests left pending for RG-07 days: escalated to HR, approved or rejected automatically (actor "system").
// shortcut: an escalated request is not escalated again to the Org Admin; HR Managers and Org Admins both act on it.
pub(crate) async fn sweep(s: &AppState, org: Uuid) -> ApiResult<i32> {
    let mut ctx = Ctx::system(&s.pool, org).await?;
    let open: Vec<(Uuid, Uuid, NaiveDate, i16, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, employee_id, work_date, levels, created_at FROM app.regularisation_requests WHERE state = 'pending' ORDER BY created_at",
    )
    .fetch_all(&mut *ctx.tx)
    .await?;
    let now: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT now()").fetch_one(&mut *ctx.tx).await?;
    let mut n = 0;
    for (id, employee, date, levels, created) in open {
        let st: AtSettings = crate::policies::settings_for(&mut ctx.tx, employee, date).await?;
        if now < created + Duration::days(st.rg_no_action_days as i64) {
            continue;
        }
        match st.rg_no_action {
            NoAction::Escalate => {
                decision(&mut ctx, id, levels, "escalate", None, Some("RG-07")).await?;
                set_state(&mut ctx, id, "escalated", levels).await?;
            }
            NoAction::AutoApprove => {
                decision(&mut ctx, id, levels, "auto_approve", None, Some("RG-07")).await?;
                set_state(&mut ctx, id, "approved", levels).await?;
                apply(&mut ctx, id).await?;
            }
            NoAction::AutoReject => {
                decision(&mut ctx, id, levels, "auto_reject", None, Some("RG-07")).await?;
                set_state(&mut ctx, id, "auto_rejected", levels).await?;
            }
        }
        n += 1;
    }
    ctx.commit().await?;
    Ok(n)
}

// ---------- C1 / C2 day overrides ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideFields {
    status: Option<String>,
    net_minutes: Option<i32>,
    // Checked for type here; the day engine reads them from `fields`.
    #[allow(dead_code)]
    late_mark: Option<bool>,
    #[allow(dead_code)]
    early_mark: Option<bool>,
    deduction_days: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOverride {
    fields: Map<String, Value>,
    reason: Option<String>,
}

const OVERRIDE: &str = "jsonb_build_object('id', o.id, 'employee_id', o.employee_id, 'work_date', o.work_date, 'status', o.status,
    'net_minutes', o.net_minutes, 'fields', o.fields, 'reason', o.reason, 'created_by', o.created_by, 'created_at', o.created_at,
    'voided', o.voided_at IS NOT NULL, 'void_reason', o.void_reason)";

pub async fn override_create(mut ctx: Ctx, Path((employee, date)): Path<(Uuid, NaiveDate)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(CORRECT)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/employees/{id}/days/{date}/overrides", &json!({ "employee": employee, "date": date, "body": raw })).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewOverride = parse(raw)?;
    let reason = need_reason(req.reason)?;
    let f: OverrideFields = serde_json::from_value(Value::Object(req.fields.clone())).map_err(|e| AppError::validation(vec![FieldError::new("fields", "invalid_value", e.to_string())]))?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, employee, CORRECT, &em).await?;
    let mut errs = Vec::new();
    if req.fields.is_empty() {
        errs.push(FieldError::new("fields", "required", "Give at least one field to set"));
    }
    if f.status.as_deref().is_some_and(|s| !DAY_STATUSES.contains(&s)) {
        errs.push(FieldError::new("fields.status", "invalid_value", DAY_STATUSES.join(", ")));
    }
    if f.net_minutes.is_some_and(|n| !(0..=1440).contains(&n)) {
        errs.push(FieldError::new("fields.net_minutes", "invalid_value", "0 to 1440"));
    }
    if f.deduction_days.is_some_and(|d| ![0.0, 0.5, 1.0].contains(&d)) {
        errs.push(FieldError::new("fields.deduction_days", "invalid_value", "0, 0.5 or 1"));
    }
    if date > today() {
        errs.push(FieldError::new("date", "invalid_value", "A date that has passed or is today"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    if placed(&mut ctx.tx, employee, date).await?.is_none() {
        return Err(AppError::unprocessable("outside_employment", "Not employed then", "There is no employment on that date").into());
    }
    if !crate::policies::settings_for(&mut ctx.tx, employee, date).await?.hr_direct_correction {
        return Err(disabled("RG-10", "Direct correction by HR is switched off"));
    }
    let mut rest = req.fields.clone();
    rest.remove("status");
    rest.remove("net_minutes");
    let doc: Value = sqlx::query_scalar(&format!(
        "INSERT INTO app.day_overrides AS o (org_id, employee_id, work_date, status, net_minutes, fields, reason)
         VALUES ($1, $2, $3, $4::app.day_status, $5, $6, $7) RETURNING {OVERRIDE}"
    ))
    .bind(ctx.org_id)
    .bind(employee)
    .bind(date)
    .bind(&f.status)
    .bind(f.net_minutes)
    .bind(Value::Object(rest))
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let id: Uuid = doc["id"].as_str().and_then(|s| s.parse().ok()).ok_or_else(AppError::internal)?;
    ctx.audit(CORRECT, "day_override", id, None, Some(doc.clone()), Some(&reason)).await?;
    ctx.event("RecalculationRequested", "employee", employee, json!({ "employee_ids": [employee], "from": date, "to": date, "reason": "HR correction" })).await?;
    let body = json!({ "override": doc, "recalculation": "queued" });
    ctx.idempotency_finish(&headers, 201, &body).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

// shortcut: day_overrides has no row_version column, so If-Match isn't checked; the void is one-way.
pub async fn override_void(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(CORRECT)?;
    let reason = need_reason(parse::<Reason>(raw)?.reason)?;
    let row: Option<(Uuid, NaiveDate, bool)> = sqlx::query_as("SELECT employee_id, work_date, voided_at IS NOT NULL FROM app.day_overrides WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let Some((employee, date, voided)) = row else { return Err(AppError::not_found().into()) };
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, employee, CORRECT, &em).await?;
    if voided {
        return Err(AppError::conflict("already_voided", "Already voided", "This correction is already voided").into());
    }
    let doc: Value = sqlx::query_scalar(&format!("UPDATE app.day_overrides o SET voided_at = now(), voided_by = $2, void_reason = $3 WHERE id = $1 RETURNING {OVERRIDE}"))
        .bind(id)
        .bind(ctx.user_id)
        .bind(&reason)
        .fetch_one(&mut *ctx.tx)
        .await?;
    ctx.audit(CORRECT, "day_override", id, Some(json!({ "voided": false })), Some(json!({ "voided": true })), Some(&reason)).await?;
    ctx.event("RecalculationRequested", "employee", employee, json!({ "employee_ids": [employee], "from": date, "to": date, "reason": "HR correction voided" })).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "override": doc, "recalculation": "queued" })).into_response())
}

// ---------- W1 waivers ----------

const WAIVER: &str = "jsonb_build_object('id', w.id, 'kind', w.kind, 'work_date', w.work_date, 'branch_id', w.branch_id, 'shift_id', w.shift_id,
    'reason', w.reason, 'state', w.state, 'created_by', w.created_by, 'created_at', w.created_at, 'withdraw_reason', w.withdraw_reason)";

/// A waiver for a branch needs that branch in scope; one for a shift alone reaches every branch, so organisation scope.
fn waiver_scope(s: &Scope, branch: Option<Uuid>) -> ApiResult<()> {
    if s.org || branch.is_some_and(|b| s.branches.contains(&b)) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

pub async fn waivers(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let s = ctx.scope(WAIVE)?;
    let (mut from, mut to) = (today() - Duration::days(30), today() + Duration::days(30));
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "from" => from = v.parse().map_err(|_| bad_param("from", "Use YYYY-MM-DD"))?,
            "to" => to = v.parse().map_err(|_| bad_param("to", "Use YYYY-MM-DD"))?,
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({WAIVER} ORDER BY w.work_date, w.created_at), '[]') FROM app.waivers w
         WHERE w.work_date BETWEEN $1 AND $2 AND ($3 OR w.branch_id = ANY($4))"
    ))
    .bind(from)
    .bind(to)
    .bind(s.org)
    .bind(&s.branches)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewWaiver {
    kind: String,
    work_date: NaiveDate,
    branch_id: Option<Uuid>,
    shift_id: Option<Uuid>,
    reason: Option<String>,
}

fn waiver_event(branch: Option<Uuid>, shift: Option<Uuid>, date: NaiveDate) -> Value {
    match branch {
        Some(b) => json!({ "branch_ids": [b], "work_date": date }),
        None => json!({ "shift_id": shift, "work_date": date }),
    }
}

// shortcut: LA-17 / EC-16 are per policy set, so the day engine applies them per employee; creating the waiver
// itself is not refused when they are off.
pub async fn waiver_create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let s = ctx.scope(WAIVE)?;
    if let Some((st, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/waivers", &raw).await? {
        return Ok((StatusCode::from_u16(st).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewWaiver = parse(raw)?;
    let reason = need_reason(req.reason)?;
    if req.kind != "late" && req.kind != "early" {
        return Err(AppError::validation(vec![FieldError::new("kind", "invalid_value", "late or early")]).into());
    }
    if req.branch_id.is_none() && req.shift_id.is_none() {
        return Err(AppError::unprocessable("target_required", "Branch or shift required", "Give a branch_id, a shift_id, or both").into());
    }
    waiver_scope(&s, req.branch_id)?;
    let doc: Value = sqlx::query_scalar(&format!(
        "INSERT INTO app.waivers AS w (org_id, work_date, kind, branch_id, shift_id, reason) VALUES ($1, $2, $3::app.waiver_kind, $4, $5, $6) RETURNING {WAIVER}"
    ))
    .bind(ctx.org_id)
    .bind(req.work_date)
    .bind(&req.kind)
    .bind(req.branch_id)
    .bind(req.shift_id)
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await
    .map_err(|e| match e.as_database_error().and_then(|d| d.code()).as_deref() {
        Some("23503") => crate::error::ApiError::from(AppError::validation(vec![FieldError::new("branch_id", "not_found", "No such branch or shift")])),
        _ => e.into(),
    })?;
    let id: Uuid = doc["id"].as_str().and_then(|s| s.parse().ok()).ok_or_else(AppError::internal)?;
    ctx.audit(WAIVE, "waiver", id, None, Some(doc.clone()), Some(&reason)).await?;
    ctx.event("PolicyChanged", "waiver", id, waiver_event(req.branch_id, req.shift_id, req.work_date)).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

pub async fn waiver_withdraw(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let s = ctx.scope(WAIVE)?;
    let reason = need_reason(parse::<Reason>(raw)?.reason)?;
    let row: Option<(Option<Uuid>, Option<Uuid>, NaiveDate, String)> = sqlx::query_as("SELECT branch_id, shift_id, work_date, state::text FROM app.waivers WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let Some((branch, shift, date, state)) = row else { return Err(AppError::not_found().into()) };
    waiver_scope(&s, branch)?;
    if state != "active" {
        return Err(AppError::conflict("already_withdrawn", "Already withdrawn", "This waiver is already withdrawn").into());
    }
    let doc: Value = sqlx::query_scalar(&format!(
        "UPDATE app.waivers w SET state = 'withdrawn', withdrawn_at = now(), withdrawn_by = $2, withdraw_reason = $3 WHERE id = $1 RETURNING {WAIVER}"
    ))
    .bind(id)
    .bind(ctx.user_id)
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.audit(WAIVE, "waiver", id, Some(json!({ "state": "active" })), Some(json!({ "state": "withdrawn" })), Some(&reason)).await?;
    ctx.event("PolicyChanged", "waiver", id, waiver_event(branch, shift, date)).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

// ---------- O1 outage fill ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Outage {
    branch_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
    reason: Option<String>,
}

/// Fills the punches a device outage lost from shift times (RG-11): a working day of an employee placed at the
/// branch without an in punch gets one at shift start; without an out punch, one at shift end.
// shortcut: existing punches are matched by local calendar date, so a night shift's next-morning out is not seen.
pub async fn outage(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let scope = ctx.scope(OUTAGE)?;
    if let Some((st, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/outage-regularisations", &raw).await? {
        return Ok((StatusCode::from_u16(st).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: Outage = parse(raw)?;
    let reason = need_reason(req.reason)?;
    if !(scope.org || scope.branches.contains(&req.branch_id)) {
        return Err(AppError::forbidden().into());
    }
    if req.to < req.from || req.to > today() || (req.to - req.from).num_days() >= 31 {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", "to on or after from, not in the future, at most 31 days").into());
    }
    let people: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT employee_id FROM app.employee_job_versions WHERE master_branch_id = $1 AND state = 'active' AND valid && daterange($2, $3, '[]')",
    )
    .bind(req.branch_id)
    .bind(req.from)
    .bind(req.to)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let batch = Uuid::new_v4();
    let (mut affected, mut added, mut switched_off) = (Vec::new(), 0, 0);
    for e in &people {
        let mut touched = false;
        for d in req.from.iter_days().take_while(|d| *d <= req.to) {
            let s = crate::policies::settings_for(&mut ctx.tx, *e, d).await?;
            if !s.hr_bulk_regularisation {
                switched_off += 1;
                continue;
            }
            let Some(sched) = crate::days::schedule(&mut ctx.tx, *e, d, &s).await? else { continue };
            let Some(sh) = sched.shift.as_ref().filter(|_| sched.master == req.branch_id && !sched.holiday && !sched.weekly_off) else { continue };
            let have: Vec<String> = sqlx::query_scalar(
                "SELECT direction::text FROM app.raw_punches WHERE employee_id = $1 AND (punched_at AT TIME ZONE $3)::date = $2
                 UNION SELECT direction::text FROM app.manual_punches WHERE employee_id = $1 AND voided_at IS NULL AND (punched_at AT TIME ZONE $3)::date = $2",
            )
            .bind(e)
            .bind(d)
            .bind(&sched.zone)
            .fetch_all(&mut *ctx.tx)
            .await?;
            let end_day = if sh.end_time <= sh.start_time { d + Duration::days(1) } else { d };
            for (dir, at) in [("in", d.and_time(sh.start_time)), ("out", end_day.and_time(sh.end_time))] {
                if have.iter().any(|h| h == dir) {
                    continue;
                }
                sqlx::query(
                    "INSERT INTO app.manual_punches (org_id, employee_id, punched_at, direction, branch_id, origin, reason, batch_id)
                     VALUES ($1, $2, $3 AT TIME ZONE $4, $5::app.punch_direction, $6, 'outage_fill', $7, $8)",
                )
                .bind(ctx.org_id)
                .bind(e)
                .bind(at)
                .bind(&sched.zone)
                .bind(dir)
                .bind(req.branch_id)
                .bind(&reason)
                .bind(batch)
                .execute(&mut *ctx.tx)
                .await?;
                added += 1;
                touched = true;
            }
        }
        if touched {
            affected.push(*e);
        }
    }
    if added == 0 && switched_off > 0 && affected.is_empty() {
        return Err(disabled("RG-11", "Bulk regularisation for an outage is switched off"));
    }
    let body = json!({ "batch_id": batch, "affected_employees": affected.len(), "punches_added": added, "queued": affected.len() });
    ctx.audit(OUTAGE, "branch", req.branch_id, None, Some(json!({ "batch_id": batch, "from": req.from, "to": req.to, "employees": affected, "punches_added": added })), Some(&reason)).await?;
    if !affected.is_empty() {
        ctx.event("RecalculationRequested", "branch", req.branch_id, json!({ "employee_ids": affected, "from": req.from, "to": req.to, "reason": "Outage fill" })).await?;
    }
    ctx.idempotency_finish(&headers, 202, &body).await?;
    ctx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_per_kind() {
        let r = |v: Value| serde_json::from_value::<Requested>(v).unwrap();
        let t = "09:00:00";
        assert!(check_requested("missed_punch", &r(json!({}))).is_some());
        assert!(check_requested("missed_punch", &r(json!({ "out_time": t }))).is_none());
        assert!(check_requested("late", &r(json!({ "out_time": t }))).is_some());
        assert!(check_requested("whole_day", &r(json!({ "in_time": t }))).is_some());
        assert!(check_requested("wrong_punch", &r(json!({ "punch_id": Uuid::nil(), "direction": "sideways" }))).is_some());
        assert!(check_requested("wrong_punch", &r(json!({ "punch_id": Uuid::nil() }))).is_none());
        assert!(serde_json::from_value::<Requested>(json!({ "colour": 1 })).is_err());
    }
}
