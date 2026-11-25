//! Policy sets PS1–PS8 (API_CONTRACTS Package 2 §6, US-AT-21): sets, dated versions with maker-checker
//! (§1.6, SM-AT-1), assignments to groups, the employee override, and the resolver (BR-AT-21).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{allowed, bad_param, check_item, em_settings, query_pairs, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::periods::{date_locked, locked_from};
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Days, NaiveDate};
use payroll_core::policy;
use payroll_core::settings::AcSettings;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use uuid::Uuid;

const MANAGE: &str = "attendance.policy.manage";
const APPROVE: &str = "attendance.policy.approve";
const VIEW: &str = "attendance.day.view";
const LEVELS: [(&str, &str); 4] = [("branch", "branches"), ("department", "departments"), ("category", "categories"), ("employee_type", "employee_types")];

fn can_view(ctx: &Ctx) -> ApiResult<()> {
    if ctx.has(MANAGE) || ctx.has(VIEW) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

fn reason_of(r: Option<String>) -> Option<String> {
    r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

fn unknown(k: &str) -> crate::error::ApiError {
    AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()
}

// ---------- resolver (BR-AT-21) ----------

pub(crate) struct Resolved {
    pub set_id: Uuid,
    pub set_name: String,
    pub version_id: Uuid,
    pub source: String,
    pub settings: Value,
}

/// The policy set in force for `employee` on `on`: the employee override, else the most specific active
/// assignment (employee type → category → department → master branch), else the organisation default.
/// A set without an approved version in force on `on` is passed over.
pub(crate) async fn resolve(tx: &mut Tx, employee: Uuid, on: NaiveDate) -> ApiResult<Option<Resolved>> {
    let row: Option<(Uuid, String, Uuid, String, Value)> = sqlx::query_as(
        "WITH jv AS (SELECT * FROM app.employee_job_versions WHERE employee_id = $1 AND state = 'active' AND valid @> $2::date),
         a AS (SELECT level::text AS level, target_id, policy_set_id FROM app.policy_assignments WHERE state = 'active' AND valid @> $2::date),
         cand AS (
           SELECT 1 AS rank, 'employee' AS source, jv.policy_set_id AS set_id FROM jv WHERE jv.policy_set_id IS NOT NULL
           UNION ALL SELECT 2, 'employee_type', a.policy_set_id FROM jv JOIN a ON a.level = 'employee_type' AND a.target_id = jv.employee_type_id
           UNION ALL SELECT 3, 'category', a.policy_set_id FROM jv JOIN a ON a.level = 'category' AND a.target_id = jv.category_id
           UNION ALL SELECT 4, 'department', a.policy_set_id FROM jv JOIN a ON a.level = 'department' AND a.target_id = jv.department_id
           UNION ALL SELECT 5, 'branch', a.policy_set_id FROM jv JOIN a ON a.level = 'branch' AND a.target_id = jv.master_branch_id
           UNION ALL SELECT 6, 'default', id FROM app.policy_sets WHERE is_default)
         SELECT ps.id, ps.name, v.id, c.source, v.settings FROM cand c
         JOIN app.policy_sets ps ON ps.id = c.set_id AND (ps.state = 'active' OR ps.is_default)
         JOIN LATERAL (SELECT w.id, w.settings FROM app.policy_set_versions w WHERE w.policy_set_id = ps.id AND w.state = 'approved'
                       AND w.valid_from <= $2 ORDER BY w.valid_from DESC LIMIT 1) v ON true
         ORDER BY c.rank LIMIT 1",
    )
    .bind(employee)
    .bind(on)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(set_id, set_name, version_id, source, settings)| Resolved { set_id, set_name, version_id, source, settings }))
}

/// The settings that apply to `employee` on `on` (BRD defaults when nothing resolves).
pub(crate) async fn settings_for(tx: &mut Tx, employee: Uuid, on: NaiveDate) -> ApiResult<policy::AtSettings> {
    Ok(policy::AtSettings::from_json(resolve(tx, employee, on).await?.map(|r| r.settings).unwrap_or(json!({}))))
}

pub(crate) fn resolved_json(r: Option<Resolved>) -> Value {
    match r {
        Some(r) => json!({ "policy_set": { "id": r.set_id, "name": r.set_name }, "version_id": r.version_id, "source": r.source }),
        None => Value::Null,
    }
}

/// AC-04: does a settings change need Organisation Admin approval?
async fn needs_approval(tx: &mut Tx) -> ApiResult<bool> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT setting_values FROM app.org_settings_versions WHERE settings_group = 'AC' AND valid_from <= $1
         ORDER BY valid_from DESC, recorded_at DESC LIMIT 1",
    )
    .bind(today())
    .fetch_optional(&mut **tx)
    .await?;
    Ok(serde_json::from_value::<AcSettings>(v.unwrap_or(json!({}))).unwrap_or_default().settings_change_approval)
}

// ---------- shapes ----------

fn set_sql(today: NaiveDate) -> String {
    format!(
        "jsonb_build_object('id', s.id, 'name', s.name, 'is_default', s.is_default, 'state', s.state, 'version', s.row_version,
           'active_version', (SELECT jsonb_build_object('id', v.id, 'valid_from', v.valid_from) FROM app.policy_set_versions v
                              WHERE v.policy_set_id = s.id AND v.state = 'approved' AND v.valid_from <= DATE '{today}' ORDER BY v.valid_from DESC LIMIT 1),
           'assignments_count', (SELECT count(*) FROM app.policy_assignments a WHERE a.policy_set_id = s.id AND a.state = 'active'
                                 AND (upper(a.valid) IS NULL OR upper(a.valid) > DATE '{today}')))"
    )
}

/// §1.6 version shape; stored `approved` reads as scheduled / active / superseded by date.
fn version_sql(today: NaiveDate) -> String {
    format!(
        "jsonb_build_object('id', v.id, 'policy_set_id', v.policy_set_id, 'valid_from', v.valid_from, 'values', v.settings,
           'change_reason', v.change_reason, 'submitted_at', v.submitted_at, 'decided_by', v.decided_by, 'decided_at', v.decided_at,
           'decision_reason', v.decision_reason, 'version', v.row_version,
           'author', (SELECT jsonb_build_object('id', u.id, 'name', u.display_name) FROM app.users u WHERE u.id = v.author_id),
           'state', CASE WHEN v.state <> 'approved' THEN v.state::text
                         WHEN v.valid_from > DATE '{today}' THEN 'scheduled'
                         WHEN v.valid_from = (SELECT max(w.valid_from) FROM app.policy_set_versions w
                              WHERE w.policy_set_id = v.policy_set_id AND w.state = 'approved' AND w.valid_from <= DATE '{today}') THEN 'active'
                         ELSE 'superseded' END)"
    )
}

async fn set_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("SELECT {} FROM app.policy_sets s WHERE s.id = $1", set_sql(today())))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

async fn version_doc(tx: &mut Tx, set: Uuid, vid: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("SELECT {} FROM app.policy_set_versions v WHERE v.id = $1 AND v.policy_set_id = $2", version_sql(today())))
        .bind(vid)
        .bind(set)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

fn version_response(status: StatusCode, doc: Value) -> Response {
    (status, [(header::ETAG, format!("\"{}\"", doc["version"]))], Json(doc)).into_response()
}

/// Checks `values` with the core schema; each broken rule is an `invalid_setting` error on `values.<key>`.
fn checked(values: Option<Map<String, Value>>, errs: &mut Vec<FieldError>) -> Map<String, Value> {
    let values = values.unwrap_or_default();
    if let Err(bad) = policy::check_values(&values) {
        errs.extend(bad.into_iter().map(|(k, m)| FieldError::new(format!("values.{k}"), "invalid_setting", m)));
    }
    values
}

async fn refuse_locked(tx: &mut Tx, branch: Option<Uuid>, from: NaiveDate) -> ApiResult<()> {
    match locked_from(tx, branch, from).await? {
        Some(lock) => Err(date_locked(lock, from)),
        None => Ok(()),
    }
}

// ---------- PS1 / PS2 / PS3 sets ----------

pub async fn list(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    can_view(&ctx)?;
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({} ORDER BY s.is_default DESC, lower(s.name)), '[]') FROM app.policy_sets s",
        set_sql(today())
    ))
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSet {
    name: Option<String>,
    copy_from_policy_set_id: Option<Uuid>,
    valid_from: Option<NaiveDate>,
    values: Option<Map<String, Value>>,
    change_reason: Option<String>,
}

/// PS2: the set and its first version as a draft. A copy starts from the source's values in force today.
pub async fn create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/policy-sets", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewSet = parse(raw)?;
    let mut errs = Vec::new();
    let name = reason_of(req.name);
    if name.is_none() {
        errs.push(FieldError::new("name", "required", "Give the policy set a name"));
    }
    if req.valid_from.is_none() {
        errs.push(FieldError::new("valid_from", "required", "Choose the date the settings start"));
    }
    let mut values = checked(req.values, &mut errs);
    if let Some(src) = req.copy_from_policy_set_id {
        let copied: Option<Value> = sqlx::query_scalar(
            "SELECT coalesce((SELECT v.settings FROM app.policy_set_versions v WHERE v.policy_set_id = s.id AND v.state = 'approved'
                              AND v.valid_from <= $2 ORDER BY v.valid_from DESC LIMIT 1), '{}') FROM app.policy_sets s WHERE s.id = $1",
        )
        .bind(src)
        .bind(today())
        .fetch_optional(&mut *ctx.tx)
        .await?;
        match copied {
            None => errs.push(FieldError::new("copy_from_policy_set_id", "invalid_value", "Not found")),
            // Values sent with the copy win over the copied ones.
            Some(Value::Object(mut c)) => {
                c.extend(values);
                values = c;
            }
            Some(_) => {}
        }
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let from = req.valid_from.unwrap();
    refuse_locked(&mut ctx.tx, None, from).await?;
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.policy_sets WHERE lower(name) = lower($1))").bind(&name).fetch_one(&mut *ctx.tx).await?;
    if taken {
        return Err(AppError::conflict("name_in_use", "Name in use", "Another policy set has this name").into());
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.policy_sets (org_id, name) VALUES ($1, $2) RETURNING id")
        .bind(ctx.org_id)
        .bind(&name)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let vid: Uuid = sqlx::query_scalar(
        "INSERT INTO app.policy_set_versions (org_id, policy_set_id, valid_from, settings, change_reason) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(from)
    .bind(Value::Object(values))
    .bind(reason_of(req.change_reason))
    .fetch_one(&mut *ctx.tx)
    .await?;
    let mut doc = set_doc(&mut ctx.tx, id).await?;
    doc["draft_version"] = version_doc(&mut ctx.tx, id, vid).await?;
    ctx.audit(MANAGE, "policy_set", id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    name: String,
}

pub async fn patch(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let Some(name) = reason_of(Some(parse::<Rename>(raw)?.name)) else {
        return Err(AppError::validation(vec![FieldError::new("name", "required", "This can't be blank")]).into());
    };
    let old = lock_set(&mut ctx.tx, id, expected).await?;
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.policy_sets WHERE lower(name) = lower($1) AND id <> $2)")
        .bind(&name)
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if taken {
        return Err(AppError::conflict("name_in_use", "Name in use", "Another policy set has this name").into());
    }
    sqlx::query("UPDATE app.policy_sets SET name = $2, row_version = row_version + 1 WHERE id = $1").bind(id).bind(&name).execute(&mut *ctx.tx).await?;
    let doc = set_doc(&mut ctx.tx, id).await?;
    ctx.audit(MANAGE, "policy_set", id, Some(json!({ "name": old["name"] })), Some(json!({ "name": name })), None).await?;
    ctx.commit().await?;
    Ok(version_response(StatusCode::OK, doc))
}

async fn lock_set(tx: &mut Tx, id: Uuid, expected: i32) -> ApiResult<Value> {
    let v: Option<i32> = sqlx::query_scalar("SELECT row_version FROM app.policy_sets WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut **tx).await?;
    match v {
        None => Err(AppError::not_found().into()),
        Some(v) if v != expected => Err(stale(v)),
        Some(_) => set_doc(tx, id).await,
    }
}

/// PS3 deactivate: never the default; never while an assignment or an employee override uses it now or later.
pub async fn deactivate(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let doc = lock_set(&mut ctx.tx, id, expected).await?;
    if doc["is_default"] == true {
        return Err(AppError::conflict("cannot_deactivate_default", "Default policy set", "The organisation default can't be switched off").into());
    }
    if doc["state"] == "active" {
        let in_use: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM app.policy_assignments WHERE policy_set_id = $1 AND state = 'active' AND (upper(valid) IS NULL OR upper(valid) > $2))
                 OR EXISTS (SELECT 1 FROM app.employee_job_versions WHERE policy_set_id = $1 AND state = 'active' AND (upper(valid) IS NULL OR upper(valid) > $2))",
        )
        .bind(id)
        .bind(today())
        .fetch_one(&mut *ctx.tx)
        .await?;
        if in_use {
            return Err(AppError::conflict("policy_set_in_use", "Policy set in use", "End its assignments and employee overrides first").into());
        }
        sqlx::query("UPDATE app.policy_sets SET state = 'inactive', row_version = row_version + 1 WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
        ctx.audit(MANAGE, "policy_set", id, Some(json!({ "state": "active" })), Some(json!({ "state": "inactive" })), reason.as_deref()).await?;
    }
    let doc = set_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(version_response(StatusCode::OK, doc))
}

// ---------- PS4 versions ----------

pub async fn versions(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    can_view(&ctx)?;
    set_doc(&mut ctx.tx, id).await?;
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({} ORDER BY v.valid_from DESC, v.recorded_at DESC), '[]') FROM app.policy_set_versions v WHERE v.policy_set_id = $1",
        version_sql(today())
    ))
    .bind(id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn version_get(mut ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, RawQuery(raw): RawQuery) -> ApiResult<Response> {
    can_view(&ctx)?;
    let mut with_defaults = false;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "with_defaults" => with_defaults = v == "true",
            _ => return Err(unknown(&k)),
        }
    }
    let mut doc = version_doc(&mut ctx.tx, id, vid).await?;
    if with_defaults {
        doc["values"] = policy::filled(doc["values"].take());
    }
    ctx.commit().await?;
    Ok(version_response(StatusCode::OK, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewVersion {
    valid_from: Option<NaiveDate>,
    values: Option<Map<String, Value>>,
    change_reason: Option<String>,
}

pub async fn version_create(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/attendance/policy-sets/{id}/versions"), &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewVersion = parse(raw)?;
    let mut errs = Vec::new();
    if req.valid_from.is_none() {
        errs.push(FieldError::new("valid_from", "required", "Choose the date the settings start"));
    }
    let values = checked(req.values, &mut errs);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let set = set_doc(&mut ctx.tx, id).await?;
    if set["state"] != "active" {
        return Err(AppError::conflict("policy_set_inactive", "Policy set inactive", "Activate the set before changing it").into());
    }
    let from = req.valid_from.unwrap();
    refuse_locked(&mut ctx.tx, None, from).await?;
    let vid: Uuid = sqlx::query_scalar(
        "INSERT INTO app.policy_set_versions (org_id, policy_set_id, valid_from, settings, change_reason) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(from)
    .bind(Value::Object(values))
    .bind(reason_of(req.change_reason))
    .fetch_one(&mut *ctx.tx)
    .await?;
    let doc = version_doc(&mut ctx.tx, id, vid).await?;
    ctx.audit(MANAGE, "policy_version", vid, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(version_response(StatusCode::CREATED, doc))
}

/// (state, valid_from, author, row_version) of a version, locked.
async fn lock_version(tx: &mut Tx, id: Uuid, vid: Uuid) -> ApiResult<(String, NaiveDate, Option<Uuid>, i32)> {
    let row = sqlx::query_as("SELECT state::text, valid_from, author_id, row_version FROM app.policy_set_versions WHERE id = $1 AND policy_set_id = $2 FOR UPDATE")
        .bind(vid)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    row.ok_or_else(|| AppError::not_found().into())
}

/// Drafts only: valid_from, values and change_reason can change.
pub async fn version_patch(mut ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let req: NewVersion = parse(raw)?;
    let (state, from, _, v) = lock_version(&mut ctx.tx, id, vid).await?;
    if v != expected {
        return Err(stale(v));
    }
    if state != "draft" {
        return Err(AppError::conflict("not_draft", "Not a draft", format!("This version is {state}")).into());
    }
    let mut errs = Vec::new();
    let values = req.values.is_some().then(|| checked(req.values, &mut errs));
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let from = req.valid_from.unwrap_or(from);
    refuse_locked(&mut ctx.tx, None, from).await?;
    let old = version_doc(&mut ctx.tx, id, vid).await?;
    sqlx::query(
        "UPDATE app.policy_set_versions SET valid_from = $2, settings = coalesce($3, settings), change_reason = coalesce($4, change_reason),
           row_version = row_version + 1 WHERE id = $1",
    )
    .bind(vid)
    .bind(from)
    .bind(values.map(Value::Object))
    .bind(reason_of(req.change_reason))
    .execute(&mut *ctx.tx)
    .await?;
    let doc = version_doc(&mut ctx.tx, id, vid).await?;
    ctx.audit(MANAGE, "policy_version", vid, Some(old), Some(doc.clone()), None).await?;
    ctx.commit().await?;
    Ok(version_response(StatusCode::OK, doc))
}

/// Approved from its date: an earlier approved version for the same date is superseded; PolicyChanged.
async fn activate(ctx: &mut Ctx, id: Uuid, vid: Uuid, from: NaiveDate) -> ApiResult<()> {
    refuse_locked(&mut ctx.tx, None, from).await?;
    sqlx::query("UPDATE app.policy_set_versions SET state = 'superseded', superseded_at = now() WHERE policy_set_id = $1 AND valid_from = $2 AND state = 'approved'")
        .bind(id)
        .bind(from)
        .execute(&mut *ctx.tx)
        .await?;
    sqlx::query("UPDATE app.policy_set_versions SET state = 'approved', row_version = row_version + 1 WHERE id = $1").bind(vid).execute(&mut *ctx.tx).await?;
    ctx.event("PolicyChanged", "policy_set", id, json!({ "policy_set_id": id, "version_id": vid, "from": from })).await
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

/// submit / approve / reject / discard (§1.6). Repeating a step that is already done returns the version.
async fn step(mut ctx: Ctx, id: Uuid, vid: Uuid, headers: HeaderMap, raw: Value, action: &str) -> ApiResult<Response> {
    ctx.scope(if action == "approve" || action == "reject" { APPROVE } else { MANAGE })?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let (state, from, author, v) = lock_version(&mut ctx.tx, id, vid).await?;
    let scheduled = state == "approved" && from > today();
    let done = match action {
        "submit" => state == "pending_approval" || state == "approved",
        "approve" => state == "approved",
        "reject" => state == "rejected",
        _ => state == "discarded",
    };
    if !done {
        let ok = match action {
            "submit" => state == "draft",
            "approve" | "reject" => state == "pending_approval",
            _ => state == "draft" || state == "pending_approval" || scheduled,
        };
        if !ok {
            return Err(AppError::conflict("not_pending", "Can't do that now", format!("This version is {state}")).into());
        }
        if if_match(&headers)? != v {
            return Err(stale(v));
        }
        if (action == "approve" || action == "reject") && author == Some(ctx.user_id) {
            return Err(AppError::new(403, "cannot_decide_own_request", "Not your own change", "Someone else must approve a change you made").into());
        }
        if action == "reject" && reason.is_none() {
            return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
        }
        match action {
            "submit" if needs_approval(&mut ctx.tx).await? => {
                sqlx::query("UPDATE app.policy_set_versions SET state = 'pending_approval', submitted_at = now(), row_version = row_version + 1 WHERE id = $1")
                    .bind(vid)
                    .execute(&mut *ctx.tx)
                    .await?;
            }
            "submit" => {
                sqlx::query("UPDATE app.policy_set_versions SET submitted_at = now() WHERE id = $1").bind(vid).execute(&mut *ctx.tx).await?;
                activate(&mut ctx, id, vid, from).await?;
            }
            "approve" => {
                sqlx::query("UPDATE app.policy_set_versions SET decided_by = $2, decided_at = now(), decision_reason = $3 WHERE id = $1")
                    .bind(vid)
                    .bind(ctx.user_id)
                    .bind(&reason)
                    .execute(&mut *ctx.tx)
                    .await?;
                activate(&mut ctx, id, vid, from).await?;
            }
            "reject" => {
                sqlx::query("UPDATE app.policy_set_versions SET state = 'rejected', decided_by = $2, decided_at = now(), decision_reason = $3, row_version = row_version + 1 WHERE id = $1")
                    .bind(vid)
                    .bind(ctx.user_id)
                    .bind(&reason)
                    .execute(&mut *ctx.tx)
                    .await?;
            }
            _ => {
                sqlx::query("UPDATE app.policy_set_versions SET state = 'discarded', row_version = row_version + 1 WHERE id = $1").bind(vid).execute(&mut *ctx.tx).await?;
                if scheduled {
                    ctx.event("PolicyChanged", "policy_set", id, json!({ "policy_set_id": id, "version_id": vid, "from": from })).await?;
                }
            }
        }
        let doc = version_doc(&mut ctx.tx, id, vid).await?;
        let perm = if action == "approve" || action == "reject" { APPROVE } else { MANAGE };
        ctx.audit(perm, "policy_version", vid, Some(json!({ "state": state })), Some(json!({ "state": doc["state"] })), reason.as_deref()).await?;
    }
    let doc = version_doc(&mut ctx.tx, id, vid).await?;
    ctx.commit().await?;
    Ok(version_response(StatusCode::OK, doc))
}

pub async fn submit(ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    step(ctx, id, vid, headers, raw, "submit").await
}
pub async fn approve(ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    step(ctx, id, vid, headers, raw, "approve").await
}
pub async fn reject(ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    step(ctx, id, vid, headers, raw, "reject").await
}
pub async fn discard(ctx: Ctx, Path((id, vid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    step(ctx, id, vid, headers, raw, "discard").await
}

// ---------- PS5 schema ----------

pub async fn schema(ctx: Ctx) -> ApiResult<Json<Value>> {
    ctx.scope(MANAGE)?;
    Ok(Json(policy::schema()))
}

// ---------- PS6 assignments ----------

const ASSIGNMENT: &str = "jsonb_build_object('id', a.id, 'policy_set_id', a.policy_set_id, 'level', a.level, 'target_id', a.target_id,
    'valid_from', lower(a.valid), 'valid_to', upper(a.valid) - 1, 'state', a.state, 'recorded_at', a.recorded_at)";

pub async fn assignments(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    ctx.scope(MANAGE)?;
    let (mut set, mut level, mut target) = (None, None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "policy_set_id" => set = Some(v.parse::<Uuid>().map_err(|_| bad_param("policy_set_id", "Not an id"))?),
            "level" => level = Some(v),
            "target_id" => target = Some(v.parse::<Uuid>().map_err(|_| bad_param("target_id", "Not an id"))?),
            _ => return Err(unknown(&k)),
        }
    }
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({ASSIGNMENT} ORDER BY a.level, lower(a.valid)), '[]') FROM app.policy_assignments a
         WHERE a.state = 'active' AND ($1::uuid IS NULL OR a.policy_set_id = $1) AND ($2::text IS NULL OR a.level::text = $2)
           AND ($3::uuid IS NULL OR a.target_id = $3)"
    ))
    .bind(set)
    .bind(level)
    .bind(target)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAssignment {
    policy_set_id: Option<Uuid>,
    level: Option<String>,
    target_id: Option<Uuid>,
    valid_from: Option<NaiveDate>,
    reason: Option<String>,
}

pub async fn assign(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/policy-assignments", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewAssignment = parse(raw)?;
    let mut errs = Vec::new();
    let table = LEVELS.iter().find(|(l, _)| Some(*l) == req.level.as_deref()).map(|(_, t)| *t);
    if table.is_none() {
        errs.push(FieldError::new("level", "invalid_value", "branch, department, category or employee_type"));
    }
    for (k, v) in [("policy_set_id", req.policy_set_id.is_none()), ("target_id", req.target_id.is_none()), ("valid_from", req.valid_from.is_none())] {
        if v {
            errs.push(FieldError::new(k, "required", "This is required"));
        }
    }
    let reason = reason_of(req.reason);
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    check_item(&mut ctx.tx, "policy_sets", req.policy_set_id, "policy_set_id", &mut errs).await?;
    if let Some(t) = table {
        check_item(&mut ctx.tx, t, req.target_id, "target_id", &mut errs).await?;
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (level, from) = (req.level.unwrap(), req.valid_from.unwrap());
    refuse_locked(&mut ctx.tx, (level == "branch").then_some(req.target_id.unwrap()), from).await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.policy_assignments (org_id, level, target_id, policy_set_id, valid) VALUES ($1, $2::app.assignment_level, $3, $4, daterange($5, NULL))
         RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(&level)
    .bind(req.target_id)
    .bind(req.policy_set_id)
    .bind(from)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let doc: Value = sqlx::query_scalar(&format!("SELECT {ASSIGNMENT} FROM app.policy_assignments a WHERE a.id = $1")).bind(id).fetch_one(&mut *ctx.tx).await?;
    ctx.audit(MANAGE, "policy_assignment", id, None, Some(doc.clone()), reason.as_deref()).await?;
    ctx.event("PolicyChanged", "policy_assignment", id, json!({ "level": level, "target_id": req.target_id, "from": from })).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct End {
    valid_to: Option<NaiveDate>,
    reason: Option<String>,
}

/// Ends an assignment on `valid_to` (inclusive). The row is replaced by a shortened one, never edited.
pub async fn assignment_end(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let req: End = parse(raw)?;
    let reason = reason_of(req.reason);
    let mut errs = Vec::new();
    if req.valid_to.is_none() {
        errs.push(FieldError::new("valid_to", "required", "Choose the last day"));
    }
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let to = req.valid_to.unwrap();
    let row: Option<(String, Uuid, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT level::text, target_id, lower(valid), upper(valid) FROM app.policy_assignments WHERE id = $1 AND state = 'active' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some((level, target, from, upper)) = row else { return Err(AppError::not_found().into()) };
    let end = to + Days::new(1);
    if to < from || upper.is_some_and(|u| end >= u) {
        return Err(AppError::validation(vec![FieldError::new("valid_to", "invalid_value", "Choose a day within the assignment")]).into());
    }
    refuse_locked(&mut ctx.tx, (level == "branch").then_some(target), end).await?;
    sqlx::query("UPDATE app.policy_assignments SET state = 'superseded', superseded_at = now() WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
    let nid: Uuid = sqlx::query_scalar(
        "INSERT INTO app.policy_assignments (org_id, level, target_id, policy_set_id, valid)
         SELECT org_id, level, target_id, policy_set_id, daterange(lower(valid), $2) FROM app.policy_assignments WHERE id = $1 RETURNING id",
    )
    .bind(id)
    .bind(end)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let doc: Value = sqlx::query_scalar(&format!("SELECT {ASSIGNMENT} FROM app.policy_assignments a WHERE a.id = $1")).bind(nid).fetch_one(&mut *ctx.tx).await?;
    ctx.audit(MANAGE, "policy_assignment", nid, Some(json!({ "id": id, "valid_to": upper.map(|u| u - Days::new(1)) })), Some(doc.clone()), reason.as_deref()).await?;
    ctx.event("PolicyChanged", "policy_assignment", nid, json!({ "level": level, "target_id": target, "from": end })).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

// ---------- PS7 / PS8 employee policy set ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmployeeSet {
    effective_from: Option<NaiveDate>,
    policy_set_id: Option<Uuid>,
    reason: Option<String>,
}

/// PS7: the employee override, a dated job change (as R7). null = no override.
pub async fn employee_set(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    crate::employees::require_edit(&mut ctx, id, MANAGE, &em).await?;
    let req: EmployeeSet = parse(raw)?;
    let reason = reason_of(req.reason);
    let mut errs = Vec::new();
    if req.effective_from.is_none() {
        errs.push(FieldError::new("effective_from", "required", "Choose the date the change starts"));
    }
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    check_item(&mut ctx.tx, "policy_sets", req.policy_set_id, "policy_set_id", &mut errs).await?;
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let d = req.effective_from.unwrap();
    let changes = json!({ "policy_set_id": req.policy_set_id }).as_object().unwrap().clone();
    let body = crate::shifts::dated_fields(&mut ctx, id, expected, d, &reason.unwrap(), changes, MANAGE, |_| Ok(())).await?;
    ctx.event("PolicyChanged", "employee", id, json!({ "employee_ids": [id], "from": d })).await?;
    ctx.commit().await?;
    Ok(([(header::ETAG, format!("\"{}\"", body["version"]))], Json(body)).into_response())
}

/// PS8: the resolved set and where it comes from.
pub async fn employee_resolved(mut ctx: Ctx, Path(id): Path<Uuid>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let mut as_of = today();
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "as_of" => as_of = v.parse().map_err(|_| bad_param("as_of", "Use YYYY-MM-DD"))?,
            _ => return Err(unknown(&k)),
        }
    }
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, VIEW, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    let r = resolve(&mut ctx.tx, id, as_of).await?;
    ctx.commit().await?;
    let mut body = resolved_json(r);
    if let Value::Object(m) = &mut body {
        m.insert("as_of".into(), json!(as_of));
    }
    Ok(Json(body))
}
