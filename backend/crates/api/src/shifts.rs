//! Shifts SH1–SH5 and an employee's default and allowed shifts R7 (API_CONTRACTS Package 2 §3, US-AT-01, 02).
//! A shift's times live in versions dated from a start date (BR-05); a saved version is never edited.

use crate::ctx::{if_match, Ctx};
use crate::employees::{bad_param, check_item, em_settings, employee_response, query_pairs, require_edit, today, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::job_changes::{apply, bump_version, load_versions, lock_employee, plan_change, version_json, PlanError};
use crate::org::{set_state, Kind};
use crate::periods::{date_locked, locked_from, refuse_if_locked};
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use payroll_core::shifts::ShiftTimes;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use uuid::Uuid;

const MANAGE: &str = "attendance.shift.manage";
const SHIFT: Kind = Kind { table: "shifts", target: "shift", manage: MANAGE, code: "code", named: true, extra: &["branch_id"] };

/// Time columns of a shift version, in `ShiftTimes` order.
const TIMES: &str = "kind, start_time, end_time, is_night, night_date_rule, required_minutes, window_before_min, window_after_min,
    flex_earliest, flex_latest, core_start, core_end, break_kind, break_start, break_end, half_day_point, half_day_time";

/// A version as the API shows it. Stored `approved` reads as scheduled / active / superseded by date (§1.6).
fn version_sql(today: NaiveDate) -> String {
    format!(
        "jsonb_build_object('id', v.id, 'valid_from', v.valid_from, 'author_id', v.author_id, 'recorded_at', v.recorded_at,
           'state', CASE WHEN v.state <> 'approved' THEN v.state::text
                         WHEN v.valid_from > DATE '{today}' THEN 'scheduled'
                         WHEN v.valid_from = (SELECT max(w.valid_from) FROM app.shift_versions w
                              WHERE w.shift_id = v.shift_id AND w.state = 'approved' AND w.valid_from <= DATE '{today}') THEN 'active'
                         ELSE 'superseded' END)
         || (SELECT to_jsonb(x) FROM (SELECT {TIMES}) x)"
    )
}

/// A shift with the version in force on `as_of` (None if it has none yet).
fn shift_sql(today: NaiveDate) -> String {
    format!(
        "jsonb_build_object('id', s.id, 'code', s.code, 'name', s.name, 'branch_id', s.branch_id, 'state', s.state, 'version', s.row_version,
           'current', (SELECT {v} FROM app.shift_versions v WHERE v.shift_id = s.id AND v.state = 'approved' AND v.valid_from <= $1
                       ORDER BY v.valid_from DESC LIMIT 1))",
        v = version_sql(today)
    )
}

async fn shift_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    let today = today();
    sqlx::query_scalar::<_, Value>(&format!("SELECT {} FROM app.shifts s WHERE s.id = $2", shift_sql(today)))
        .bind(today)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

fn shift_errors(t: &ShiftTimes) -> ApiResult<()> {
    let errs: Vec<FieldError> = t.check().into_iter().map(|(f, c, m)| FieldError::new(f, c, m)).collect();
    if errs.is_empty() { Ok(()) } else { Err(AppError::validation(errs).into()) }
}

/// Splits a flat body into its named keys and the shift times (the rest).
fn split_times(raw: Value, keys: &[&str]) -> ApiResult<(Map<String, Value>, ShiftTimes)> {
    let Value::Object(mut m) = raw else { return Err(bad_param("body", "Send a JSON object").into()) };
    let own = keys.iter().filter_map(|k| m.remove(*k).map(|v| (k.to_string(), v))).collect();
    Ok((own, parse(Value::Object(m))?))
}

fn date_of(own: &Map<String, Value>, key: &str, errs: &mut Vec<FieldError>) -> Option<NaiveDate> {
    match own.get(key).and_then(Value::as_str).map(str::parse::<NaiveDate>) {
        Some(Ok(d)) => Some(d),
        Some(Err(_)) => {
            errs.push(FieldError::new(key, "invalid_value", "Use YYYY-MM-DD"));
            None
        }
        None => {
            errs.push(FieldError::new(key, "required", "Choose the date it starts"));
            None
        }
    }
}

/// Adds an approved version from `from`. One approved version per start date: an earlier one for the
/// same date is superseded. Dates in a locked period are refused (BR-AT-18).
// shortcut: shift versions ignore AC-04 (policy sets honour it since S3) and are approved at once; add
// draft → submit → approve / reject (§1.6) as in policies.rs when shift maker-checker is wanted.
async fn add_version(ctx: &mut Ctx, shift: Uuid, branch: Option<Uuid>, from: NaiveDate, times: &ShiftTimes) -> ApiResult<Uuid> {
    if let Some(lock) = locked_from(&mut ctx.tx, branch, from).await? {
        return Err(date_locked(lock, from));
    }
    sqlx::query("UPDATE app.shift_versions SET state = 'superseded', superseded_at = now() WHERE shift_id = $1 AND valid_from = $2 AND state = 'approved'")
        .bind(shift)
        .bind(from)
        .execute(&mut *ctx.tx)
        .await?;
    let id: Uuid = sqlx::query_scalar(&format!(
        "INSERT INTO app.shift_versions (org_id, shift_id, valid_from, state, {TIMES})
         SELECT $1, $2, $3, 'approved', {TIMES} FROM jsonb_populate_record(NULL::app.shift_versions, $4) RETURNING id"
    ))
    .bind(ctx.org_id)
    .bind(shift)
    .bind(from)
    .bind(serde_json::to_value(times).expect("times serialise"))
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.event("ScheduleChanged", "shift", shift, json!({ "shift_id": shift, "from": from })).await?;
    Ok(id)
}

// ---------- SH1 GET /attendance/shifts ----------

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    if !(ctx.has("attendance.roster.view") || ctx.has(MANAGE)) {
        return Err(AppError::forbidden().into());
    }
    let (mut as_of, mut state, mut branch) = (today(), None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "as_of" => as_of = v.parse().map_err(|_| bad_param("as_of", "Use YYYY-MM-DD"))?,
            "state" if v == "active" || v == "inactive" => state = Some(v),
            "state" => return Err(bad_param("state", "active or inactive").into()),
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({} ORDER BY lower(s.code)), '[]') FROM app.shifts s
         WHERE ($2::text IS NULL OR s.state::text = $2) AND ($3::uuid IS NULL OR s.branch_id IS NULL OR s.branch_id = $3)",
        shift_sql(today())
    ))
    .bind(as_of)
    .bind(state)
    .bind(branch)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

// ---------- SH2 POST /attendance/shifts ----------

pub async fn create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/shifts", &raw).await? {
        return Ok(employee_response(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let (own, times) = split_times(raw, &["code", "name", "branch_id", "valid_from"])?;
    let mut errs = Vec::new();
    let text = |k: &str, errs: &mut Vec<FieldError>| match own.get(k).and_then(Value::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => {
            errs.push(FieldError::new(k, "required", "This is required"));
            None
        }
    };
    let (code, name) = (text("code", &mut errs), text("name", &mut errs));
    let from = date_of(&own, "valid_from", &mut errs);
    let branch: Option<Uuid> = match own.get("branch_id") {
        None | Some(Value::Null) => None,
        Some(v) => v.as_str().and_then(|s| s.parse().ok()).or_else(|| {
            errs.push(FieldError::new("branch_id", "invalid_value", "Not an id"));
            None
        }),
    };
    check_item(&mut ctx.tx, "branches", branch, "branch_id", &mut errs).await?;
    errs.extend(times.check().into_iter().map(|(f, c, m)| FieldError::new(f, c, m)));
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.shifts (org_id, code, name, branch_id) VALUES ($1, $2, $3, $4) RETURNING id")
        .bind(ctx.org_id)
        .bind(code)
        .bind(name)
        .bind(branch)
        .fetch_one(&mut *ctx.tx)
        .await?;
    add_version(&mut ctx, id, branch, from.unwrap(), &times).await?;
    let doc = shift_doc(&mut ctx.tx, id).await?;
    ctx.audit(MANAGE, "shift", id, None, Some(json!({ "shift": doc, "valid_from": from })), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::CREATED, doc))
}

// ---------- SH3 PATCH /attendance/shifts/{id} ----------

/// The code is the shift's identity and is not changed here.
pub async fn patch(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let Value::Object(m) = raw else { return Err(bad_param("body", "Send a JSON object").into()) };
    if let Some(k) = m.keys().find(|k| *k != "name" && *k != "branch_id") {
        return Err(AppError::unprocessable("unknown_field", "Request not valid", format!("unknown field `{k}`")).into());
    }
    let mut errs = Vec::new();
    let name = match m.get("name") {
        None => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(_) => {
            errs.push(FieldError::new("name", "required", "This can't be blank"));
            None
        }
    };
    // Missing = unchanged; null = usable at every branch.
    let branch: Option<Option<Uuid>> = match m.get("branch_id") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => match v.as_str().and_then(|s| s.parse().ok()) {
            Some(b) => Some(Some(b)),
            None => {
                errs.push(FieldError::new("branch_id", "invalid_value", "Not an id"));
                None
            }
        },
    };
    if let Some(b) = branch {
        check_item(&mut ctx.tx, "branches", b, "branch_id", &mut errs).await?;
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let old = shift_doc(&mut ctx.tx, id).await?;
    let v: i32 = sqlx::query_scalar("SELECT row_version FROM app.shifts WHERE id = $1 FOR UPDATE").bind(id).fetch_one(&mut *ctx.tx).await?;
    if v != expected {
        return Err(crate::ctx::stale(v));
    }
    sqlx::query(
        "UPDATE app.shifts SET name = coalesce($2, name), branch_id = CASE WHEN $3 THEN $4 ELSE branch_id END,
                row_version = row_version + 1, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&name)
    .bind(branch.is_some())
    .bind(branch.flatten())
    .execute(&mut *ctx.tx)
    .await?;
    let new = shift_doc(&mut ctx.tx, id).await?;
    let pick = |d: &Value| json!({ "name": d["name"], "branch_id": d["branch_id"] });
    ctx.audit(MANAGE, "shift", id, Some(pick(&old)), Some(pick(&new)), None).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, new))
}

// ---------- SH4 versions ----------

pub async fn versions(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    if !(ctx.has("attendance.roster.view") || ctx.has(MANAGE)) {
        return Err(AppError::forbidden().into());
    }
    shift_doc(&mut ctx.tx, id).await?;
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({} ORDER BY v.valid_from DESC, v.recorded_at DESC), '[]') FROM app.shift_versions v WHERE v.shift_id = $1",
        version_sql(today())
    ))
    .bind(id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn version_create(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/attendance/shifts/{id}/versions"), &raw).await? {
        return Ok(employee_response(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let (own, times) = split_times(raw, &["valid_from", "reason"])?;
    let mut errs = Vec::new();
    let from = date_of(&own, "valid_from", &mut errs);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    shift_errors(&times)?;
    let shift = shift_doc(&mut ctx.tx, id).await?;
    sqlx::query("SELECT 1 FROM app.shifts WHERE id = $1 FOR UPDATE").bind(id).execute(&mut *ctx.tx).await?;
    let branch = shift["branch_id"].as_str().and_then(|s| s.parse().ok());
    let vid = add_version(&mut ctx, id, branch, from.unwrap(), &times).await?;
    let reason = own.get("reason").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty());
    let doc: Value = sqlx::query_scalar(&format!("SELECT {} FROM app.shift_versions v WHERE v.id = $1", version_sql(today())))
        .bind(vid)
        .fetch_one(&mut *ctx.tx)
        .await?;
    ctx.audit(MANAGE, "shift_version", vid, shift.get("current").cloned(), Some(doc.clone()), reason).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

/// A scheduled version (not yet in force) can be discarded; one in force can't be undone, only replaced.
pub async fn version_discard(mut ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, Body(raw): Body) -> ApiResult<Json<Value>> {
    ctx.scope(MANAGE)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct R {
        reason: Option<String>,
    }
    let reason = parse::<R>(raw)?.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let row: Option<(String, NaiveDate)> =
        sqlx::query_as("SELECT state::text, valid_from FROM app.shift_versions WHERE id = $1 AND shift_id = $2 FOR UPDATE")
            .bind(vid)
            .bind(id)
            .fetch_optional(&mut *ctx.tx)
            .await?;
    let Some((state, from)) = row else { return Err(AppError::not_found().into()) };
    if state != "discarded" {
        if state != "approved" || from <= today() {
            return Err(AppError::conflict("not_scheduled", "Already in force", "Only a version that has not started yet can be discarded").into());
        }
        sqlx::query("UPDATE app.shift_versions SET state = 'discarded' WHERE id = $1").bind(vid).execute(&mut *ctx.tx).await?;
        ctx.audit(MANAGE, "shift_version", vid, Some(json!({ "state": "scheduled" })), Some(json!({ "state": "discarded" })), reason.as_deref()).await?;
        ctx.event("ScheduleChanged", "shift", id, json!({ "shift_id": id, "from": from })).await?;
    }
    let doc: Value = sqlx::query_scalar(&format!("SELECT {} FROM app.shift_versions v WHERE v.id = $1", version_sql(today())))
        .bind(vid)
        .fetch_one(&mut *ctx.tx)
        .await?;
    ctx.commit().await?;
    Ok(Json(doc))
}

// ---------- SH5 deactivate / activate ----------

pub async fn deactivate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &SHIFT, id, headers, raw, "inactive").await
}
pub async fn activate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &SHIFT, id, headers, raw, "active").await
}

// ---------- R7 PUT /attendance/employees/{id}/shifts ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmployeeShifts {
    effective_from: Option<NaiveDate>,
    default_shift_id: Option<Uuid>,
    #[serde(default)]
    allowed_shift_ids: Vec<Uuid>,
    reason: Option<String>,
}

/// A dated job change of the two shift fields only (as E6), so history and scheduling work the same.
pub async fn employee_shifts(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let today = today();
    let em = em_settings(&mut ctx.tx, today).await?;
    require_edit(&mut ctx, id, "attendance.roster.manage", &em).await?;
    let req: EmployeeShifts = parse(raw)?;
    let mut errs = Vec::new();
    if req.effective_from.is_none() {
        errs.push(FieldError::new("effective_from", "required", "Choose the date the change starts"));
    }
    let reason = req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    check_item(&mut ctx.tx, "shifts", req.default_shift_id, "default_shift_id", &mut errs).await?;
    let mut allowed = req.allowed_shift_ids.clone();
    allowed.sort_by_key(Uuid::to_string);
    allowed.dedup();
    for s in &allowed {
        check_item(&mut ctx.tx, "shifts", Some(*s), "allowed_shift_ids", &mut errs).await?;
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (d, reason) = (req.effective_from.unwrap(), reason.unwrap());
    let changes: Map<String, Value> = json!({ "default_shift_id": req.default_shift_id, "allowed_shift_ids": allowed }).as_object().unwrap().clone();
    // A branch's own shift can't be used at another master branch.
    let ids: Vec<Uuid> = allowed.iter().copied().chain(req.default_shift_id).collect();
    let owners: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT id, branch_id FROM app.shifts WHERE id = ANY($1) AND branch_id IS NOT NULL")
        .bind(&ids)
        .fetch_all(&mut *ctx.tx)
        .await?;
    let check = |content: &Map<String, Value>| {
        let master = content.get("master_branch_id").and_then(Value::as_str).and_then(|s| s.parse::<Uuid>().ok());
        if owners.iter().any(|(_, b)| Some(*b) != master) {
            return Err(AppError::validation(vec![FieldError::new("default_shift_id", "shift_not_allowed_at_branch", "A chosen shift belongs to another branch")]).into());
        }
        Ok(())
    };
    let body = dated_fields(&mut ctx, id, expected, d, &reason, changes, "attendance.roster.manage", check).await?;
    ctx.event("ScheduleChanged", "employee", id, json!({ "employee_ids": [id], "from": d })).await?;
    ctx.commit().await?;
    Ok(([(header::ETAG, format!("\"{}\"", body["version"]))], Json(body)).into_response())
}

/// R7 and PS7: a dated job change of attendance-owned job fields only (as E6), so history and scheduling
/// work the same. `check` sees each new job segment from `d` on. Audits and emits JobChanged; no commit.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dated_fields(
    ctx: &mut Ctx,
    id: Uuid,
    expected: i32,
    d: NaiveDate,
    reason: &str,
    changes: Map<String, Value>,
    permission: &str,
    check: impl Fn(&Map<String, Value>) -> ApiResult<()>,
) -> ApiResult<Value> {
    let (period, joining) = lock_employee(&mut ctx.tx, id, Some(expected)).await?;
    if d < joining {
        return Err(AppError::unprocessable("date_before_joining", "Date before joining", format!("The employee joined on {joining}")).into());
    }
    refuse_if_locked(&mut ctx.tx, id, d).await?;
    let versions = load_versions(&mut ctx.tx, id, period).await?;
    let reps = match plan_change(&versions, d, &changes, reason) {
        Ok(r) => r,
        Err(PlanError::NoChange) => return Err(AppError::unprocessable("no_change", "Nothing to change", "These are already the values on that date").into()),
        Err(_) => return Err(AppError::internal().into()),
    };
    for seg in reps.iter().flat_map(|r| &r.new).filter(|s| s.from >= d) {
        check(&seg.content)?;
    }
    let created = apply(ctx, id, period, &reps).await?;
    let version = bump_version(&mut ctx.tx, id).await?;
    let base = versions.iter().find(|v| v.id == reps[0].old).unwrap();
    let old: Map<String, Value> = changes.keys().map(|k| (k.clone(), base.content.get(k).cloned().unwrap_or(Value::Null))).collect();
    let fields: Vec<&String> = changes.keys().collect();
    ctx.audit(permission, "employee", id, Some(json!({ "effective_from": d, "job": old })), Some(json!({ "effective_from": d, "job": changes })), Some(reason)).await?;
    ctx.event("JobChanged", "employee", id, json!({ "employee_id": id, "effective_from": d, "changed_fields": fields })).await?;
    let today = today();
    Ok(json!({ "versions": version_json(&created, today), "scheduled": d > today, "version": version }))
}
