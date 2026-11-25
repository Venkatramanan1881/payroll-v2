//! Status change requests S1–S7 (API_CONTRACTS §5.3, SM-1, SM-2, BR-EM-27, BR-EM-08).

use crate::ctx::{if_match, stale, Ctx, Scope};
use crate::employees::{allowed, bad_param, check_item, em_settings, push_from, push_scope, query_pairs, require_edit, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::job_changes::{apply, bump_version, load_versions, Replacement, Segment};
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use payroll_core::status::{approver_role, ApproverRole, EmployeeStatus};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

const REQUEST_JSON: &str = "jsonb_build_object(
    'id', r.id,
    'employee', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
    'from_status', r.from_status, 'to_status', r.to_status, 'effective_date', r.effective_date, 'reason', r.reason,
    'exit', CASE WHEN r.to_status = 'exited' THEN jsonb_build_object(
        'last_working_day', r.last_working_day, 'exit_reason_id', r.exit_reason_id, 'remarks', r.exit_remarks) END,
    'state', r.state, 'approver_role', r.approver_role,
    'raised_by', (SELECT jsonb_build_object('id', u.id, 'name', u.display_name) FROM app.users u WHERE u.id = r.raised_by),
    'raised_at', r.created_at,
    'decided_by', (SELECT jsonb_build_object('id', u.id, 'name', u.display_name) FROM app.users u WHERE u.id = r.decided_by),
    'decided_at', r.decided_at, 'decision_reason', r.decision_reason,
    'cancelled_by', (SELECT jsonb_build_object('id', u.id, 'name', u.display_name) FROM app.users u WHERE u.id = r.cancelled_by),
    'cancelled_at', r.cancelled_at, 'cancel_reason', r.cancel_reason, 'version', r.row_version)";

fn status_of(s: &str) -> EmployeeStatus {
    serde_json::from_value(json!(s)).expect("database enum matches core enum")
}
fn status_str(s: EmployeeStatus) -> String {
    serde_json::to_value(s).unwrap().as_str().unwrap().to_string()
}

async fn request_json(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT {REQUEST_JSON} FROM app.status_change_requests r JOIN app.employees e ON e.id = r.employee_id WHERE r.id = $1"
    ))
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}

fn request_response(status: StatusCode, doc: Value) -> Response {
    let etag = format!("\"{}\"", doc["version"]);
    (status, [(header::ETAG, etag)], Json(doc)).into_response()
}

/// Built-in roles the caller holds (org_admin, hr_manager, …).
async fn builtin_roles(tx: &mut Tx, user: Uuid) -> ApiResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT r.builtin_key FROM app.user_roles ur JOIN app.roles r ON r.id = ur.role_id
         WHERE ur.user_id = $1 AND r.builtin_key IS NOT NULL AND r.state = 'active'",
    )
    .bind(user)
    .fetch_all(&mut **tx)
    .await?)
}

/// The employee's latest status (not superseded): (status, since).
pub(crate) async fn current_status(tx: &mut Tx, employee: Uuid) -> ApiResult<(EmployeeStatus, NaiveDate)> {
    let row = sqlx::query(
        "SELECT status::text AS status, effective_from FROM app.employee_status_changes
         WHERE employee_id = $1 AND superseded_at IS NULL ORDER BY effective_from DESC LIMIT 1",
    )
    .bind(employee)
    .fetch_one(&mut **tx)
    .await?;
    Ok((status_of(row.get("status")), row.get("effective_from")))
}

/// SM-1 move and date order, checked on raise and again at decision time.
async fn check_move(tx: &mut Tx, employee: Uuid, from: EmployeeStatus, to: EmployeeStatus, effective: NaiveDate) -> ApiResult<()> {
    let (current, since) = current_status(tx, employee).await?;
    if current != from || !from.can_move_to(to) {
        return Err(AppError::unprocessable(
            "transition_not_allowed",
            "Move not allowed",
            format!("{} cannot move to {}", status_str(current), status_str(to)),
        )
        .into());
    }
    if effective <= since {
        return Err(bad_param("effective_date", &format!("Must be after the current status started ({since})")).into());
    }
    crate::periods::refuse_if_locked(tx, employee, effective).await
}

pub(crate) struct Req {
    id: Uuid,
    employee: Uuid,
    from: EmployeeStatus,
    to: EmployeeStatus,
    effective: NaiveDate,
    last_working_day: Option<NaiveDate>,
    exit_reason_id: Option<Uuid>,
    exit_remarks: Option<String>,
    raised_by: Uuid,
    approver: String,
    state: String,
    version: i32,
}

async fn lock_request(tx: &mut Tx, id: Uuid) -> ApiResult<Req> {
    let r = sqlx::query(
        "SELECT id, employee_id, from_status::text AS from_status, to_status::text AS to_status, effective_date,
                last_working_day, exit_reason_id, exit_remarks, raised_by, approver_role::text AS approver,
                state::text AS state, row_version
         FROM app.status_change_requests WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(AppError::not_found)?;
    Ok(Req {
        id: r.get("id"),
        employee: r.get("employee_id"),
        from: status_of(r.get("from_status")),
        to: status_of(r.get("to_status")),
        effective: r.get("effective_date"),
        last_working_day: r.get("last_working_day"),
        exit_reason_id: r.get("exit_reason_id"),
        exit_remarks: r.get("exit_remarks"),
        raised_by: r.get("raised_by"),
        approver: r.get("approver"),
        state: r.get("state"),
        version: r.get("row_version"),
    })
}

/// The change takes effect: status row, and for an exit everything in DATA_MODEL §6 "Exit takes effect".
/// Also used by the 00:15 job (J2) once it exists.
pub(crate) async fn take_effect(ctx: &mut Ctx, r: &Req) -> ApiResult<()> {
    let period: Uuid = sqlx::query_scalar("SELECT id FROM app.employment_periods WHERE employee_id = $1 AND state = 'open'")
        .bind(r.employee)
        .fetch_one(&mut *ctx.tx)
        .await?;
    sqlx::query(
        "INSERT INTO app.employee_status_changes (org_id, employee_id, employment_period_id, status, effective_from, request_id)
         VALUES ($1, $2, $3, $4::app.employee_status, $5, $6)",
    )
    .bind(ctx.org_id)
    .bind(r.employee)
    .bind(period)
    .bind(status_str(r.to))
    .bind(r.effective)
    .bind(r.id)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.event("StatusChanged", "employee", r.employee, json!({
        "employee_id": r.employee, "from": r.from, "to": r.to, "effective_from": r.effective, "request_id": r.id
    }))
    .await?;

    if r.to == EmployeeStatus::Exited {
        let lwd = r.last_working_day.expect("exit has a last working day (CHECK)");
        // Job versions: none may run past the last working day (edge case 11). Ending the last one frees the enrolment ID.
        let versions = load_versions(&mut ctx.tx, r.employee, period).await?;
        let end = lwd.succ_opt().unwrap();
        let mut reps = Vec::new();
        for v in &versions {
            if v.from > lwd {
                sqlx::query("UPDATE app.employee_job_versions SET state = 'cancelled', cancelled_reason = 'Employee exited' WHERE id = $1")
                    .bind(v.id)
                    .execute(&mut *ctx.tx)
                    .await?;
            } else if v.to.is_none_or(|t| t > end) {
                reps.push(Replacement {
                    old: v.id,
                    new: vec![Segment { from: v.from, to: Some(end), content: v.content.clone(), reason: v.reason.clone() }],
                });
            }
        }
        apply(ctx, r.employee, period, &reps).await?;
        sqlx::query(
            "UPDATE app.employment_periods SET state = 'closed', last_working_day = $2, exit_reason_id = $3, exit_remarks = $4
             WHERE id = $1",
        )
        .bind(period)
        .bind(lwd)
        .bind(r.exit_reason_id)
        .bind(&r.exit_remarks)
        .execute(&mut *ctx.tx)
        .await?;
        // Frees the mobile number (BR-EM-09); sign-in ends at once (BR-EM-08).
        sqlx::query("UPDATE app.employees SET is_current = false WHERE id = $1").bind(r.employee).execute(&mut *ctx.tx).await?;
        sqlx::query(
            "UPDATE app.user_sessions SET revoked_at = now(), revoke_reason = 'employee exited'
             WHERE revoked_at IS NULL AND user_id IN (SELECT id FROM app.users WHERE employee_id = $1)",
        )
        .bind(r.employee)
        .execute(&mut *ctx.tx)
        .await?;
        sqlx::query("UPDATE app.user_credentials SET state = 'revoked' WHERE user_id IN (SELECT id FROM app.users WHERE employee_id = $1)")
            .bind(r.employee)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.event("EmployeeExited", "employee", r.employee, json!({ "employee_id": r.employee, "last_working_day": lwd })).await?;
    }
    bump_version(&mut ctx.tx, r.employee).await?;
    Ok(())
}

// ---------- S1 POST /employees/{id}/status-requests ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExitDetails {
    last_working_day: Option<NaiveDate>,
    exit_reason_id: Option<Uuid>,
    remarks: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raise {
    to_status: Option<EmployeeStatus>,
    effective_date: Option<NaiveDate>,
    reason: Option<String>,
    exit: Option<ExitDetails>,
}

pub async fn raise(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.status.raise", &em).await?;
    if let Some((status, body)) = ctx.idempotency_begin(&headers, &format!("POST /v1/employees/{id}/status-requests"), &raw).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let req: Raise = parse(raw)?;

    let mut errs = Vec::new();
    if req.to_status.is_none() {
        errs.push(FieldError::new("to_status", "required", "Choose the new status"));
    }
    if req.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    let to = req.to_status;
    let exit = req.exit.as_ref();
    let mut effective = req.effective_date;
    if to == Some(EmployeeStatus::Exited) {
        let (lwd, why) = (exit.and_then(|x| x.last_working_day), exit.and_then(|x| x.exit_reason_id));
        if lwd.is_none() || why.is_none() {
            return Err(AppError::unprocessable("exit_details_required", "Exit details needed", "Give the last working day and an exit reason").into());
        }
        // Exited starts the day after the last working day, when access ends (BR-EM-08).
        let starts = lwd.unwrap().succ_opt().unwrap();
        if effective.is_some_and(|d| d != starts) {
            errs.push(FieldError::new("effective_date", "invalid_value", format!("For an exit this is the day after the last working day ({starts})")));
        }
        effective = Some(starts);
        check_item(&mut ctx.tx, "exit_reasons", why, "exit.exit_reason_id", &mut errs).await?;
    } else if exit.is_some() {
        errs.push(FieldError::new("exit", "invalid_value", "Exit details only go with an exit"));
    }
    if effective.is_none() {
        errs.push(FieldError::new("effective_date", "required", "Choose the date the change starts"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (to, effective) = (to.unwrap(), effective.unwrap());

    sqlx::query("SELECT 1 FROM app.employees WHERE id = $1 FOR UPDATE").bind(id).execute(&mut *ctx.tx).await?;
    let open: Option<String> = sqlx::query_scalar(
        "SELECT state::text FROM app.status_change_requests WHERE employee_id = $1 AND state IN ('pending', 'scheduled')",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    if let Some(state) = open {
        return Err(AppError::conflict("request_already_pending", "A change is already open", format!("This employee already has a {state} status change")).into());
    }
    let joining: Option<NaiveDate> = sqlx::query_scalar("SELECT joining_date FROM app.employment_periods WHERE employee_id = $1 AND state = 'open'")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let Some(joining) = joining else {
        return Err(AppError::conflict("employee_not_current", "Employee has left", "Rejoin the employee first").into());
    };
    if exit.and_then(|x| x.last_working_day).is_some_and(|l| l < joining) {
        return Err(AppError::unprocessable("date_before_joining", "Date before joining", format!("The employee joined on {joining}")).into());
    }
    let (from, _) = current_status(&mut ctx.tx, id).await?;
    check_move(&mut ctx.tx, id, from, to, effective).await?;

    let raiser_is_admin = builtin_roles(&mut ctx.tx, ctx.user_id).await?.iter().any(|r| r == "org_admin");
    let approver = match approver_role(raiser_is_admin) {
        ApproverRole::OrgAdmin => "org_admin",
        ApproverRole::HrManager => "hr_manager",
    };
    let reason = req.reason.unwrap().trim().to_string();
    let request: Uuid = sqlx::query_scalar(
        "INSERT INTO app.status_change_requests (org_id, employee_id, from_status, to_status, effective_date, reason,
             last_working_day, exit_reason_id, exit_remarks, raised_by, approver_role)
         VALUES ($1, $2, $3::app.employee_status, $4::app.employee_status, $5, $6, $7, $8, $9, $10, $11::app.approver_role)
         RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(status_str(from))
    .bind(status_str(to))
    .bind(effective)
    .bind(&reason)
    .bind(exit.and_then(|x| x.last_working_day))
    .bind(exit.and_then(|x| x.exit_reason_id))
    .bind(exit.and_then(|x| x.remarks.as_deref().map(str::trim)))
    .bind(ctx.user_id)
    .bind(approver)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let doc = request_json(&mut ctx.tx, request).await?;
    ctx.audit("employee.status.raise", "status_change_request", request, None, Some(doc.clone()), Some(&reason)).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::CREATED, doc))
}

// ---------- S2 GET /status-requests, S3 GET /status-requests/{id} ----------

/// S2/S3 may be used with either permission; the caller sees the union of both scopes.
fn read_scope(ctx: &Ctx) -> ApiResult<Scope> {
    let (a, b) = (ctx.scope("employee.status.raise").ok(), ctx.scope("employee.status.approve").ok());
    match (a, b) {
        (None, None) => Err(AppError::forbidden().into()),
        (Some(s), None) | (None, Some(s)) => Ok(s),
        (Some(mut s), Some(t)) => {
            s.org |= t.org;
            s.chain |= t.chain;
            s.own |= t.own;
            s.branches.extend(t.branches);
            s.departments.extend(t.departments);
            Ok(s)
        }
    }
}

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = read_scope(&ctx)?;
    let pairs = query_pairs(raw)?;
    let mut states = Vec::new();
    let (mut awaiting_me, mut employee, mut limit, mut cursor) = (false, None, 50i64, None);
    for (k, v) in &pairs {
        match k.as_str() {
            "state" => states.push(v.clone()),
            "awaiting_me" => awaiting_me = v == "true",
            "employee_id" => employee = Some(v.parse::<Uuid>().map_err(|_| bad_param("employee_id", "Not a valid id"))?),
            "limit" => limit = v.parse().ok().filter(|n| (1..=200).contains(n)).ok_or_else(|| bad_param("limit", "1 to 200"))?,
            "cursor" => cursor = Some(v.clone()),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let as_of = today();
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let roles = builtin_roles(&mut ctx.tx, ctx.user_id).await?;

    let mut qb = QueryBuilder::new(format!("SELECT r.created_at, r.id, {REQUEST_JSON} AS doc"));
    push_from(&mut qb, as_of);
    qb.push(" JOIN app.status_change_requests r ON r.employee_id = e.id WHERE ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    if !states.is_empty() {
        qb.push(" AND r.state::text = ANY(").push_bind(states).push(")");
    }
    if let Some(e) = employee {
        qb.push(" AND r.employee_id = ").push_bind(e);
    }
    if awaiting_me {
        qb.push(" AND r.state = 'pending' AND r.raised_by <> ").push_bind(ctx.user_id)
            .push(" AND r.approver_role::text = ANY(").push_bind(roles).push(")");
    }
    if let Some(c) = cursor {
        // Cursor = "<created_at RFC 3339>|<id>" of the last row of the previous page.
        let (ts, id) = c.split_once('|').ok_or_else(|| bad_param("cursor", "Not a valid cursor"))?;
        let ts: DateTime<Utc> = ts.parse().map_err(|_| bad_param("cursor", "Not a valid cursor"))?;
        let id: Uuid = id.parse().map_err(|_| bad_param("cursor", "Not a valid cursor"))?;
        qb.push(" AND (r.created_at, r.id) < (").push_bind(ts).push(", ").push_bind(id).push(")");
    }
    qb.push(" ORDER BY r.created_at DESC, r.id DESC LIMIT ").push_bind(limit + 1);
    let rows = qb.build().fetch_all(&mut *ctx.tx).await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<Value> = rows.iter().take(limit as usize).map(|r| r.get("doc")).collect();
    let next_cursor = more.then(|| {
        let last = &rows[limit as usize - 1];
        format!("{}|{}", last.get::<DateTime<Utc>, _>("created_at").to_rfc3339(), last.get::<Uuid, _>("id"))
    });
    ctx.commit().await?;
    Ok(Json(json!({ "items": items, "next_cursor": next_cursor })))
}

/// Request id → employee id, if the caller may see that employee under `permission` (else 404).
async fn visible_request(ctx: &mut Ctx, id: Uuid, permission: &str) -> ApiResult<Uuid> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    let employee: Option<Uuid> = sqlx::query_scalar("SELECT employee_id FROM app.status_change_requests WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    match employee {
        Some(e) if allowed(ctx, e, permission, Access::View, &em).await? => Ok(e),
        _ => Err(AppError::not_found().into()),
    }
}

pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let scope = read_scope(&ctx)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    let employee: Option<Uuid> = sqlx::query_scalar("SELECT employee_id FROM app.status_change_requests WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let Some(employee) = employee else { return Err(AppError::not_found().into()) };
    let as_of = today();
    let mut qb = QueryBuilder::new("SELECT EXISTS (SELECT 1");
    push_from(&mut qb, as_of);
    qb.push(" WHERE e.id = ").push_bind(employee).push(" AND ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    qb.push(")");
    if !qb.build_query_scalar::<bool>().fetch_one(&mut *ctx.tx).await? {
        return Err(AppError::not_found().into());
    }
    let doc = request_json(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::OK, doc))
}

// ---------- S4 approve, S5 reject ----------

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Decision {
    note: Option<String>,
    reason: Option<String>,
}

/// Common checks for approve and reject. Returns None when the same decision was already made.
async fn decide_checks(ctx: &mut Ctx, id: Uuid, headers: &HeaderMap, done_states: &[&str]) -> ApiResult<Option<Req>> {
    ctx.scope("employee.status.approve")?;
    visible_request(ctx, id, "employee.status.approve").await?;
    let r = lock_request(&mut ctx.tx, id).await?;
    if done_states.contains(&r.state.as_str()) {
        return Ok(None);
    }
    if r.state != "pending" {
        return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {}", r.state)).into());
    }
    let expected = if_match(headers)?;
    if expected != r.version {
        return Err(stale(r.version));
    }
    if r.raised_by == ctx.user_id {
        return Err(AppError::new(403, "cannot_decide_own_request", "Not your own request", "Someone else must decide a request you raised").into());
    }
    if !builtin_roles(&mut ctx.tx, ctx.user_id).await?.contains(&r.approver) {
        let who = if r.approver == "org_admin" { "the Organisation Admin" } else { "an HR Manager" };
        return Err(AppError::new(403, "wrong_approver_role", "Not the approver", format!("This request is decided by {who}")).into());
    }
    Ok(Some(r))
}

pub async fn approve(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let d: Decision = parse(raw)?;
    let Some(r) = decide_checks(&mut ctx, id, &headers, &["scheduled", "effective"]).await? else {
        let doc = request_json(&mut ctx.tx, id).await?;
        ctx.commit().await?;
        return Ok(request_response(StatusCode::OK, doc));
    };
    check_move(&mut ctx.tx, r.employee, r.from, r.to, r.effective).await?;
    let effective_now = r.effective <= today();
    sqlx::query(
        "UPDATE app.status_change_requests SET state = $2::app.status_request_state, decided_by = $3, decided_at = now(),
             decision_reason = $4, row_version = row_version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(if effective_now { "effective" } else { "scheduled" })
    .bind(ctx.user_id)
    .bind(d.note.as_deref().map(str::trim))
    .execute(&mut *ctx.tx)
    .await?;
    if effective_now {
        take_effect(&mut ctx, &r).await?;
    }
    let doc = request_json(&mut ctx.tx, id).await?;
    ctx.audit("employee.status.approve", "status_change_request", id, None, Some(json!({ "state": doc["state"] })), d.note.as_deref()).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::OK, doc))
}

pub async fn reject(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let d: Decision = parse(raw)?;
    let reason = d.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    let Some(_) = decide_checks(&mut ctx, id, &headers, &["rejected"]).await? else {
        let doc = request_json(&mut ctx.tx, id).await?;
        ctx.commit().await?;
        return Ok(request_response(StatusCode::OK, doc));
    };
    let reason = reason.ok_or_else(|| AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]))?;
    sqlx::query(
        "UPDATE app.status_change_requests SET state = 'rejected', decided_by = $2, decided_at = now(),
             decision_reason = $3, row_version = row_version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(ctx.user_id)
    .bind(&reason)
    .execute(&mut *ctx.tx)
    .await?;
    let doc = request_json(&mut ctx.tx, id).await?;
    ctx.audit("employee.status.approve", "status_change_request", id, None, Some(json!({ "state": "rejected" })), Some(&reason)).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::OK, doc))
}

// ---------- S6 withdraw, S7 cancel ----------

pub async fn withdraw(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap) -> ApiResult<Response> {
    ctx.scope("employee.status.raise")?;
    visible_request(&mut ctx, id, "employee.status.raise").await?;
    let r = lock_request(&mut ctx.tx, id).await?;
    if r.raised_by != ctx.user_id {
        return Err(AppError::forbidden().into());
    }
    match r.state.as_str() {
        "withdrawn" => {}
        "pending" => {
            let expected = if_match(&headers)?;
            if expected != r.version {
                return Err(stale(r.version));
            }
            sqlx::query("UPDATE app.status_change_requests SET state = 'withdrawn', row_version = row_version + 1 WHERE id = $1")
                .bind(id)
                .execute(&mut *ctx.tx)
                .await?;
            ctx.audit("employee.status.raise", "status_change_request", id, None, Some(json!({ "state": "withdrawn" })), None).await?;
        }
        s => return Err(AppError::conflict("not_pending", "Already decided", format!("This request is {s}")).into()),
    }
    let doc = request_json(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::OK, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelBody {
    reason: Option<String>,
}

pub async fn cancel(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let b: CancelBody = parse(raw)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    let employee = visible_request(&mut ctx, id, "employee.view").await?;
    require_edit(&mut ctx, employee, "employee.status.cancel", &em).await?;
    let r = lock_request(&mut ctx.tx, id).await?;
    match r.state.as_str() {
        "cancelled" => {}
        "scheduled" => {
            let reason = b.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string).ok_or_else(|| {
                AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")])
            })?;
            let expected = if_match(&headers)?;
            if expected != r.version {
                return Err(stale(r.version));
            }
            sqlx::query(
                "UPDATE app.status_change_requests SET state = 'cancelled', cancelled_by = $2, cancelled_at = now(),
                     cancel_reason = $3, row_version = row_version + 1 WHERE id = $1",
            )
            .bind(id)
            .bind(ctx.user_id)
            .bind(&reason)
            .execute(&mut *ctx.tx)
            .await?;
            ctx.audit("employee.status.cancel", "status_change_request", id, None, Some(json!({ "state": "cancelled" })), Some(&reason)).await?;
        }
        _ => {
            return Err(AppError::conflict("not_scheduled", "Not a scheduled change", "Only an approved change that has not started can be cancelled").into())
        }
    }
    let doc = request_json(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(request_response(StatusCode::OK, doc))
}

// ---------- J2 POST /internal/jobs/status-due ----------

/// Scheduled requests due on or before today take effect, as S4 would (SM-2). One transaction per
/// request, so one that can no longer apply is reported and the rest still go through.
/// Safe to run twice: a request is re-read under lock and skipped unless still scheduled and due.
pub(crate) async fn apply_due(pool: &sqlx::PgPool, org: Uuid) -> ApiResult<Value> {
    let today = today();
    let mut ctx = Ctx::system(pool, org).await?;
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM app.status_change_requests WHERE state = 'scheduled' AND effective_date <= $1 ORDER BY effective_date, id",
    )
    .bind(today)
    .fetch_all(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;

    let (mut applied, mut failed) = (Vec::new(), Vec::new());
    for id in due {
        let mut ctx = Ctx::system(pool, org).await?;
        let result: ApiResult<bool> = async {
            let r = lock_request(&mut ctx.tx, id).await?;
            if r.state != "scheduled" || r.effective > today {
                return Ok(false);
            }
            check_move(&mut ctx.tx, r.employee, r.from, r.to, r.effective).await?;
            sqlx::query("UPDATE app.status_change_requests SET state = 'effective', row_version = row_version + 1 WHERE id = $1")
                .bind(id)
                .execute(&mut *ctx.tx)
                .await?;
            take_effect(&mut ctx, &r).await?;
            ctx.audit("job.status_due", "status_change_request", id, Some(json!({ "state": "scheduled" })), Some(json!({ "state": "effective" })), None)
                .await?;
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => {
                ctx.commit().await?;
                applied.push(id);
            }
            Ok(false) => {}
            Err(e) => {
                // Left scheduled for HR to cancel or fix; the transaction rolls back on drop.
                tracing::warn!(request_id = %id, code = e.0.code, "scheduled status change could not take effect");
                failed.push(json!({ "request_id": id, "code": e.0.code }));
            }
        }
    }
    Ok(json!({ "applied": applied, "failed": failed }))
}
