//! E6 dated job change and E7 cancel a scheduled change (API_CONTRACTS §5.2, SM-3, BR-EM-05).
//! A saved version is never edited: affected versions are marked superseded and new ones inserted.

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{check_item, em_settings, require_edit, today, Tx};
use crate::error::{parse, ApiResult, Body};
use axum::extract::Path;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use payroll_core::settings::{AllowedBranches, EmSettings};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// Dated job fields, in the shape stored per version (allowed branches sorted).
const JOB_FIELDS: [&str; 10] = [
    "master_branch_id", "allowed_branch_ids", "department_id", "designation_id", "category_id",
    "employee_type_id", "contractor_id", "reporting_manager_id", "payroll_group_id", "enrolment_id",
];
/// Always mandatory (EM-02); the rest may be made mandatory by the organisation.
const REQUIRED: [&str; 5] = ["master_branch_id", "department_id", "category_id", "employee_type_id", "payroll_group_id"];

#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    pub id: Uuid,
    pub from: NaiveDate,
    pub to: Option<NaiveDate>, // exclusive; None = open-ended
    pub content: Map<String, Value>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub from: NaiveDate,
    pub to: Option<NaiveDate>,
    pub content: Map<String, Value>,
    pub reason: Option<String>,
}

/// Replace version `old` by `new` segments covering the same dates.
#[derive(Debug, PartialEq)]
pub struct Replacement {
    pub old: Uuid,
    pub new: Vec<Segment>,
}

#[derive(Debug, PartialEq)]
pub enum PlanError {
    NoVersionOnDate,
    NoChange,
    NotCancellable,
}

/// Later versions take the new value of a field only where they still held the value it replaced,
/// so a field a scheduled version set itself is kept.
fn carry_forward(later: &[Version], mut prev_old: Map<String, Value>, mut prev_new: Map<String, Value>) -> Vec<Replacement> {
    let mut out = Vec::new();
    for w in later {
        let mut c = w.content.clone();
        for (k, v) in &w.content {
            if prev_old.get(k) == Some(v) {
                c.insert(k.clone(), prev_new.get(k).cloned().unwrap_or(Value::Null));
            }
        }
        if c != w.content {
            out.push(Replacement { old: w.id, new: vec![Segment { from: w.from, to: w.to, content: c.clone(), reason: w.reason.clone() }] });
        }
        prev_old = w.content.clone();
        prev_new = c;
    }
    out
}

/// E6: apply `changes` from date `d`. `versions` are one period's active versions, in date order.
pub fn plan_change(versions: &[Version], d: NaiveDate, changes: &Map<String, Value>, reason: &str) -> Result<Vec<Replacement>, PlanError> {
    let i = versions
        .iter()
        .position(|v| v.from <= d && v.to.is_none_or(|t| d < t))
        .ok_or(PlanError::NoVersionOnDate)?;
    let base = &versions[i];
    let mut new_base = base.content.clone();
    new_base.extend(changes.clone());
    if new_base == base.content {
        return Err(PlanError::NoChange);
    }
    let mut segs = Vec::new();
    if base.from < d {
        segs.push(Segment { from: base.from, to: Some(d), content: base.content.clone(), reason: base.reason.clone() });
    }
    segs.push(Segment { from: d, to: base.to, content: new_base.clone(), reason: Some(reason.into()) });
    let mut out = vec![Replacement { old: base.id, new: segs }];
    out.extend(carry_forward(&versions[i + 1..], base.content.clone(), new_base));
    Ok(out)
}

/// E7: the version before the cancelled one is extended over its dates, and later versions
/// fall back to the values the cancelled version had replaced.
pub fn plan_cancel(versions: &[Version], target: Uuid) -> Result<Vec<Replacement>, PlanError> {
    let i = versions.iter().position(|v| v.id == target).ok_or(PlanError::NotCancellable)?;
    let (v, prev) = (&versions[i], versions.get(i.wrapping_sub(1)).filter(|_| i > 0).ok_or(PlanError::NotCancellable)?);
    if prev.to != Some(v.from) {
        return Err(PlanError::NotCancellable);
    }
    let mut out = vec![Replacement {
        old: prev.id,
        new: vec![Segment { from: prev.from, to: v.to, content: prev.content.clone(), reason: prev.reason.clone() }],
    }];
    out.extend(carry_forward(&versions[i + 1..], v.content.clone(), prev.content.clone()));
    Ok(out)
}

// ---------- database ----------

pub(crate) async fn load_versions(tx: &mut Tx, employee: Uuid, period: Uuid) -> ApiResult<Vec<Version>> {
    let rows = sqlx::query(
        "SELECT v.id, lower(v.valid) AS valid_from, upper(v.valid) AS valid_to, v.change_reason,
                jsonb_build_object(
                  'master_branch_id', v.master_branch_id, 'department_id', v.department_id,
                  'designation_id', v.designation_id, 'category_id', v.category_id,
                  'employee_type_id', v.employee_type_id, 'contractor_id', v.contractor_id,
                  'reporting_manager_id', v.reporting_manager_id, 'payroll_group_id', v.payroll_group_id,
                  'enrolment_id', v.enrolment_id,
                  'allowed_branch_ids', coalesce((SELECT jsonb_agg(ab.branch_id ORDER BY ab.branch_id::text)
                      FROM app.job_version_allowed_branches ab WHERE ab.job_version_id = v.id), '[]'::jsonb),
                  -- Package 2 fields: not editable through E6, but carried into every new version.
                  'default_shift_id', v.default_shift_id, 'policy_set_id', v.policy_set_id,
                  'allowed_shift_ids', coalesce((SELECT jsonb_agg(sh.shift_id ORDER BY sh.shift_id::text)
                      FROM app.job_version_allowed_shifts sh WHERE sh.job_version_id = v.id), '[]'::jsonb)) AS content
         FROM app.employee_job_versions v
         WHERE v.employee_id = $1 AND v.employment_period_id = $2 AND v.state = 'active'
         ORDER BY lower(v.valid)",
    )
    .bind(employee)
    .bind(period)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Version {
            id: r.get("id"),
            from: r.get("valid_from"),
            to: r.get("valid_to"),
            content: r.get::<Value, _>("content").as_object().cloned().unwrap_or_default(),
            reason: r.get("change_reason"),
        })
        .collect())
}

/// Supersedes the old versions and inserts their replacements. Returns the new versions.
pub(crate) async fn apply(ctx: &mut Ctx, employee: Uuid, period: Uuid, reps: &[Replacement]) -> ApiResult<Vec<(Uuid, NaiveDate, Option<NaiveDate>)>> {
    // All old rows leave the "active" set first, so the no-overlap constraint never sees both.
    for r in reps {
        sqlx::query("UPDATE app.employee_job_versions SET state = 'superseded', superseded_at = now() WHERE id = $1")
            .bind(r.old)
            .execute(&mut *ctx.tx)
            .await?;
    }
    let mut created = Vec::new();
    for r in reps {
        let mut first = None;
        for s in &r.new {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO app.employee_job_versions (org_id, employee_id, employment_period_id, valid,
                     master_branch_id, department_id, designation_id, category_id, employee_type_id, contractor_id,
                     reporting_manager_id, payroll_group_id, enrolment_id, change_reason, default_shift_id, policy_set_id)
                 SELECT $1, $2, $3, daterange($4, $5),
                     (c->>'master_branch_id')::uuid, (c->>'department_id')::uuid, (c->>'designation_id')::uuid,
                     (c->>'category_id')::uuid, (c->>'employee_type_id')::uuid, (c->>'contractor_id')::uuid,
                     (c->>'reporting_manager_id')::uuid, (c->>'payroll_group_id')::uuid, c->>'enrolment_id', $7,
                     (c->>'default_shift_id')::uuid, (c->>'policy_set_id')::uuid
                 FROM (SELECT $6::jsonb AS c) x RETURNING id",
            )
            .bind(ctx.org_id)
            .bind(employee)
            .bind(period)
            .bind(s.from)
            .bind(s.to)
            .bind(Value::Object(s.content.clone()))
            .bind(&s.reason)
            .fetch_one(&mut *ctx.tx)
            .await?;
            sqlx::query(
                "INSERT INTO app.job_version_allowed_branches (org_id, job_version_id, branch_id)
                 SELECT $1, $2, b::uuid FROM jsonb_array_elements_text($3) AS b",
            )
            .bind(ctx.org_id)
            .bind(id)
            .bind(s.content.get("allowed_branch_ids").cloned().unwrap_or(json!([])))
            .execute(&mut *ctx.tx)
            .await?;
            sqlx::query(
                "INSERT INTO app.job_version_allowed_shifts (org_id, job_version_id, shift_id)
                 SELECT $1, $2, s::uuid FROM jsonb_array_elements_text($3) AS s",
            )
            .bind(ctx.org_id)
            .bind(id)
            .bind(s.content.get("allowed_shift_ids").cloned().unwrap_or(json!([])))
            .execute(&mut *ctx.tx)
            .await?;
            first.get_or_insert(id);
            created.push((id, s.from, s.to));
        }
        sqlx::query("UPDATE app.employee_job_versions SET superseded_by_id = $2 WHERE id = $1")
            .bind(r.old)
            .bind(first)
            .execute(&mut *ctx.tx)
            .await?;
    }
    Ok(created)
}

/// Locks the employee, checks the version if one is given, and finds the open employment period.
pub(crate) async fn lock_employee(tx: &mut Tx, id: Uuid, expected: Option<i32>) -> ApiResult<(Uuid, NaiveDate)> {
    let version: i32 = sqlx::query_scalar("SELECT row_version FROM app.employees WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    if expected.is_some_and(|e| e != version) {
        return Err(stale(version));
    }
    let row = sqlx::query("SELECT id, joining_date FROM app.employment_periods WHERE employee_id = $1 AND state = 'open'")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::conflict("employee_not_current", "Employee has left", "Job changes need a current employee"))?;
    Ok((row.get("id"), row.get("joining_date")))
}

pub(crate) async fn bump_version(tx: &mut Tx, id: Uuid) -> ApiResult<i32> {
    Ok(sqlx::query_scalar("UPDATE app.employees SET row_version = row_version + 1 WHERE id = $1 RETURNING row_version")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?)
}

pub(crate) fn version_json(created: &[(Uuid, NaiveDate, Option<NaiveDate>)], today: NaiveDate) -> Vec<Value> {
    created
        .iter()
        .map(|(id, from, to)| {
            json!({
                "version_id": id, "valid_from": from, "valid_to": to.and_then(|t| t.pred_opt()),
                "state": if *from > today { "scheduled" } else { "effective" },
            })
        })
        .collect()
}

/// Shape and presence checks on `changes`; returns the normalised map (ids canonical, branches sorted).
fn normalise(raw: &Map<String, Value>, em: &EmSettings, errs: &mut Vec<FieldError>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in raw {
        let field = format!("changes.{k}");
        if !JOB_FIELDS.contains(&k.as_str()) {
            errs.push(FieldError::new(field, "unknown_field", "Not a job field"));
            continue;
        }
        let norm = match (k.as_str(), v) {
            ("allowed_branch_ids", Value::Array(a)) => {
                let ids: Option<Vec<Uuid>> = a.iter().map(|x| x.as_str().and_then(|s| s.parse().ok())).collect();
                ids.map(|mut ids| {
                    ids.sort_by_key(|i| i.to_string());
                    ids.dedup();
                    json!(ids)
                })
            }
            ("enrolment_id", Value::String(s)) if !s.trim().is_empty() => Some(json!(s.trim())),
            ("enrolment_id", Value::String(_)) => None,
            (_, Value::String(s)) if k != "allowed_branch_ids" => s.parse::<Uuid>().ok().map(|u| json!(u)),
            (_, Value::Null) if k != "allowed_branch_ids" => Some(Value::Null),
            _ => None,
        };
        let Some(norm) = norm else {
            errs.push(FieldError::new(field, "invalid_value", "Not a valid value"));
            continue;
        };
        if norm.is_null() && (REQUIRED.contains(&k.as_str()) || em.is_mandatory(k)) {
            errs.push(FieldError::new(field, "required", format!("{k} is required")));
            continue;
        }
        out.insert(k.clone(), norm);
    }
    out
}

fn uuid_of(m: &Map<String, Value>, k: &str) -> Option<Uuid> {
    m.get(k).and_then(Value::as_str).and_then(|s| s.parse().ok())
}

// ---------- E6 POST /employees/{id}/job-changes ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobChange {
    effective_from: Option<NaiveDate>,
    #[serde(default)]
    changes: Map<String, Value>,
    reason: Option<String>,
}

pub async fn change(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let today = today();
    let done = change_in(&mut ctx, id, raw, Some(expected)).await?;
    ctx.commit().await?;
    let prompts: Vec<&str> = if done.master_changed {
        vec!["review_payroll_group", "review_shift"] // AC-EM-10.4
    } else {
        vec![]
    };
    let version = done.version;
    let body = json!({ "versions": version_json(&done.created, today), "scheduled": done.effective_from > today, "prompts": prompts, "version": version });
    Ok(([(header::ETAG, format!("\"{version}\""))], Json(body)).into_response())
}

pub(crate) struct ChangeDone {
    created: Vec<(Uuid, NaiveDate, Option<NaiveDate>)>,
    version: i32,
    effective_from: NaiveDate,
    master_changed: bool,
}

/// E8 without commit; `expected` None (bulk import) skips the version check but still locks the employee.
pub(crate) async fn change_in(ctx: &mut Ctx, id: Uuid, raw: Value, expected: Option<i32>) -> ApiResult<ChangeDone> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(ctx, id, "employee.edit", &em).await?;
    let req: JobChange = parse(raw)?;

    let mut errs = Vec::new();
    if req.effective_from.is_none() {
        errs.push(FieldError::new("effective_from", "required", "Choose the date the change starts"));
    }
    if req.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    let changes = normalise(&req.changes, &em, &mut errs);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    if changes.is_empty() {
        return Err(AppError::unprocessable("no_change", "Nothing to change", "Send at least one job field").into());
    }
    let d = req.effective_from.unwrap();
    let reason = req.reason.unwrap().trim().to_string();

    let (period, joining) = lock_employee(&mut ctx.tx, id, expected).await?;
    if d < joining {
        return Err(AppError::unprocessable("date_before_joining", "Date before joining", format!("The employee joined on {joining}")).into());
    }
    crate::periods::refuse_if_locked(&mut ctx.tx, id, d).await?;
    let versions = load_versions(&mut ctx.tx, id, period).await?;
    let reps = match plan_change(&versions, d, &changes, &reason) {
        Ok(r) => r,
        Err(PlanError::NoChange) => {
            return Err(AppError::unprocessable("no_change", "Nothing to change", "These are already the values on that date").into())
        }
        Err(_) => return Err(AppError::internal().into()),
    };
    let new_base = &reps[0].new.last().unwrap().content;

    // Field checks on the values the change brings in (AC-OR-01.4, BR-EM-03, BR-EM-11).
    let tx = &mut ctx.tx;
    let lists = [
        ("master_branch_id", "branches"), ("department_id", "departments"), ("designation_id", "designations"),
        ("category_id", "categories"), ("employee_type_id", "employee_types"), ("contractor_id", "contractors"),
        ("payroll_group_id", "payroll_groups"),
    ];
    for (k, table) in lists {
        if changes.contains_key(k) {
            check_item(tx, table, uuid_of(&changes, k), &format!("changes.{k}"), &mut errs).await?;
        }
    }
    if let Some(Value::Array(ids)) = changes.get("allowed_branch_ids") {
        if !ids.is_empty() && em.allowed_branches != AllowedBranches::Chosen {
            errs.push(FieldError::new("changes.allowed_branch_ids", "invalid_value", "This organisation does not use chosen allowed branches"));
        }
        for b in ids {
            check_item(tx, "branches", b.as_str().and_then(|s| s.parse().ok()), "changes.allowed_branch_ids", &mut errs).await?;
        }
    }
    for seg in reps.iter().flat_map(|r| &r.new) {
        let master = seg.content.get("master_branch_id");
        if seg.content.get("allowed_branch_ids").and_then(Value::as_array).is_some_and(|a| master.is_some_and(|m| a.contains(m))) {
            errs.push(FieldError::new("changes.allowed_branch_ids", "invalid_value", "The master branch is not also an allowed branch"));
            break;
        }
    }
    if let Some(m) = changes.contains_key("reporting_manager_id").then(|| uuid_of(&changes, "reporting_manager_id")).flatten() {
        let current: Option<bool> = sqlx::query_scalar("SELECT is_current FROM app.employees WHERE id = $1")
            .bind(m)
            .fetch_optional(&mut **tx)
            .await?;
        if current != Some(true) {
            errs.push(FieldError::new("changes.reporting_manager_id", "invalid_value", "The manager must be a current employee"));
        } else {
            let pairs = sqlx::query(
                "SELECT employee_id, reporting_manager_id FROM app.employee_job_versions
                 WHERE state = 'active' AND valid @> $1::date AND reporting_manager_id IS NOT NULL AND employee_id <> $2",
            )
            .bind(d)
            .bind(id)
            .fetch_all(&mut **tx)
            .await?;
            let chain: HashMap<Uuid, Uuid> = pairs.iter().map(|r| (r.get("employee_id"), r.get("reporting_manager_id"))).collect();
            if payroll_core::reporting::would_loop(id, m, |x| chain.get(&x).copied()) {
                return Err(AppError::unprocessable("manager_loop", "Reporting loop", "This manager already reports to this employee").into());
            }
        }
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }

    let created = apply(ctx, id, period, &reps).await?;
    let version = bump_version(&mut ctx.tx, id).await?;
    let old: Map<String, Value> = changes.keys().map(|k| (k.clone(), versions.iter().find(|v| v.id == reps[0].old).unwrap().content[k].clone())).collect();
    ctx.audit("employee.edit", "employee", id, Some(json!({ "effective_from": d, "job": old })), Some(json!({ "effective_from": d, "job": changes })), Some(&reason)).await?;
    ctx.event("JobChanged", "employee", id, json!({
        "employee_id": id, "effective_from": d, "changed_fields": changes.keys().collect::<Vec<_>>()
    }))
    .await?;
    let master_changed = changes.contains_key("master_branch_id") && uuid_of(new_base, "master_branch_id").is_some();
    Ok(ChangeDone { created, version, effective_from: d, master_changed })
}

// ---------- E7 POST /employees/{id}/job-changes/{version_id}/cancel ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    reason: Option<String>,
}

pub async fn cancel(mut ctx: Ctx, Path((id, version_id)): Path<(Uuid, Uuid)>, Body(raw): Body) -> ApiResult<Response> {
    let today = today();
    let em = em_settings(&mut ctx.tx, today).await?;
    require_edit(&mut ctx, id, "employee.edit", &em).await?;
    let req: Cancel = parse(raw)?;
    let reason = req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string).ok_or_else(|| {
        AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")])
    })?;

    let (period, _) = lock_employee(&mut ctx.tx, id, None).await?;
    let row = sqlx::query("SELECT state::text AS state, lower(valid) AS valid_from FROM app.employee_job_versions WHERE id = $1 AND employee_id = $2")
        .bind(version_id)
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let not_scheduled = || AppError::conflict("not_scheduled", "Not a scheduled change", "Only a change that has not started yet can be cancelled");
    match row.get::<String, _>("state").as_str() {
        "cancelled" => {
            // Already done: same answer again (STATE_MACHINES "Common to all machines").
            ctx.commit().await?;
            return Ok(Json(json!({ "version_id": version_id, "state": "cancelled" })).into_response());
        }
        "active" if row.get::<NaiveDate, _>("valid_from") > today => {}
        _ => return Err(not_scheduled().into()),
    }

    let versions = load_versions(&mut ctx.tx, id, period).await?;
    let reps = plan_cancel(&versions, version_id).map_err(|_| not_scheduled())?;
    sqlx::query("UPDATE app.employee_job_versions SET state = 'cancelled', cancelled_reason = $2 WHERE id = $1")
        .bind(version_id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
    apply(&mut ctx, id, period, &reps).await?;
    let version = bump_version(&mut ctx.tx, id).await?;
    let from: NaiveDate = row.get("valid_from");
    ctx.audit("employee.edit", "job_version", version_id, None, Some(json!({ "state": "cancelled", "employee_id": id })), Some(&reason)).await?;
    ctx.event("JobChanged", "employee", id, json!({ "employee_id": id, "effective_from": from, "cancelled_version_id": version_id })).await?;
    ctx.commit().await?;
    Ok(([(header::ETAG, format!("\"{version}\""))], Json(json!({ "version_id": version_id, "state": "cancelled", "version": version }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }
    fn c(branch: &str, dept: &str) -> Map<String, Value> {
        json!({ "master_branch_id": branch, "department_id": dept }).as_object().unwrap().clone()
    }
    fn v(from: NaiveDate, to: Option<NaiveDate>, content: Map<String, Value>) -> Version {
        Version { id: Uuid::new_v4(), from, to, content, reason: None }
    }

    #[test]
    fn past_change_splits_the_version_in_force() {
        let a = v(d(1, 1), None, c("CHN", "OPS"));
        let reps = plan_change(std::slice::from_ref(&a), d(3, 1), &c("BLR", "OPS"), "transfer").unwrap();
        assert_eq!(reps.len(), 1);
        let segs = &reps[0].new;
        assert_eq!((segs[0].from, segs[0].to, &segs[0].content), (d(1, 1), Some(d(3, 1)), &a.content));
        assert_eq!((segs[1].from, segs[1].to, segs[1].content["master_branch_id"].as_str()), (d(3, 1), None, Some("BLR")));
    }

    #[test]
    fn change_on_start_date_replaces_without_split() {
        let a = v(d(1, 1), None, c("CHN", "OPS"));
        let reps = plan_change(&[a], d(1, 1), &c("BLR", "OPS"), "fix").unwrap();
        assert_eq!(reps[0].new.len(), 1);
    }

    #[test]
    fn later_version_keeps_its_own_fields_and_inherits_the_rest() {
        // Jan: CHN/OPS. Scheduled Jun: department → HR. Now a transfer to BLR from Mar.
        let a = v(d(1, 1), Some(d(6, 1)), c("CHN", "OPS"));
        let b = v(d(6, 1), None, c("CHN", "HR"));
        let reps = plan_change(&[a, b.clone()], d(3, 1), &json!({ "master_branch_id": "BLR" }).as_object().unwrap().clone(), "t").unwrap();
        assert_eq!(reps.len(), 2);
        assert_eq!(reps[1].old, b.id);
        assert_eq!(reps[1].new[0].content, c("BLR", "HR")); // branch carried, own department kept
    }

    #[test]
    fn same_values_are_no_change() {
        let a = v(d(1, 1), None, c("CHN", "OPS"));
        assert_eq!(plan_change(&[a], d(2, 1), &c("CHN", "OPS"), "x"), Err(PlanError::NoChange));
    }

    #[test]
    fn cancel_extends_previous_and_reverts_inherited_values() {
        // Jan CHN/OPS; Jun → BLR (to cancel); Sep → dept HR, still at BLR (inherited).
        let a = v(d(1, 1), Some(d(6, 1)), c("CHN", "OPS"));
        let b = v(d(6, 1), Some(d(9, 1)), c("BLR", "OPS"));
        let w = v(d(9, 1), None, c("BLR", "HR"));
        let reps = plan_cancel(&[a.clone(), b.clone(), w.clone()], b.id).unwrap();
        assert_eq!(reps[0].old, a.id);
        assert_eq!((reps[0].new[0].from, reps[0].new[0].to), (d(1, 1), Some(d(9, 1))));
        assert_eq!(reps[1].new[0].content, c("CHN", "HR"));
    }

    #[test]
    fn first_version_cannot_be_cancelled() {
        let a = v(d(1, 1), None, c("CHN", "OPS"));
        assert_eq!(plan_cancel(std::slice::from_ref(&a), a.id), Err(PlanError::NotCancellable));
    }
}
