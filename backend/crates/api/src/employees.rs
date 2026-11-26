//! Employee routes E1, E2, E5, E8 (API_CONTRACTS §5.1–5.2).

use crate::ctx::{if_match, stale, Ctx, Scope};
use crate::error::{parse, ApiResult, Body};
use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use payroll_core::settings::{AllowedBranchAccess, AllowedBranches, CodeMode, EmSettings};
use payroll_core::status::EmployeeStatus;
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{Postgres, QueryBuilder, Row, Transaction};
use uuid::Uuid;

pub(crate) type Tx = Transaction<'static, Postgres>;

/// shortcut: "today" is India time for every organisation; use each master branch's time zone
/// once branches outside India go live (API_CONTRACTS §1.1).
pub fn today() -> NaiveDate {
    (chrono::Utc::now() + chrono::Duration::minutes(330)).date_naive()
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Access {
    View,
    Edit,
}

pub(crate) async fn em_settings(tx: &mut Tx, on: NaiveDate) -> ApiResult<EmSettings> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT setting_values FROM app.org_settings_versions
         WHERE settings_group = 'EM' AND valid_from <= $1 ORDER BY valid_from DESC, recorded_at DESC LIMIT 1",
    )
    .bind(on)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(v.map(|v| EmSettings::from_json(v).unwrap_or_else(|e| {
        tracing::error!(error = %e, "stored EM settings unreadable; using defaults");
        EmSettings::default()
    }))
    .unwrap_or_default())
}

/// `FROM` with the job version in force on `as_of` (or the nearest one: the last before an exit,
/// the first for a future joiner) joined as `jv`.
pub(crate) fn push_from(qb: &mut QueryBuilder<'static, Postgres>, as_of: NaiveDate) {
    qb.push(
        " FROM app.employees e LEFT JOIN LATERAL (SELECT v.* FROM app.employee_job_versions v
          WHERE v.employee_id = e.id AND v.state = 'active' ORDER BY lower(v.valid) <= ",
    )
    .push_bind(as_of)
    .push("::date DESC, abs(lower(v.valid) - ")
    .push_bind(as_of)
    .push("::date) LIMIT 1) jv ON true");
}

/// Scope rules employee-view / employee-edit (§1.3) as a SQL condition on `e` and `jv`.
pub(crate) fn push_scope(qb: &mut QueryBuilder<'static, Postgres>, s: &Scope, access: Access, me: Option<Uuid>, em10: AllowedBranchAccess, as_of: NaiveDate) {
    if s.org {
        qb.push("true");
        return;
    }
    qb.push("(false");
    if !s.branches.is_empty() {
        qb.push(" OR jv.master_branch_id = ANY(").push_bind(s.branches.clone()).push(")");
        let via_allowed = match access {
            Access::View => em10 != AllowedBranchAccess::NotVisible,
            Access::Edit => em10 == AllowedBranchAccess::ViewEdit,
        };
        if via_allowed {
            qb.push(" OR EXISTS (SELECT 1 FROM app.job_version_allowed_branches ab WHERE ab.job_version_id = jv.id AND ab.branch_id = ANY(")
                .push_bind(s.branches.clone())
                .push("))");
        }
    }
    if !s.departments.is_empty() {
        qb.push(" OR jv.department_id = ANY(").push_bind(s.departments.clone()).push(")");
    }
    if let (Access::View, Some(me)) = (access, me) {
        if s.chain {
            qb.push(
                " OR e.id IN (WITH RECURSIVE ch(id) AS (
                    SELECT v.employee_id FROM app.employee_job_versions v
                    WHERE v.state = 'active' AND v.valid @> ",
            )
            .push_bind(as_of)
            .push("::date AND v.reporting_manager_id = ")
            .push_bind(me)
            .push(
                " UNION SELECT v.employee_id FROM app.employee_job_versions v JOIN ch ON v.reporting_manager_id = ch.id
                    WHERE v.state = 'active' AND v.valid @> ",
            )
            .push_bind(as_of)
            .push("::date) SELECT id FROM ch)");
        }
        if s.own {
            qb.push(" OR e.id = ").push_bind(me);
        }
    }
    qb.push(")");
}

/// Is this employee visible (or editable) to the caller under `permission`? Unknown ids are simply false.
pub(crate) async fn allowed(ctx: &mut Ctx, id: Uuid, permission: &str, access: Access, em: &EmSettings) -> ApiResult<bool> {
    let Ok(scope) = ctx.scope(permission) else { return Ok(false) };
    // Self-edit (M2) reaches one record only: the caller's own.
    if permission == "employee.profile.edit_own" {
        return Ok(scope.own && ctx.employee_id == Some(id));
    }
    let as_of = today();
    let mut qb = QueryBuilder::new("SELECT EXISTS (SELECT 1");
    push_from(&mut qb, as_of);
    qb.push(" WHERE e.id = ").push_bind(id).push(" AND ");
    push_scope(&mut qb, &scope, access, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    qb.push(")");
    Ok(qb.build_query_scalar::<bool>().fetch_one(&mut *ctx.tx).await?)
}

/// 404 if the caller can't see the employee; 403 if they can see but not change it (§1.3).
pub(crate) async fn require_edit(ctx: &mut Ctx, id: Uuid, permission: &str, em: &EmSettings) -> ApiResult<()> {
    if !allowed(ctx, id, "employee.view", Access::View, em).await? {
        return Err(AppError::not_found().into());
    }
    if !allowed(ctx, id, permission, Access::Edit, em).await? {
        return Err(AppError::forbidden().into());
    }
    Ok(())
}

/// The Employee shape of §2.1 on `as_of`.
/// shortcut: list names are today's names, not "as named on as_of" (AC-OR-02.3); photo_url, sign_in,
/// custom_fields and incomplete_fields come with their own routes (policy_set is added by E2).
pub(crate) async fn load_employee(tx: &mut Tx, id: Uuid, as_of: NaiveDate) -> ApiResult<Option<Value>> {
    let mut qb = QueryBuilder::new("SELECT jsonb_build_object(
      'id', e.id, 'employee_code', e.employee_code, 'version', e.row_version, 'as_of', ");
    qb.push_bind(as_of).push("::date,
      'personal', jsonb_build_object('full_name', e.full_name, 'date_of_birth', e.date_of_birth, 'gender', e.gender,
          'mobile', e.mobile, 'email', e.email, 'address', e.address,
          'emergency_contact_name', e.emergency_contact_name, 'emergency_contact_phone', e.emergency_contact_phone),
      'employment', (SELECT jsonb_build_object('period_id', p.id, 'joining_date', p.joining_date,
            'probation_end_date', p.probation_end_date, 'last_working_day', p.last_working_day,
            'exit_reason', (SELECT jsonb_build_object('id', x.id, 'code', x.code, 'name', x.name)
                            FROM app.exit_reasons x WHERE x.id = p.exit_reason_id),
            'exit_remarks', p.exit_remarks)
          FROM app.employment_periods p WHERE p.employee_id = e.id
          ORDER BY p.joining_date <= ").push_bind(as_of).push("::date DESC, abs(p.joining_date - ").push_bind(as_of).push("::date) LIMIT 1),
      'status', (SELECT jsonb_build_object('value', s.status, 'since', s.effective_from) FROM app.employee_status_changes s
          WHERE s.employee_id = e.id AND s.superseded_at IS NULL AND s.effective_from <= ").push_bind(as_of).push("::date
          ORDER BY s.effective_from DESC LIMIT 1),
      'pending_status_request', (SELECT jsonb_build_object('id', r.id, 'to_status', r.to_status, 'effective_date', r.effective_date)
          FROM app.status_change_requests r WHERE r.employee_id = e.id AND r.state = 'pending'),
      'job', CASE WHEN jv.id IS NULL THEN NULL ELSE jsonb_build_object(
          'version_id', jv.id, 'valid_from', lower(jv.valid), 'valid_to', upper(jv.valid) - 1,
          'master_branch', (SELECT jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name) FROM app.branches b WHERE b.id = jv.master_branch_id),
          'allowed_branches', coalesce((SELECT jsonb_agg(jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name) ORDER BY b.short_code)
              FROM app.job_version_allowed_branches ab JOIN app.branches b ON b.id = ab.branch_id WHERE ab.job_version_id = jv.id), '[]'::jsonb),
          'department', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.departments d WHERE d.id = jv.department_id),
          'designation', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.designations d WHERE d.id = jv.designation_id),
          'category', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.categories d WHERE d.id = jv.category_id),
          'employee_type', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.employee_types d WHERE d.id = jv.employee_type_id),
          'contractor', (SELECT jsonb_build_object('id', c.id, 'code', c.code, 'name', c.name) FROM app.contractors c WHERE c.id = jv.contractor_id),
          'reporting_manager', (SELECT jsonb_build_object('id', m.id, 'employee_code', m.employee_code, 'full_name', m.full_name) FROM app.employees m WHERE m.id = jv.reporting_manager_id),
          'payroll_group', (SELECT jsonb_build_object('id', g.id, 'name', g.name) FROM app.payroll_groups g WHERE g.id = jv.payroll_group_id),
          'enrolment_id', jv.enrolment_id) END,
      'enrolment_status', e.enrolment_status, 'portal_access', e.portal_access,
      'custom_fields', coalesce((SELECT jsonb_object_agg(v.field_id, coalesce(to_jsonb(v.value_text), to_jsonb(v.value_number),
              to_jsonb(v.value_date), to_jsonb(v.value_bool), to_jsonb(v.value_options)))
          FROM app.custom_field_values v JOIN app.custom_field_definitions d ON d.id = v.field_id AND d.state = 'active'
          WHERE v.employee_id = e.id), '{}'::jsonb),
      'scheduled_changes_count', (SELECT count(*) FROM app.employee_job_versions v
          WHERE v.employee_id = e.id AND v.state = 'active' AND lower(v.valid) > ").push_bind(as_of).push("::date)
    )");
    push_from(&mut qb, as_of);
    qb.push(" WHERE e.id = ").push_bind(id);
    Ok(qb.build_query_scalar::<Value>().fetch_optional(&mut **tx).await?)
}

pub(crate) fn employee_response(status: StatusCode, doc: Value) -> Response {
    let etag = format!("\"{}\"", doc["version"]);
    (status, [(header::ETAG, etag)], Json(doc)).into_response()
}

pub(crate) fn query_pairs(raw: Option<String>) -> ApiResult<Vec<(String, String)>> {
    serde_urlencoded::from_str(raw.as_deref().unwrap_or_default())
        .map_err(|e| AppError::unprocessable("invalid_value", "Query not valid", e.to_string()).into())
}

pub(crate) fn bad_param(name: &str, why: &str) -> shared::AppError {
    AppError::validation(vec![FieldError::new(name, "invalid_value", why)])
}

/// Adds `can` (§2.1) for callers who may edit at all.
pub(crate) async fn add_can(ctx: &mut Ctx, id: Uuid, doc: &mut Value, em: &EmSettings) -> ApiResult<()> {
    if !ctx.has("employee.edit") {
        return Ok(());
    }
    doc["can"] = json!({
        "edit": allowed(ctx, id, "employee.edit", Access::Edit, em).await?,
        "raise_status": allowed(ctx, id, "employee.status.raise", Access::Edit, em).await?,
        "issue_pin": allowed(ctx, id, "employee.pin.issue", Access::Edit, em).await?,
    });
    Ok(())
}

// ---------- E1 GET /employees ----------

const JOB_FILTERS: [(&str, &str); 8] = [
    ("branch_id", "jv.master_branch_id"),
    ("department_id", "jv.department_id"),
    ("designation_id", "jv.designation_id"),
    ("category_id", "jv.category_id"),
    ("employee_type_id", "jv.employee_type_id"),
    ("payroll_group_id", "jv.payroll_group_id"),
    ("contractor_id", "jv.contractor_id"),
    ("reporting_manager_id", "jv.reporting_manager_id"),
];

/// Query parameters E1's filters understand (R10 and the employee export take the same ones).
pub(crate) const FILTER_PARAMS: [&str; 3] = ["status", "q", "as_of"];

/// Refuses parameters that are neither E1 filters nor in `extra`.
pub(crate) fn check_params(pairs: &[(String, String)], extra: &[&str]) -> ApiResult<()> {
    for (k, _) in pairs {
        if !(JOB_FILTERS.iter().any(|(n, _)| n == k) || FILTER_PARAMS.contains(&k.as_str()) || extra.contains(&k.as_str())) {
            return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into());
        }
    }
    Ok(())
}

/// `as_of` from the parameters, default today.
pub(crate) fn as_of_param(pairs: &[(String, String)]) -> ApiResult<NaiveDate> {
    match pairs.iter().find(|(n, _)| n == "as_of") {
        Some((_, d)) => Ok(d.parse().map_err(|_| bad_param("as_of", "Use YYYY-MM-DD"))?),
        None => Ok(today()),
    }
}

/// FROM … WHERE for the employees the caller may view on `as_of`, narrowed by E1's filters.
/// Joins `e` (employee), `jv` (job version in force) and `st` (status on `as_of`).
pub(crate) fn push_filtered(qb: &mut QueryBuilder<'static, Postgres>, ctx: &Ctx, scope: &Scope, em: &EmSettings, as_of: NaiveDate, pairs: &[(String, String)]) -> ApiResult<()> {
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let all = |k: &str| pairs.iter().filter(|(n, _)| n == k).map(|(_, v)| v.clone()).collect::<Vec<_>>();
    push_from(qb, as_of);
    qb.push(
        " LEFT JOIN LATERAL (SELECT s.status::text AS status FROM app.employee_status_changes s
           WHERE s.employee_id = e.id AND s.superseded_at IS NULL AND s.effective_from <= ",
    )
    .push_bind(as_of)
    .push("::date ORDER BY s.effective_from DESC LIMIT 1) st ON true WHERE ");
    push_scope(qb, scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);

    for (name, col) in JOB_FILTERS {
        let values = all(name);
        if values.is_empty() {
            continue;
        }
        let ids = values.iter().map(|v| v.parse::<Uuid>()).collect::<Result<Vec<_>, _>>().map_err(|_| bad_param(name, "Not a valid id"))?;
        qb.push(format!(" AND {col} = ANY(")).push_bind(ids).push(")");
    }
    let statuses = all("status");
    if !statuses.is_empty() {
        qb.push(" AND st.status = ANY(").push_bind(statuses).push(")");
    }
    if let Some(q) = get("q").filter(|q| !q.trim().is_empty()) {
        let like = format!("%{}%", q.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        qb.push(" AND (e.full_name ILIKE ").push_bind(like.clone())
            .push(" OR e.employee_code ILIKE ").push_bind(like.clone())
            .push(" OR e.mobile ILIKE ").push_bind(like).push(")");
    }
    Ok(())
}

/// Headcount for the employee dashboard: totals by status, department and master branch.
/// Same scope and filters as the list (E1), so the numbers match what the caller can open.
pub async fn summary(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope("employee.view")?;
    let pairs = query_pairs(raw)?;
    check_params(&pairs, &[])?;
    let as_of = as_of_param(&pairs)?;
    let em = em_settings(&mut ctx.tx, as_of).await?;

    let mut qb = QueryBuilder::new("WITH f AS (SELECT st.status, jv.department_id, jv.master_branch_id");
    push_filtered(&mut qb, &ctx, &scope, &em, as_of, &pairs)?;
    qb.push(
        ") SELECT jsonb_build_object(
           'as_of', ",
    )
    .push_bind(as_of)
    .push(
        "::date,
           'total', (SELECT count(*) FROM f),
           'by_status', COALESCE((SELECT jsonb_object_agg(k, n) FROM (SELECT COALESCE(status, 'unknown') k, count(*) n FROM f GROUP BY 1) x), '{}'::jsonb),
           'by_department', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name, 'count', x.n) ORDER BY x.n DESC, d.name)
               FROM (SELECT department_id, count(*) n FROM f GROUP BY 1) x LEFT JOIN app.departments d ON d.id = x.department_id), '[]'::jsonb),
           'by_branch', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name, 'count', x.n) ORDER BY x.n DESC, b.name)
               FROM (SELECT master_branch_id, count(*) n FROM f GROUP BY 1) x LEFT JOIN app.branches b ON b.id = x.master_branch_id), '[]'::jsonb)
         ) AS summary",
    );
    let body: Value = qb.build().fetch_one(&mut *ctx.tx).await?.get("summary");
    ctx.commit().await?;
    Ok(Json(body))
}

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope("employee.view")?;
    let pairs = query_pairs(raw)?;
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    // shortcut: incomplete=true and include_total are not built yet; refused rather than ignored.
    check_params(&pairs, &["limit", "cursor", "sort"])?;
    let as_of = as_of_param(&pairs)?;
    let limit: i64 = match get("limit") {
        Some(l) => l.parse().ok().filter(|n| (1..=200).contains(n)).ok_or_else(|| bad_param("limit", "1 to 200"))?,
        None => 50,
    };
    if get("sort").is_some_and(|s| s != "employee_code") {
        return Err(bad_param("sort", "Only employee_code is supported so far").into());
    }
    let em = em_settings(&mut ctx.tx, as_of).await?;

    let mut qb = QueryBuilder::new(
        "SELECT e.employee_code, jsonb_build_object(
           'id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name, 'mobile', e.mobile,
           'status', st.status,
           'master_branch', (SELECT jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name) FROM app.branches b WHERE b.id = jv.master_branch_id),
           'department', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.departments d WHERE d.id = jv.department_id),
           'designation', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.designations d WHERE d.id = jv.designation_id),
           'payroll_group', (SELECT jsonb_build_object('id', g.id, 'name', g.name) FROM app.payroll_groups g WHERE g.id = jv.payroll_group_id),
           'portal_access', e.portal_access) AS item",
    );
    push_filtered(&mut qb, &ctx, &scope, &em, as_of, &pairs)?;
    if let Some(c) = get("cursor") {
        qb.push(" AND e.employee_code > ").push_bind(c);
    }
    qb.push(" ORDER BY e.employee_code LIMIT ").push_bind(limit + 1);

    let rows = qb.build().fetch_all(&mut *ctx.tx).await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<Value> = rows.iter().take(limit as usize).map(|r| r.get("item")).collect();
    let next_cursor = more.then(|| rows[limit as usize - 1].get::<String, _>("employee_code"));
    ctx.commit().await?;
    Ok(Json(json!({ "items": items, "next_cursor": next_cursor })))
}

// ---------- E2 GET /employees/{id} ----------

pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>, RawQuery(raw): RawQuery) -> ApiResult<Response> {
    let mut as_of = today();
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "as_of" => as_of = v.parse().map_err(|_| bad_param("as_of", "Use YYYY-MM-DD"))?,
            // shortcut: known_on (record as recorded at a past time) is not built yet.
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, "employee.view", Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    let mut doc = load_employee(&mut ctx.tx, id, as_of).await?.ok_or_else(AppError::not_found)?;
    add_can(&mut ctx, id, &mut doc, &em).await?;
    doc["policy_set"] = crate::policies::resolved_json(crate::policies::resolve(&mut ctx.tx, id, as_of).await?); // AC-AT-21.2
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, doc))
}

// ---------- E5 POST /employees ----------

#[derive(Deserialize, Default, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Personal {
    full_name: Option<String>,
    mobile: Option<String>,
    date_of_birth: Option<NaiveDate>,
    gender: Option<String>,
    email: Option<String>,
    address: Option<String>,
    emergency_contact_name: Option<String>,
    emergency_contact_phone: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobFields {
    pub(crate) master_branch_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) allowed_branch_ids: Vec<Uuid>,
    pub(crate) department_id: Option<Uuid>,
    pub(crate) designation_id: Option<Uuid>,
    pub(crate) category_id: Option<Uuid>,
    pub(crate) employee_type_id: Option<Uuid>,
    pub(crate) contractor_id: Option<Uuid>,
    pub(crate) reporting_manager_id: Option<Uuid>,
    pub(crate) payroll_group_id: Option<Uuid>,
    pub(crate) enrolment_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateEmployee {
    employee_code: Option<String>,
    #[serde(default)]
    personal: Personal,
    joining_date: Option<NaiveDate>,
    probation_end_date: Option<NaiveDate>,
    initial_status: Option<EmployeeStatus>,
    #[serde(default)]
    job: JobFields,
    // shortcut: only "none" until E13 (PIN issue) exists; custom_fields come with O19/E10.
    portal_access: Option<String>,
}

pub(crate) fn is_e164(s: &str) -> bool {
    let d = s.strip_prefix('+').unwrap_or("");
    (8..=15).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()) && !d.starts_with('0')
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|v| v.trim().is_empty())
}

/// Format and presence checks shared by create (all fields) and edit (only the fields sent).
fn check_personal(p: &Personal, present: impl Fn(&str) -> bool, em: &EmSettings, errs: &mut Vec<FieldError>) {
    let req = |errs: &mut Vec<FieldError>, f: &str, label: &str| errs.push(FieldError::new(format!("personal.{f}"), "required", format!("{label} is required")));
    if present("full_name") && blank(&p.full_name) {
        req(errs, "full_name", "Name");
    }
    if present("mobile") {
        match p.mobile.as_deref().map(str::trim) {
            None | Some("") => req(errs, "mobile", "Mobile number"),
            Some(m) if !is_e164(m) => errs.push(FieldError::new("personal.mobile", "invalid_value", "Use international format, e.g. +919876543210")),
            _ => {}
        }
    }
    if present("emergency_contact_phone") && p.emergency_contact_phone.as_deref().is_some_and(|m| !is_e164(m.trim())) {
        errs.push(FieldError::new("personal.emergency_contact_phone", "invalid_value", "Use international format, e.g. +919876543210"));
    }
    if present("email") && p.email.as_deref().is_some_and(|e| !e.contains('@') || e.trim().contains(' ')) {
        errs.push(FieldError::new("personal.email", "invalid_value", "Not a valid email address"));
    }
    let values = [
        ("date_of_birth", p.date_of_birth.is_none()),
        ("gender", blank(&p.gender)),
        ("email", blank(&p.email)),
        ("address", blank(&p.address)),
        ("emergency_contact_name", blank(&p.emergency_contact_name)),
        ("emergency_contact_phone", blank(&p.emergency_contact_phone)),
    ];
    for (f, missing) in values {
        if present(f) && missing && em.is_mandatory(f) {
            req(errs, f, f);
        }
    }
}

/// Unknown → "not found"; inactive → refused (AC-OR-01.4). `table` is one of a fixed set.
pub(crate) async fn check_item(tx: &mut Tx, table: &str, id: Option<Uuid>, field: &str, errs: &mut Vec<FieldError>) -> ApiResult<()> {
    let Some(id) = id else { return Ok(()) };
    let state: Option<String> = sqlx::query_scalar(&format!("SELECT state::text FROM app.{table} WHERE id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    match state.as_deref() {
        None => errs.push(FieldError::new(field, "invalid_value", "Not found")),
        Some("active") => {}
        Some(_) => errs.push(FieldError::new(field, "invalid_value", "This item is inactive")),
    }
    Ok(())
}

pub(crate) async fn mobile_holder(tx: &mut Tx, mobile: &str, except: Option<Uuid>) -> ApiResult<()> {
    let holder: Option<String> = sqlx::query_scalar(
        "SELECT employee_code FROM app.employees WHERE mobile = $1 AND is_current AND id IS DISTINCT FROM $2",
    )
    .bind(mobile)
    .bind(except)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(code) = holder {
        let mut err = AppError::conflict("mobile_in_use", "Mobile number in use", format!("Mobile number belongs to {code}"))
            .with_meta(json!({ "employee_code": code }));
        err.errors.push(FieldError::new("personal.mobile", "mobile_in_use", format!("Mobile number belongs to {code}")));
        return Err(err.into());
    }
    Ok(())
}

/// Takes the next running number in its own committed transaction, so a later failure still
/// uses it up (AC-EM-02.4). Skips numbers whose code is already taken (edge case 16).
async fn next_code(state: &crate::AppState, ctx: &Ctx, em: &EmSettings, branch_id: Uuid) -> ApiResult<String> {
    let pattern = em.code_pattern();
    let mut tx = db::begin_tenant(&state.pool, ctx.org_id, Some(ctx.user_id)).await?;
    let row = sqlx::query(
        "SELECT o.short_code AS org_code, b.short_code AS branch_code FROM app.organisations o, app.branches b
         WHERE o.id = $1 AND b.id = $2",
    )
    .bind(ctx.org_id)
    .bind(branch_id)
    .fetch_one(&mut *tx)
    .await?;
    let (org_code, branch_code): (String, String) = (row.get("org_code"), row.get("branch_code"));
    let key = pattern.sequence_key(&branch_id.to_string());
    for _ in 0..1000 {
        let last: Option<i32> = sqlx::query_scalar(
            "SELECT last_value FROM app.employee_code_sequences WHERE org_id = $1 AND sequence_key = $2 FOR UPDATE",
        )
        .bind(ctx.org_id)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await?;
        let n = pattern.next_number(last.unwrap_or(0) as u32);
        sqlx::query(
            "INSERT INTO app.employee_code_sequences (org_id, sequence_key, last_value) VALUES ($1, $2, $3)
             ON CONFLICT (org_id, sequence_key) DO UPDATE SET last_value = EXCLUDED.last_value",
        )
        .bind(ctx.org_id)
        .bind(&key)
        .bind(n as i32)
        .execute(&mut *tx)
        .await?;
        let code = pattern.format(&org_code, &branch_code, n);
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employees WHERE employee_code = $1)")
            .bind(&code)
            .fetch_one(&mut *tx)
            .await?;
        if !taken {
            tx.commit().await?;
            return Ok(code);
        }
    }
    Err(AppError::internal().into())
}

pub async fn create(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    mut ctx: Ctx,
    headers: HeaderMap,
    Body(raw): Body,
) -> ApiResult<Response> {
    ctx.scope("employee.create")?;
    if let Some((status, body)) = ctx.idempotency_begin(&headers, "POST /v1/employees", &raw).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let (id, over_limit, em) = create_in(&state, &mut ctx, raw).await?;
    finish_joiner(ctx, &headers, id, &em, over_limit, StatusCode::CREATED).await
}

/// E5 without idempotency or commit, so bulk import (J5) creates a row the same way.
/// Returns the new id, the over-limit figures (allow-and-alert) and the EM settings used.
pub(crate) async fn create_in(state: &crate::AppState, ctx: &mut Ctx, raw: Value) -> ApiResult<(Uuid, Option<(i32, i64)>, EmSettings)> {
    let scope = ctx.scope("employee.create")?;
    let req: CreateEmployee = parse(raw.clone())?;
    let today = today();
    let em = em_settings(&mut ctx.tx, today).await?;
    let mut errs = Vec::new();

    check_personal(&req.personal, |_| true, &em, &mut errs);
    match (em.employee_code_mode, blank(&req.employee_code)) {
        (CodeMode::Manual, true) => errs.push(FieldError::new("employee_code", "required", "Employee code is required")),
        (CodeMode::Automatic | CodeMode::Custom, false) => {
            errs.push(FieldError::new("employee_code", "invalid_value", "Codes are generated automatically for this organisation"))
        }
        _ => {}
    }
    if req.portal_access.as_deref().is_some_and(|p| p != "none") {
        errs.push(FieldError::new("portal_access", "invalid_value", "Grant portal access after creating the employee"));
    }
    let j = &req.job;
    check_joiner(&mut ctx.tx, &em, req.joining_date, req.initial_status, req.probation_end_date, j, &mut errs).await?;
    let (master, department) = (j.master_branch_id.unwrap_or_default(), j.department_id.unwrap_or_default());
    if errs.is_empty() && !(scope.org || scope.branches.contains(&master) || scope.departments.contains(&department)) {
        return Err(AppError::forbidden().into());
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let mobile = req.personal.mobile.as_deref().unwrap_or_default().trim().to_string();
    let over_limit = check_limit(ctx).await?;
    mobile_holder(&mut ctx.tx, &mobile, None).await?;

    let code = match em.employee_code_mode {
        CodeMode::Manual => {
            let code = req.employee_code.as_deref().unwrap_or_default().trim().to_string();
            let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employees WHERE employee_code = $1)")
                .bind(&code)
                .fetch_one(&mut *ctx.tx)
                .await?;
            if taken {
                return Err(AppError::conflict("employee_code_in_use", "Employee code in use", format!("{code} is already used")).into());
            }
            code
        }
        _ => next_code(state, ctx, &em, master).await?,
    };

    let p = &req.personal;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.employees (org_id, employee_code, full_name, date_of_birth, gender, mobile, email, address,
                                    emergency_contact_name, emergency_contact_phone)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(&code)
    .bind(p.full_name.as_deref().map(str::trim))
    .bind(p.date_of_birth)
    .bind(&p.gender)
    .bind(&mobile)
    .bind(p.email.as_deref().map(str::trim))
    .bind(&p.address)
    .bind(&p.emergency_contact_name)
    .bind(p.emergency_contact_phone.as_deref().map(str::trim))
    .fetch_one(&mut *ctx.tx)
    .await?;
    let (joining, status) = (req.joining_date.unwrap(), req.initial_status.unwrap());
    start_period(ctx, id, &em, joining, status, req.probation_end_date, j).await?;

    ctx.audit("employee.create", "employee", id, None, Some(raw), None).await?;
    ctx.event("EmployeeCreated", "employee", id, json!({
        "employee_id": id, "employee_code": code, "joining_date": joining, "master_branch_id": master, "status": status
    }))
    .await?;
    Ok((id, over_limit, em))
}

/// Allow-and-alert: the vendor hears that the limit was passed (AC-EM-18.2).
pub(crate) async fn limit_event(ctx: &mut Ctx, over_limit: Option<(i32, i64)>) -> ApiResult<()> {
    if let Some((limit, current)) = over_limit {
        ctx.event("EmployeeLimitExceeded", "organisation", ctx.org_id, json!({ "limit": limit, "current": current + 1 })).await?;
    }
    Ok(())
}

/// Writes the idempotent response, commits, and adds the over-limit header (shared by E5 and E12).
pub(crate) async fn finish_joiner(mut ctx: Ctx, headers: &HeaderMap, id: Uuid, em: &EmSettings, over_limit: Option<(i32, i64)>, status: StatusCode) -> ApiResult<Response> {
    limit_event(&mut ctx, over_limit).await?;
    let mut doc = load_employee(&mut ctx.tx, id, today()).await?.ok_or_else(AppError::internal)?;
    add_can(&mut ctx, id, &mut doc, em).await?;
    ctx.idempotency_finish(headers, status.as_u16(), &doc).await?;
    ctx.commit().await?;
    let mut res = employee_response(status, doc);
    if over_limit.is_some() {
        res.headers_mut().insert("x-employee-limit", axum::http::HeaderValue::from_static("exceeded"));
    }
    Ok(res)
}

/// Joining date, status and job checks for a new joiner or a rejoiner (AC-EM-14.3).
pub(crate) async fn check_joiner(
    tx: &mut Tx,
    em: &EmSettings,
    joining_date: Option<NaiveDate>,
    initial_status: Option<EmployeeStatus>,
    probation_end_date: Option<NaiveDate>,
    j: &JobFields,
    errs: &mut Vec<FieldError>,
) -> ApiResult<()> {
    let required = [
        ("joining_date", joining_date.is_none()),
        ("initial_status", initial_status.is_none()),
        ("job.master_branch_id", j.master_branch_id.is_none()),
        ("job.department_id", j.department_id.is_none()),
        ("job.category_id", j.category_id.is_none()),
        ("job.employee_type_id", j.employee_type_id.is_none()),
        ("job.payroll_group_id", j.payroll_group_id.is_none()),
    ];
    for (f, missing) in required {
        if missing {
            errs.push(FieldError::new(f, "required", format!("{f} is required")));
        }
    }
    let optional = [
        ("designation_id", j.designation_id.is_none()),
        ("contractor_id", j.contractor_id.is_none()),
        ("reporting_manager_id", j.reporting_manager_id.is_none()),
        ("enrolment_id", blank(&j.enrolment_id)),
        ("probation_end_date", probation_end_date.is_none()),
    ];
    for (f, missing) in optional {
        if missing && em.is_mandatory(f) {
            errs.push(FieldError::new(f, "required", format!("{f} is required")));
        }
    }
    if initial_status.is_some_and(|s| !s.is_valid_initial()) {
        errs.push(FieldError::new("initial_status", "invalid_value", "A new employee starts as active or probation"));
    }
    if let (Some(join), Some(end)) = (joining_date, probation_end_date) {
        if end < join {
            errs.push(FieldError::new("probation_end_date", "date_before_joining", "Probation cannot end before joining"));
        }
    }
    if j.enrolment_id.as_deref().is_some_and(|e| e.trim().is_empty()) {
        errs.push(FieldError::new("job.enrolment_id", "invalid_value", "Leave empty or enter an ID"));
    }

    // EM-06 allowed branches.
    match em.allowed_branches {
        AllowedBranches::Chosen => {}
        _ if j.allowed_branch_ids.is_empty() => {}
        _ => errs.push(FieldError::new("job.allowed_branch_ids", "invalid_value", "This organisation does not use chosen allowed branches")),
    }
    if j.master_branch_id.is_some_and(|m| j.allowed_branch_ids.contains(&m)) {
        errs.push(FieldError::new("job.allowed_branch_ids", "invalid_value", "The master branch is not also an allowed branch"));
    }

    check_item(tx, "branches", j.master_branch_id, "job.master_branch_id", errs).await?;
    for b in &j.allowed_branch_ids {
        check_item(tx, "branches", Some(*b), "job.allowed_branch_ids", errs).await?;
    }
    check_item(tx, "departments", j.department_id, "job.department_id", errs).await?;
    check_item(tx, "designations", j.designation_id, "job.designation_id", errs).await?;
    check_item(tx, "categories", j.category_id, "job.category_id", errs).await?;
    check_item(tx, "employee_types", j.employee_type_id, "job.employee_type_id", errs).await?;
    check_item(tx, "contractors", j.contractor_id, "job.contractor_id", errs).await?;
    check_item(tx, "payroll_groups", j.payroll_group_id, "job.payroll_group_id", errs).await?;
    if let Some(m) = j.reporting_manager_id {
        let current: Option<bool> = sqlx::query_scalar("SELECT is_current FROM app.employees WHERE id = $1")
            .bind(m)
            .fetch_optional(&mut **tx)
            .await?;
        if current != Some(true) {
            errs.push(FieldError::new("job.reporting_manager_id", "invalid_value", "The manager must be a current employee"));
        }
    }
    Ok(())
}

/// Plan limit (BR-EM-12, VC-01): Err when blocking, Some((limit, current)) when over with allow-and-alert.
/// Takes a per-organisation lock so two joiners can't both take the last place.
pub(crate) async fn check_limit(ctx: &mut Ctx) -> ApiResult<Option<(i32, i64)>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))").bind(ctx.org_id).execute(&mut *ctx.tx).await?;
    let lim = sqlx::query(
        "SELECT o.employee_limit, o.limit_behaviour::text AS behaviour,
                (SELECT count(*) FROM app.employees WHERE is_current) AS current
         FROM app.organisations o WHERE o.id = $1",
    )
    .bind(ctx.org_id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let (limit, current): (i32, i64) = (lim.get("employee_limit"), lim.get("current"));
    let over_limit = current >= limit as i64;
    if over_limit && lim.get::<String, _>("behaviour") == "block" {
        return Err(AppError::conflict("employee_limit_reached", "Employee limit reached", format!("Your plan allows {limit} employees"))
            .with_meta(json!({ "limit": limit }))
            .into());
    }
    Ok(over_limit.then_some((limit, current)))
}

/// New open period, first job version and first status row (create and rejoin).
pub(crate) async fn start_period(
    ctx: &mut Ctx,
    id: Uuid,
    em: &EmSettings,
    joining: NaiveDate,
    status: EmployeeStatus,
    probation_end_date: Option<NaiveDate>,
    j: &JobFields,
) -> ApiResult<()> {
    let (master, department) = (j.master_branch_id.unwrap(), j.department_id.unwrap());
    let probation_end = probation_end_date.or_else(|| {
        (status == EmployeeStatus::Probation).then(|| em.default_probation_end(joining)).flatten()
    });
    let period: Uuid = sqlx::query_scalar(
        "INSERT INTO app.employment_periods (org_id, employee_id, joining_date, probation_end_date)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(joining)
    .bind(probation_end)
    .fetch_one(&mut *ctx.tx)
    .await?;

    let version: Uuid = sqlx::query_scalar(
        "INSERT INTO app.employee_job_versions (org_id, employee_id, employment_period_id, valid, master_branch_id,
             department_id, designation_id, category_id, employee_type_id, contractor_id, reporting_manager_id,
             payroll_group_id, enrolment_id, change_reason)
         VALUES ($1, $2, $3, daterange($4, NULL), $5, $6, $7, $8, $9, $10, $11, $12, $13, 'Joined') RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(period)
    .bind(joining)
    .bind(master)
    .bind(department)
    .bind(j.designation_id)
    .bind(j.category_id)
    .bind(j.employee_type_id)
    .bind(j.contractor_id)
    .bind(j.reporting_manager_id)
    .bind(j.payroll_group_id)
    .bind(j.enrolment_id.as_deref().map(str::trim))
    .fetch_one(&mut *ctx.tx)
    .await?;
    for b in &j.allowed_branch_ids {
        sqlx::query("INSERT INTO app.job_version_allowed_branches (org_id, job_version_id, branch_id) VALUES ($1, $2, $3)")
            .bind(ctx.org_id)
            .bind(version)
            .bind(b)
            .execute(&mut *ctx.tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO app.employee_status_changes (org_id, employee_id, employment_period_id, status, effective_from)
         VALUES ($1, $2, $3, $4::app.employee_status, $5)",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(period)
    .bind(serde_json::to_value(status).unwrap().as_str().unwrap_or_default().to_string())
    .bind(joining)
    .execute(&mut *ctx.tx)
    .await?;
    Ok(())
}

// ---------- E8 PATCH /employees/{id}/personal ----------

const PERSONAL_FIELDS: [&str; 8] = [
    "full_name", "mobile", "date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone",
];

pub async fn patch_personal(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let em = personal_in(&mut ctx, id, raw, Some(expected), "employee.edit").await?;
    let mut doc = load_employee(&mut ctx.tx, id, today()).await?.ok_or_else(AppError::not_found)?;
    add_can(&mut ctx, id, &mut doc, &em).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, doc))
}

/// E6 without commit; `expected` None (bulk import) skips the version check but still locks the row.
/// `permission` is checked against the employee and names the audit action (M2 uses the self-edit one).
pub(crate) async fn personal_in(ctx: &mut Ctx, id: Uuid, raw: Value, expected: Option<i32>, permission: &str) -> ApiResult<EmSettings> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(ctx, id, permission, &em).await?;

    let changes = raw.as_object().cloned().ok_or_else(|| AppError::unprocessable("invalid_value", "Request not valid", "Send a JSON object"))?;
    let p: Personal = parse(raw.clone())?; // also refuses unknown fields and wrong types
    let mut errs = Vec::new();
    check_personal(&p, |f| changes.contains_key(f), &em, &mut errs);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }

    let row = sqlx::query(
        "SELECT row_version, jsonb_build_object('full_name', full_name, 'mobile', mobile, 'date_of_birth', date_of_birth,
             'gender', gender, 'email', email, 'address', address, 'emergency_contact_name', emergency_contact_name,
             'emergency_contact_phone', emergency_contact_phone) AS personal
         FROM app.employees WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let version: i32 = row.get("row_version");
    if expected.is_some_and(|e| e != version) {
        return Err(stale(version));
    }
    let old: Value = row.get("personal");
    // Trimmed new values, compared with the stored ones; only real changes are written and audited.
    let new: serde_json::Map<String, Value> = changes
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().map(|s| Value::from(s.trim())).unwrap_or_else(|| v.clone())))
        .filter(|(k, v)| old.get(k) != Some(v))
        .collect();

    if !new.is_empty() {
        if let Some(m) = new.get("mobile").and_then(Value::as_str) {
            mobile_holder(&mut ctx.tx, m, Some(id)).await?;
        }
        let mut qb = QueryBuilder::new("UPDATE app.employees SET row_version = row_version + 1");
        for f in PERSONAL_FIELDS.iter().filter(|f| new.contains_key(**f)) {
            qb.push(format!(", {f} = "));
            if *f == "date_of_birth" {
                qb.push_bind(p.date_of_birth);
            } else {
                qb.push_bind(new[*f].as_str().map(str::to_string));
            }
        }
        qb.push(" WHERE id = ").push_bind(id);
        qb.build().execute(&mut *ctx.tx).await?;
        let old_subset: serde_json::Map<String, Value> = new.keys().map(|k| (k.clone(), old[k].clone())).collect();
        ctx.audit(permission, "employee", id, Some(Value::Object(old_subset)), Some(Value::Object(new)), None).await?;
    }
    Ok(em)
}

// ---------- M1 / M2 self-service ----------

/// The employee's own record: personal, employment, status, job and custom fields only (§2.1);
/// the version travels in the ETag for M2.
async fn own_profile(ctx: &mut Ctx, id: Uuid) -> ApiResult<Response> {
    let doc = load_employee(&mut ctx.tx, id, today()).await?.ok_or_else(AppError::not_found)?;
    let body: serde_json::Map<String, Value> =
        ["personal", "employment", "status", "job", "custom_fields"].iter().map(|k| (k.to_string(), doc[*k].clone())).collect();
    let etag = format!("\"{}\"", doc["version"]);
    Ok(([(header::ETAG, etag)], Json(Value::Object(body))).into_response())
}

/// M1 (AC-EM-16.1, 16.2): only ever the caller's own employee record.
pub async fn me_profile(mut ctx: Ctx) -> ApiResult<Response> {
    ctx.scope("employee.view")?;
    let id = ctx.employee_id.ok_or_else(AppError::not_found)?;
    let res = own_profile(&mut ctx, id).await?;
    ctx.commit().await?;
    Ok(res)
}

const CONTACT_FIELDS: [&str; 5] = ["mobile", "email", "address", "emergency_contact_name", "emergency_contact_phone"];

/// M2 (AC-EM-17.1–17.4, BR-EM-23): contact details, applied at once and audited, while EM-03 is on.
pub async fn me_contact(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    ctx.scope("employee.profile.edit_own")?;
    let id = ctx.employee_id.ok_or_else(AppError::not_found)?;
    if !em_settings(&mut ctx.tx, today()).await?.self_edit_contact {
        return Err(AppError::new(403, "self_edit_disabled", "Read only", "Your organisation doesn't allow editing contact details").into());
    }
    let errs: Vec<FieldError> = raw
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .filter(|k| !CONTACT_FIELDS.contains(&k.as_str()))
        .map(|k| FieldError::new(k.clone(), "unknown_field", "Only contact details can be changed here"))
        .collect();
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    personal_in(&mut ctx, id, raw, Some(expected), "employee.profile.edit_own").await?;
    let res = own_profile(&mut ctx, id).await?;
    ctx.commit().await?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::is_e164;

    #[test]
    fn e164() {
        assert!(is_e164("+919876543210"));
        assert!(!is_e164("9876543210"));
        assert!(!is_e164("+0919876543"));
        assert!(!is_e164("+91 98765"));
    }
}
