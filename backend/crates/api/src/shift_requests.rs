//! Shift change and swap requests SW1–SW4 (API_CONTRACTS Package 2, US-AT-05, SM-AT-7). Only when SH-12 is on.
//! A change asks for another shift on a future date; a swap trades shifts with a colleague of the same branch on
//! that date. The requester's manager decides; one approval writes both roster entries (ATQ-12).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{allowed, em_settings, push_from, push_scope, query_pairs, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::periods::{date_locked, lock_on};
use crate::roster::{replace_entry, Entry};
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

const REQUEST: &str = "attendance.shift_swap.request";
const APPROVE: &str = "attendance.shift_swap.approve";

const DOC: &str = "jsonb_build_object('id', r.id, 'kind', r.kind, 'state', r.state, 'work_date', r.work_date, 'reason', r.reason,
    'requester', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
    'colleague', CASE WHEN c.id IS NULL THEN NULL ELSE jsonb_build_object('id', c.id, 'employee_code', c.employee_code, 'full_name', c.full_name) END,
    'wanted_shift', jsonb_build_object('id', ws.id, 'code', ws.code, 'name', ws.name),
    'colleague_shift', CASE WHEN cs.id IS NULL THEN NULL ELSE jsonb_build_object('id', cs.id, 'code', cs.code, 'name', cs.name) END,
    'raised_by', r.raised_by, 'decided_by', r.decided_by, 'decided_at', r.decided_at, 'decision_reason', r.decision_reason,
    'created_at', r.created_at, 'version', r.row_version)";
/// Joins for DOC once `r` and `e` (the requester) are in the query.
const JOINS: &str = " LEFT JOIN app.employees c ON c.id = r.colleague_id JOIN app.shifts ws ON ws.id = r.wanted_shift_id
    LEFT JOIN app.shifts cs ON cs.id = r.colleague_shift_id";

fn me(ctx: &Ctx) -> ApiResult<Uuid> {
    ctx.scope(REQUEST)?;
    ctx.employee_id.ok_or_else(|| AppError::forbidden().into())
}

fn reason_of(r: Option<String>) -> Option<String> {
    r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

async fn doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    Ok(sqlx::query_scalar(&format!("SELECT {DOC} FROM app.shift_swap_requests r JOIN app.employees e ON e.id = r.requester_id{JOINS} WHERE r.id = $1"))
        .bind(id)
        .fetch_one(&mut **tx)
        .await?)
}

/// What an employee works on a date: job version, master branch, and the roster entry or else the default shift.
struct Sched {
    job: Uuid,
    master: Uuid,
    shift: Option<Uuid>,
}

async fn schedule(tx: &mut Tx, employee: Uuid, date: NaiveDate) -> ApiResult<Option<Sched>> {
    let row: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT jv.id, jv.master_branch_id, CASE WHEN re.id IS NULL THEN jv.default_shift_id WHEN re.is_weekly_off THEN NULL ELSE re.shift_id END
         FROM app.employee_job_versions jv
         LEFT JOIN app.roster_entries re ON re.employee_id = jv.employee_id AND re.work_date = $2 AND re.superseded_at IS NULL
         WHERE jv.employee_id = $1 AND jv.state = 'active' AND jv.valid @> $2::date",
    )
    .bind(employee)
    .bind(date)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(job, master, shift)| Sched { job, master, shift }))
}

/// SQL: the job version's allowed list contains the shift, or it has no list (any shift of the branch, ATQ-07).
fn on_list(job: &str, shift: &str) -> String {
    format!(
        "(NOT EXISTS (SELECT 1 FROM app.job_version_allowed_shifts a WHERE a.job_version_id = {job})
          OR EXISTS (SELECT 1 FROM app.job_version_allowed_shifts a WHERE a.job_version_id = {job} AND a.shift_id = {shift}))"
    )
}

/// The shift is active, usable at the branch, and allowed for the employee.
async fn may_work(tx: &mut Tx, s: &Sched, shift: Uuid) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM app.shifts s WHERE s.id = $3 AND s.state = 'active' AND (s.branch_id IS NULL OR s.branch_id = $2) AND {})",
        on_list("$1", "$3")
    ))
    .bind(s.job)
    .bind(s.master)
    .bind(shift)
    .fetch_one(&mut **tx)
    .await?)
}

// ---------- GET /attendance/me/shift-options?date= ----------

/// For the request form: my shift on the date, the shifts I may change to, and colleagues I may swap with
/// (same branch, a different shift, each allowed the other's). Names, codes and shift codes only.
pub async fn my_options(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let employee = me(&ctx)?;
    let mut date = None;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "date" => date = Some(v.parse::<NaiveDate>().map_err(|_| crate::employees::bad_param("date", "Use YYYY-MM-DD"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let date = date.ok_or_else(|| AppError::validation(vec![FieldError::new("date", "required", "Give a date")]))?;
    let Some(mine) = schedule(&mut ctx.tx, employee, date).await? else {
        ctx.commit().await?;
        return Ok(Json(json!({ "date": date, "current": null, "shifts": [], "colleagues": [] })));
    };
    let shift_json = "jsonb_build_object('id', s.id, 'code', s.code, 'name', s.name)";
    let current: Value = sqlx::query_scalar(&format!("SELECT {shift_json} FROM app.shifts s WHERE s.id = $1"))
        .bind(mine.shift)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .unwrap_or(Value::Null);
    let shifts: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({shift_json} ORDER BY s.code), '[]') FROM app.shifts s
         WHERE s.state = 'active' AND (s.branch_id IS NULL OR s.branch_id = $2) AND s.id IS DISTINCT FROM $3 AND {}",
        on_list("$1", "s.id")
    ))
    .bind(mine.job)
    .bind(mine.master)
    .bind(mine.shift)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let colleagues: Value = match mine.shift {
        None => json!([]),
        Some(my_shift) => sqlx::query_scalar(&format!(
            "WITH x AS (
               SELECT jv.employee_id, jv.id AS job,
                 CASE WHEN re.id IS NULL THEN jv.default_shift_id WHEN re.is_weekly_off THEN NULL ELSE re.shift_id END AS shift
               FROM app.employee_job_versions jv
               LEFT JOIN app.roster_entries re ON re.employee_id = jv.employee_id AND re.work_date = $3 AND re.superseded_at IS NULL
               WHERE jv.state = 'active' AND jv.valid @> $3::date AND jv.master_branch_id = $4 AND jv.employee_id <> $1)
             SELECT coalesce(jsonb_agg(jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name,
                      'shift', jsonb_build_object('id', s.id, 'code', s.code, 'name', s.name)) ORDER BY e.full_name), '[]')
             FROM x JOIN app.employees e ON e.id = x.employee_id JOIN app.shifts s ON s.id = x.shift
             WHERE x.shift <> $5 AND s.state = 'active' AND {} AND {}",
            on_list("x.job", "$5"),
            on_list("$2", "x.shift")
        ))
        .bind(employee)
        .bind(mine.job)
        .bind(date)
        .bind(mine.master)
        .bind(my_shift)
        .fetch_one(&mut *ctx.tx)
        .await?,
    };
    ctx.commit().await?;
    Ok(Json(json!({ "date": date, "current": current, "shifts": shifts, "colleagues": colleagues })))
}

async fn user_of(tx: &mut Tx, employee: Uuid) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM app.users WHERE employee_id = $1 AND state = 'active' LIMIT 1").bind(employee).fetch_optional(&mut **tx).await?)
}

fn refuse(code: &'static str, title: &'static str, detail: &'static str) -> crate::error::ApiError {
    AppError::unprocessable(code, title, detail).into()
}

// ---------- SW1 POST /attendance/shift-requests ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewRequest {
    work_date: NaiveDate,
    kind: String,
    shift_id: Option<Uuid>,
    colleague_employee_id: Option<Uuid>,
    reason: Option<String>,
}

pub async fn create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let employee = me(&ctx)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/shift-requests", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewRequest = parse(raw)?;
    let reason = reason_of(req.reason);
    let mut errs = Vec::new();
    match req.kind.as_str() {
        "change" if req.shift_id.is_none() => errs.push(FieldError::new("shift_id", "required", "Choose the shift you want")),
        "swap" if req.colleague_employee_id.is_none() => errs.push(FieldError::new("colleague_employee_id", "required", "Choose the colleague to swap with")),
        "change" | "swap" => {}
        _ => errs.push(FieldError::new("kind", "invalid_value", "change or swap")),
    }
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    if !crate::policies::settings_for(&mut ctx.tx, employee, req.work_date).await?.shift_requests_enabled {
        return Err(AppError::new(403, "feature_disabled", "Switched off", "Shift change and swap requests are not allowed under your policy")
            .with_meta(json!({ "setting": "SH-12" }))
            .into());
    }
    if req.work_date <= today() {
        return Err(refuse("date_not_future", "Date not in the future", "Ask for a date after today"));
    }
    let Some(mine) = schedule(&mut ctx.tx, employee, req.work_date).await? else {
        return Err(refuse("outside_employment", "Not employed on that date", "You aren't employed on that date"));
    };
    let (wanted, colleague, colleague_shift) = if req.kind == "change" {
        let wanted = req.shift_id.unwrap();
        if mine.shift == Some(wanted) || !may_work(&mut ctx.tx, &mine, wanted).await? {
            return Err(refuse("shift_not_allowed", "Shift not allowed", "You can't be given this shift on that date"));
        }
        (wanted, None, None)
    } else {
        let other = req.colleague_employee_id.unwrap();
        let theirs = if other == employee { None } else { schedule(&mut ctx.tx, other, req.work_date).await? };
        // Same branch, both working different shifts that the other may work (AC-AT-05.2).
        let (Some(theirs), Some(my_shift)) = (theirs, mine.shift) else {
            return Err(refuse("colleague_not_eligible", "Can't swap", "You and the colleague both need a shift on that date"));
        };
        let Some(their_shift) = theirs.shift.filter(|s| *s != my_shift && theirs.master == mine.master) else {
            return Err(refuse("colleague_not_eligible", "Can't swap", "The colleague needs a different shift at your branch on that date"));
        };
        if !may_work(&mut ctx.tx, &mine, their_shift).await? || !may_work(&mut ctx.tx, &theirs, my_shift).await? {
            return Err(refuse("shift_not_allowed", "Shift not allowed", "Each of you must be allowed to work the other's shift"));
        }
        (their_shift, Some(other), Some(my_shift))
    };
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.shift_swap_requests (org_id, kind, requester_id, work_date, wanted_shift_id, colleague_id, colleague_shift_id, reason)
         VALUES ($1, $2::app.swap_kind, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(&req.kind)
    .bind(employee)
    .bind(req.work_date)
    .bind(wanted)
    .bind(colleague)
    .bind(colleague_shift)
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let body = doc(&mut ctx.tx, id).await?;
    ctx.audit(REQUEST, "shift_swap_request", id, None, Some(body.clone()), reason.as_deref()).await?;
    // Manager told; colleague told (SM-AT-7).
    let key = json!({ "request_id": id });
    let payload = json!({ "request_id": id, "work_date": req.work_date, "kind": req.kind });
    let mgr: Option<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM app.employee_job_versions jv JOIN app.users u ON u.employee_id = jv.reporting_manager_id AND u.state = 'active'
         WHERE jv.id = $1 LIMIT 1",
    )
    .bind(mine.job)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    if let Some(u) = mgr {
        crate::notify::notify(&mut ctx.tx, ctx.org_id, u, "ShiftRequestSubmitted", "manager", &key, &payload).await?;
    }
    if let Some(c) = colleague {
        if let Some(u) = user_of(&mut ctx.tx, c).await? {
            crate::notify::notify(&mut ctx.tx, ctx.org_id, u, "ShiftSwapProposed", "employee", &key, &payload).await?;
        }
    }
    ctx.idempotency_finish(&headers, 201, &body).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

// ---------- SW2 GET /attendance/shift-requests ----------

/// Requests the caller may decide, plus their own and those naming them as the colleague.
// shortcut: newest 200 only, no cursor; add one when a list grows past that.
pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let (mut states, mut awaiting_me) = (Vec::new(), false);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "state" => states.push(v),
            "awaiting_me" => awaiting_me = v == "true",
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let approver = ctx.scope(APPROVE).ok();
    if approver.is_none() && ctx.scope(REQUEST).is_err() {
        return Err(AppError::forbidden().into());
    }
    let as_of = today();
    let mut qb = QueryBuilder::new(format!("SELECT coalesce(jsonb_agg(x.doc ORDER BY x.created_at DESC), '[]') FROM (SELECT {DOC} AS doc, r.created_at"));
    push_from(&mut qb, as_of);
    qb.push(format!(" JOIN app.shift_swap_requests r ON r.requester_id = e.id{JOINS} WHERE ("));
    if let Some(scope) = &approver {
        let em = em_settings(&mut ctx.tx, as_of).await?;
        push_scope(&mut qb, scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    } else {
        qb.push("false");
    }
    if let Some(e) = ctx.employee_id {
        qb.push(" OR r.requester_id = ").push_bind(e).push(" OR r.colleague_id = ").push_bind(e);
    }
    qb.push(")");
    if !states.is_empty() {
        qb.push(" AND r.state::text = ANY(").push_bind(states).push(")");
    }
    if awaiting_me {
        qb.push(" AND r.state = 'pending' AND r.raised_by <> ").push_bind(ctx.user_id);
        if approver.is_none() {
            qb.push(" AND false");
        }
        if let Some(e) = ctx.employee_id {
            qb.push(" AND r.requester_id <> ").push_bind(e);
        }
    }
    qb.push(" ORDER BY r.created_at DESC LIMIT 200) x");
    let items: Value = qb.build().fetch_one(&mut *ctx.tx).await?.get(0);
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

// ---------- SW3 approve / reject, SW4 withdraw ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

struct Locked {
    kind: String,
    requester: Uuid,
    state: String,
    raised_by: Option<Uuid>,
    date: NaiveDate,
    wanted: Uuid,
    colleague: Option<Uuid>,
    colleague_shift: Option<Uuid>,
    version: i32,
}

async fn lock_request(tx: &mut Tx, id: Uuid) -> ApiResult<Locked> {
    let row = sqlx::query(
        "SELECT kind::text, requester_id, state::text, raised_by, work_date, wanted_shift_id, colleague_id, colleague_shift_id, row_version
         FROM app.shift_swap_requests WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(AppError::not_found)?;
    Ok(Locked {
        kind: row.get(0),
        requester: row.get(1),
        state: row.get(2),
        raised_by: row.get(3),
        date: row.get(4),
        wanted: row.get(5),
        colleague: row.get(6),
        colleague_shift: row.get(7),
        version: row.get(8),
    })
}

/// Writes the approved roster entries, after checking nothing changed since the request was raised.
async fn apply(ctx: &mut Ctx, r: &Locked) -> ApiResult<()> {
    let mut entries = vec![(r.requester, r.wanted, r.colleague_shift)];
    if let (Some(c), Some(cs)) = (r.colleague, r.colleague_shift) {
        // Swap: each side must still be on the shift they're giving away.
        entries = vec![(r.requester, r.wanted, Some(cs)), (c, cs, Some(r.wanted))];
    }
    let source = if r.kind == "swap" { "swap" } else { "shift_change" };
    for (employee, to, from) in entries {
        if let Some(lock) = lock_on(&mut ctx.tx, employee, r.date).await? {
            return Err(date_locked(lock, r.date));
        }
        let sched = schedule(&mut ctx.tx, employee, r.date).await?;
        let still = match &sched {
            None => false,
            Some(s) => (from.is_none() || s.shift == from) && may_work(&mut ctx.tx, s, to).await?,
        };
        if !still {
            return Err(AppError::conflict("roster_changed", "Roster changed", format!("The roster for {} changed since this was asked; it can only be rejected", r.date)).into());
        }
        let en = Entry { employee_id: employee, work_date: r.date, shift_id: Some(to), is_weekly_off: false };
        if replace_entry(ctx, &en, source).await? {
            ctx.audit(APPROVE, "employee", employee, None, Some(json!({ "roster": [{ "work_date": r.date, "shift_id": to }], "source": source })), None).await?;
            ctx.event("ScheduleChanged", "employee", employee, json!({ "employee_ids": [employee], "dates": [r.date] })).await?;
        }
    }
    Ok(())
}

async fn decide(mut ctx: Ctx, id: Uuid, headers: HeaderMap, raw: Value, to: &str) -> ApiResult<Response> {
    ctx.scope(APPROVE)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let r = lock_request(&mut ctx.tx, id).await?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, r.requester, APPROVE, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    if r.state != to {
        if r.state != "pending" {
            return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {}", r.state)).into());
        }
        let expected = if_match(&headers)?;
        if expected != r.version {
            return Err(stale(r.version));
        }
        if r.raised_by == Some(ctx.user_id) || ctx.employee_id == Some(r.requester) {
            return Err(AppError::new(403, "cannot_decide_own_request", "Not your own request", "Someone else must decide a request you raised").into());
        }
        if to == "rejected" && reason.is_none() {
            return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
        }
        if to == "approved" {
            apply(&mut ctx, &r).await?;
        }
        sqlx::query(
            "UPDATE app.shift_swap_requests SET state = $2::app.request_state, decided_by = $3, decided_at = now(), decision_reason = $4,
               row_version = row_version + 1, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(to)
        .bind(ctx.user_id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
        ctx.audit(APPROVE, "shift_swap_request", id, Some(json!({ "state": r.state })), Some(json!({ "state": to })), reason.as_deref()).await?;
    }
    let body = doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}

pub async fn approve(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "approved").await
}
pub async fn reject(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "rejected").await
}

pub async fn withdraw(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let employee = me(&ctx)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let r = lock_request(&mut ctx.tx, id).await?;
    if r.requester != employee {
        return Err(AppError::not_found().into());
    }
    if r.state != "withdrawn" {
        if r.state != "pending" {
            return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {}", r.state)).into());
        }
        let expected = if_match(&headers)?;
        if expected != r.version {
            return Err(stale(r.version));
        }
        sqlx::query("UPDATE app.shift_swap_requests SET state = 'withdrawn', row_version = row_version + 1, updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.audit(REQUEST, "shift_swap_request", id, Some(json!({ "state": r.state })), Some(json!({ "state": "withdrawn" })), reason.as_deref()).await?;
    }
    let body = doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}
