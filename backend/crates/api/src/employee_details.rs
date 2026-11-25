//! E3 history, E9 probation, E10 custom fields, E11 enrolment status, E12 rejoin (API_CONTRACTS §5.1–5.2).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{
    add_can, allowed, check_joiner, check_limit, em_settings, employee_response, finish_joiner, is_e164, load_employee,
    mobile_holder, require_edit, start_period, today, Access, JobFields, Tx,
};
use crate::error::{parse, ApiResult, Body};
use crate::job_changes::bump_version;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use payroll_core::status::EmployeeStatus;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// Locks the employee row and checks If-Match.
async fn lock_version(tx: &mut Tx, id: Uuid, expected: impl Into<Option<i32>>) -> ApiResult<()> {
    let version: i32 = sqlx::query_scalar("SELECT row_version FROM app.employees WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    if expected.into().is_some_and(|e| e != version) {
        return Err(stale(version));
    }
    Ok(())
}

async fn employee_ok(ctx: &mut Ctx, id: Uuid) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    let mut doc = load_employee(&mut ctx.tx, id, today()).await?.ok_or_else(AppError::not_found)?;
    add_can(ctx, id, &mut doc, &em).await?;
    Ok(employee_response(StatusCode::OK, doc))
}

fn required(field: &str, message: &str) -> AppError {
    AppError::validation(vec![FieldError::new(field, "required", message)])
}

// ---------- E3 GET /employees/{id}/history ----------

pub async fn history(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    ctx.scope("employee.history.view")?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, "employee.history.view", Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    let rows = sqlx::query(
        "SELECT v.id, v.employment_period_id, lower(v.valid) AS valid_from, upper(v.valid) AS valid_to, v.state::text AS state,
                coalesce(v.cancelled_reason, v.change_reason) AS reason, v.recorded_at, v.superseded_at,
                (SELECT u.display_name FROM app.users u WHERE u.id = v.recorded_by) AS recorded_by,
                jsonb_build_object(
                  'master_branch_id', v.master_branch_id, 'department_id', v.department_id,
                  'designation_id', v.designation_id, 'category_id', v.category_id,
                  'employee_type_id', v.employee_type_id, 'contractor_id', v.contractor_id,
                  'reporting_manager_id', v.reporting_manager_id, 'payroll_group_id', v.payroll_group_id,
                  'enrolment_id', v.enrolment_id,
                  'allowed_branch_ids', coalesce((SELECT jsonb_agg(ab.branch_id ORDER BY ab.branch_id::text)
                      FROM app.job_version_allowed_branches ab WHERE ab.job_version_id = v.id), '[]'::jsonb)) AS content
         FROM app.employee_job_versions v WHERE v.employee_id = $1
         ORDER BY lower(v.valid), v.recorded_at",
    )
    .bind(id)
    .fetch_all(&mut *ctx.tx)
    .await?;

    // Each version's changes are against the active version just before it in the same period.
    let today = today();
    let active: Vec<&sqlx::postgres::PgRow> = rows.iter().filter(|r| r.get::<String, _>("state") == "active").collect();
    let job_versions: Vec<Value> = rows
        .iter()
        .map(|r| {
            let (period, from): (Uuid, NaiveDate) = (r.get("employment_period_id"), r.get("valid_from"));
            let content: Value = r.get("content");
            let prev = active
                .iter()
                .rfind(|p| p.get::<Uuid, _>("employment_period_id") == period && p.get::<NaiveDate, _>("valid_from") < from);
            let changes: Vec<Value> = match prev {
                Some(p) => {
                    let before: Value = p.get("content");
                    content
                        .as_object()
                        .unwrap()
                        .iter()
                        .filter(|(k, v)| before.get(k.as_str()) != Some(v))
                        .map(|(k, v)| json!({ "field": k, "old": before[k.as_str()], "new": v }))
                        .collect()
                }
                None => vec![],
            };
            let state = match r.get::<String, _>("state").as_str() {
                "active" if from > today => "scheduled".to_string(),
                "active" => "effective".to_string(),
                s => s.to_string(),
            };
            json!({
                "version_id": r.get::<Uuid, _>("id"), "valid_from": from,
                "valid_to": r.get::<Option<NaiveDate>, _>("valid_to").and_then(|t| t.pred_opt()),
                "state": state, "changes": changes, "reason": r.get::<Option<String>, _>("reason"),
                "recorded_by": r.get::<Option<String>, _>("recorded_by"),
                "recorded_at": r.get::<chrono::DateTime<chrono::Utc>, _>("recorded_at"),
                "superseded_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("superseded_at"),
            })
        })
        .collect();

    let rest: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object(
           'status', coalesce((SELECT jsonb_agg(jsonb_build_object('status', s.status, 'effective_from', s.effective_from,
                'request_id', s.request_id, 'superseded_at', s.superseded_at) ORDER BY s.effective_from, s.recorded_at)
              FROM app.employee_status_changes s WHERE s.employee_id = $1), '[]'::jsonb),
           'periods', coalesce((SELECT jsonb_agg(jsonb_build_object('period_id', p.id, 'joining_date', p.joining_date,
                'probation_end_date', p.probation_end_date, 'last_working_day', p.last_working_day,
                'exit_reason', (SELECT jsonb_build_object('id', x.id, 'code', x.code, 'name', x.name) FROM app.exit_reasons x WHERE x.id = p.exit_reason_id),
                'exit_remarks', p.exit_remarks, 'state', p.state) ORDER BY p.joining_date)
              FROM app.employment_periods p WHERE p.employee_id = $1), '[]'::jsonb),
           'personal_changes', coalesce((SELECT jsonb_agg(jsonb_build_object('field', kv.key, 'old', a.old_values -> kv.key,
                'new', kv.value, 'by', u.display_name, 'at', a.occurred_at) ORDER BY a.occurred_at, kv.key)
              FROM app.audit_log a CROSS JOIN LATERAL jsonb_each(a.new_values) kv
              LEFT JOIN app.users u ON u.id = a.actor_id
              WHERE a.target_type = 'employee' AND a.target_id = $1 AND a.action = 'employee.edit'
                AND kv.key = ANY($2)), '[]'::jsonb))",
    )
    .bind(id)
    .bind(["full_name", "mobile", "date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone"].map(String::from).to_vec())
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    let mut out = rest;
    out["job_versions"] = json!(job_versions);
    Ok(Json(out))
}

// ---------- E9 PATCH /employees/{id}/probation ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probation {
    probation_end_date: Option<NaiveDate>,
    reason: Option<String>,
}

pub async fn probation(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.probation.extend", &em).await?;
    let req: Probation = parse(raw)?;
    let end = req.probation_end_date.ok_or_else(|| required("probation_end_date", "Choose the new end date"))?;
    let reason = req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).ok_or_else(|| required("reason", "Give a reason"))?.to_string();

    lock_version(&mut ctx.tx, id, expected).await?;
    let (status, _) = crate::status::current_status(&mut ctx.tx, id).await?;
    if status != EmployeeStatus::Probation {
        return Err(AppError::conflict("not_in_probation", "Not in probation", "Only an employee in probation has a probation end date").into());
    }
    let row = sqlx::query("SELECT id, joining_date, probation_end_date FROM app.employment_periods WHERE employee_id = $1 AND state = 'open'")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let (period, joining, old): (Uuid, NaiveDate, Option<NaiveDate>) = (row.get("id"), row.get("joining_date"), row.get("probation_end_date"));
    if end < joining {
        return Err(AppError::unprocessable("date_before_joining", "Date before joining", format!("The employee joined on {joining}")).into());
    }
    if old == Some(end) {
        return Err(AppError::unprocessable("no_change", "Nothing to change", "That is already the end date").into());
    }
    // The reminder re-arms by itself: J3 sends when probation_reminder_sent_for differs from the end date (AC-EM-12.4).
    sqlx::query("UPDATE app.employment_periods SET probation_end_date = $2 WHERE id = $1")
        .bind(period)
        .bind(end)
        .execute(&mut *ctx.tx)
        .await?;
    bump_version(&mut ctx.tx, id).await?;
    ctx.audit("employee.probation.extend", "employee", id, Some(json!({ "probation_end_date": old })), Some(json!({ "probation_end_date": end })), Some(&reason)).await?;
    let res = employee_ok(&mut ctx, id).await?;
    ctx.commit().await?;
    Ok(res)
}

// ---------- E10 PUT /employees/{id}/custom-fields ----------

struct Def {
    label: String,
    kind: String,
    options: Vec<String>,
    inactive: Vec<String>,
    mandatory: bool,
}

/// One validated value, split into the typed columns of custom_field_values.
#[derive(Default)]
struct Typed {
    text: Option<String>,
    number: Option<String>, // cast to numeric in SQL
    date: Option<NaiveDate>,
    boolean: Option<bool>,
    options: Option<Vec<String>>,
}

fn typed(def: &Def, v: &Value, current: Option<&Value>) -> Result<Typed, (&'static str, String)> {
    let bad = |m: &str| Err(("invalid_value", format!("{}: {m}", def.label)));
    let inactive = |o: &String| def.inactive.contains(o) && !current.is_some_and(|c| c == o || c.as_array().is_some_and(|a| a.iter().any(|x| x == o)));
    match (def.kind.as_str(), v) {
        ("text", Value::String(s)) if !s.trim().is_empty() => Ok(Typed { text: Some(s.trim().into()), ..Default::default() }),
        ("number", Value::Number(n)) => Ok(Typed { number: Some(n.to_string()), ..Default::default() }),
        ("date", Value::String(s)) => match s.parse() {
            Ok(d) => Ok(Typed { date: Some(d), ..Default::default() }),
            Err(_) => bad("use YYYY-MM-DD"),
        },
        ("yes_no", Value::Bool(b)) => Ok(Typed { boolean: Some(*b), ..Default::default() }),
        ("dropdown", Value::String(s)) if def.options.contains(s) => {
            if inactive(s) {
                return Err(("option_inactive", format!("{}: \"{s}\" is no longer offered", def.label)));
            }
            Ok(Typed { text: Some(s.clone()), ..Default::default() })
        }
        ("multiple_choice", Value::Array(a)) => {
            let mut picked = Vec::new();
            for x in a {
                match x.as_str() {
                    Some(s) if def.options.iter().any(|o| o == s) => {
                        if inactive(&s.to_string()) {
                            return Err(("option_inactive", format!("{}: \"{s}\" is no longer offered", def.label)));
                        }
                        if !picked.iter().any(|p: &String| p == s) {
                            picked.push(s.to_string());
                        }
                    }
                    _ => return bad("choose from the listed options"),
                }
            }
            Ok(Typed { options: Some(picked), ..Default::default() })
        }
        ("dropdown", _) | ("multiple_choice", _) => bad("choose from the listed options"),
        _ => bad("wrong kind of value"),
    }
}

pub async fn custom_fields(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    custom_fields_in(&mut ctx, id, raw, Some(expected)).await?;
    let res = employee_ok(&mut ctx, id).await?;
    ctx.commit().await?;
    Ok(res)
}

/// E10 without commit; keys are field ids. `expected` None (bulk import) skips the version check.
pub(crate) async fn custom_fields_in(ctx: &mut Ctx, id: Uuid, raw: Value, expected: Option<i32>) -> ApiResult<()> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(ctx, id, "employee.edit", &em).await?;
    let changes = raw.as_object().cloned().ok_or_else(|| AppError::unprocessable("invalid_value", "Request not valid", "Send a JSON object"))?;

    let defs: HashMap<Uuid, Def> = sqlx::query(
        "SELECT id, label, field_type::text AS kind, coalesce(options, '{}') AS options,
                coalesce(inactive_options, '{}') AS inactive, is_mandatory
         FROM app.custom_field_definitions WHERE state = 'active'",
    )
    .fetch_all(&mut *ctx.tx)
    .await?
    .iter()
    .map(|r| {
        (r.get("id"), Def { label: r.get("label"), kind: r.get("kind"), options: r.get("options"), inactive: r.get("inactive"), mandatory: r.get("is_mandatory") })
    })
    .collect();
    let current: HashMap<Uuid, Value> = sqlx::query(
        "SELECT field_id, coalesce(to_jsonb(value_text), to_jsonb(value_number), to_jsonb(value_date),
                                   to_jsonb(value_bool), to_jsonb(value_options)) AS value
         FROM app.custom_field_values WHERE employee_id = $1",
    )
    .bind(id)
    .fetch_all(&mut *ctx.tx)
    .await?
    .iter()
    .map(|r| (r.get("field_id"), r.get::<Option<Value>, _>("value").unwrap_or(Value::Null)))
    .collect();

    let mut errs = Vec::new();
    let mut ops: Vec<(Uuid, Option<Typed>)> = Vec::new();
    for (k, v) in &changes {
        let Some((fid, def)) = k.parse::<Uuid>().ok().and_then(|f| defs.get(&f).map(|d| (f, d))) else {
            errs.push(FieldError::new(k.clone(), "invalid_value", "No such custom field"));
            continue;
        };
        if v.is_null() {
            if def.mandatory {
                errs.push(FieldError::new(k.clone(), "required", format!("{} is required", def.label)));
            } else {
                ops.push((fid, None));
            }
            continue;
        }
        match typed(def, v, current.get(&fid)) {
            Ok(t) => ops.push((fid, Some(t))),
            Err((code, msg)) => errs.push(FieldError::new(k.clone(), code, msg)),
        }
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }

    lock_version(&mut ctx.tx, id, expected).await?;
    for (fid, t) in &ops {
        match t {
            None => {
                sqlx::query("DELETE FROM app.custom_field_values WHERE employee_id = $1 AND field_id = $2")
                    .bind(id)
                    .bind(fid)
                    .execute(&mut *ctx.tx)
                    .await?;
            }
            Some(t) => {
                sqlx::query(
                    "INSERT INTO app.custom_field_values (org_id, employee_id, field_id, value_text, value_number, value_date, value_bool, value_options)
                     VALUES ($1, $2, $3, $4, $5::numeric, $6, $7, $8)
                     ON CONFLICT (employee_id, field_id) DO UPDATE SET value_text = EXCLUDED.value_text,
                       value_number = EXCLUDED.value_number, value_date = EXCLUDED.value_date, value_bool = EXCLUDED.value_bool,
                       value_options = EXCLUDED.value_options, updated_at = now(), updated_by = app.current_user_id()",
                )
                .bind(ctx.org_id)
                .bind(id)
                .bind(fid)
                .bind(&t.text)
                .bind(&t.number)
                .bind(t.date)
                .bind(t.boolean)
                .bind(&t.options)
                .execute(&mut *ctx.tx)
                .await?;
            }
        }
    }
    let old: Map<String, Value> = ops.iter().map(|(f, _)| (f.to_string(), current.get(f).cloned().unwrap_or(Value::Null))).collect();
    bump_version(&mut ctx.tx, id).await?;
    ctx.audit("employee.edit", "employee", id, Some(json!({ "custom_fields": old })), Some(json!({ "custom_fields": changes })), None).await?;
    Ok(())
}

// ---------- E11 PATCH /employees/{id}/enrolment-status ----------

#[derive(Deserialize, serde::Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum EnrolmentStatus {
    NotEnrolled,
    Enrolled,
    ReEnrolmentNeeded,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrolment {
    enrolment_status: Option<EnrolmentStatus>,
}

pub async fn enrolment_status(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.edit", &em).await?;
    let req: Enrolment = parse(raw)?;
    let new = req.enrolment_status.ok_or_else(|| required("enrolment_status", "Choose the enrolment status"))?;
    lock_version(&mut ctx.tx, id, expected).await?;
    let new_text = serde_json::to_value(new).unwrap().as_str().unwrap().to_string();
    let old: String = sqlx::query_scalar("SELECT enrolment_status::text FROM app.employees WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if old != new_text {
        sqlx::query("UPDATE app.employees SET enrolment_status = $2::app.enrolment_status, row_version = row_version + 1 WHERE id = $1")
            .bind(id)
            .bind(&new_text)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.audit("employee.edit", "employee", id, Some(json!({ "enrolment_status": old })), Some(json!({ "enrolment_status": new_text })), None).await?;
    }
    let res = employee_ok(&mut ctx, id).await?;
    ctx.commit().await?;
    Ok(res)
}

// ---------- E12 POST /employees/{id}/rejoin ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rejoin {
    joining_date: Option<NaiveDate>,
    probation_end_date: Option<NaiveDate>,
    initial_status: Option<EmployeeStatus>,
    #[serde(default)]
    job: JobFields,
    mobile: Option<String>,
    reason: Option<String>,
}

pub async fn rejoin(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let scope = ctx.scope("employee.rejoin")?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, "employee.view", Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    if let Some((status, body)) = ctx.idempotency_begin(&headers, &format!("POST /v1/employees/{id}/rejoin"), &raw).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let expected = if_match(&headers)?;
    let req: Rejoin = parse(raw.clone())?;

    let mut errs = Vec::new();
    check_joiner(&mut ctx.tx, &em, req.joining_date, req.initial_status, req.probation_end_date, &req.job, &mut errs).await?;
    if req.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    if req.mobile.as_deref().is_some_and(|m| !is_e164(m.trim())) {
        errs.push(FieldError::new("mobile", "invalid_value", "Use international format, e.g. +919876543210"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (master, department) = (req.job.master_branch_id.unwrap(), req.job.department_id.unwrap());
    if !(scope.org || scope.branches.contains(&master) || scope.departments.contains(&department)) {
        return Err(AppError::forbidden().into());
    }

    let row = sqlx::query("SELECT row_version, is_current, mobile FROM app.employees WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let version: i32 = row.get("row_version");
    if version != expected {
        return Err(stale(version));
    }
    let open: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employment_periods WHERE employee_id = $1 AND state = 'open')")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if row.get::<bool, _>("is_current") || open {
        return Err(AppError::conflict("not_exited", "Employee has not left", "Only an exited employee can rejoin").into());
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM app.status_change_requests WHERE employee_id = $1 AND state IN ('pending', 'scheduled'))",
    )
    .bind(id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    if pending {
        return Err(AppError::conflict("status_request_pending", "A status change is open", "Withdraw or cancel it first").into());
    }
    let last_day: Option<NaiveDate> = sqlx::query_scalar("SELECT max(last_working_day) FROM app.employment_periods WHERE employee_id = $1")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let joining = req.joining_date.unwrap();
    if last_day.is_some_and(|l| joining <= l) {
        return Err(AppError::unprocessable("date_before_last_exit", "Date before last exit", format!("The last working day was {}", last_day.unwrap())).into());
    }

    let over_limit = check_limit(&mut ctx).await?;
    // AC-EM-14.2: the old mobile may now belong to someone else; HR then enters a new one.
    let mobile = req.mobile.as_deref().map(str::trim).map(str::to_string).unwrap_or_else(|| row.get("mobile"));
    mobile_holder(&mut ctx.tx, &mobile, Some(id)).await?;
    sqlx::query("UPDATE app.employees SET is_current = true, mobile = $2, row_version = row_version + 1 WHERE id = $1")
        .bind(id)
        .bind(&mobile)
        .execute(&mut *ctx.tx)
        .await?;
    let status = req.initial_status.unwrap();
    start_period(&mut ctx, id, &em, joining, status, req.probation_end_date, &req.job).await?;

    ctx.audit("employee.rejoin", "employee", id, None, Some(raw), req.reason.as_deref()).await?;
    ctx.event("EmployeeCreated", "employee", id, json!({
        "employee_id": id, "joining_date": joining, "master_branch_id": master, "status": status, "rejoin": true
    }))
    .await?;
    ctx.event("StatusChanged", "employee", id, json!({ "employee_id": id, "from": "exited", "to": status, "effective_from": joining })).await?;
    finish_joiner(ctx, &headers, id, &em, over_limit, StatusCode::OK).await
}
