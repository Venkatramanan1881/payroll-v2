//! Organisation structure O1–O19 (API_CONTRACTS §4): branches, reference lists, contractors,
//! payroll groups, settings and custom field definitions. Scope rule `org`: the permission is enough (§1.3).
//! Entries are deactivated, never deleted (BR-OR-02); renames go to name history by trigger (AC-OR-02.3).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{allowed, bad_param, em_settings, employee_response, push_from, push_scope, query_pairs, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::job_changes::{apply, bump_version, load_versions, plan_change, PlanError};
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use payroll_core::employee_code::CodePattern;
use payroll_core::settings::{check_group, filled, GROUPS};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

/// One row of `t` as the API shows it.
const ITEM: &str = "((to_jsonb(t) - 'org_id' - 'created_by' - 'created_at' - 'updated_at' - 'row_version') || jsonb_build_object('version', t.row_version))";

/// A table managed by the shared list routes.
pub(crate) struct Kind {
    pub(crate) table: &'static str,
    pub(crate) target: &'static str,
    pub(crate) manage: &'static str,
    pub(crate) code: &'static str,
    pub(crate) named: bool,
    pub(crate) extra: &'static [&'static str],
}

const BRANCH: Kind = Kind { table: "branches", target: "branch", manage: "org.branch.manage", code: "short_code", named: true, extra: &["address", "time_zone"] };
const CONTRACTOR: Kind = Kind {
    table: "contractors", target: "contractor", manage: "org.contractor.manage", code: "code", named: true,
    extra: &["contact_name", "contact_phone", "contact_email"],
};
const CUSTOM_FIELD: Kind = Kind {
    table: "custom_field_definitions", target: "custom_field", manage: "employee.custom_fields.manage", code: "label", named: false, extra: &[],
};

fn list_kind(list: &str) -> ApiResult<Kind> {
    let (table, target) = match list {
        "departments" => ("departments", "department"),
        "designations" => ("designations", "designation"),
        "categories" => ("categories", "category"),
        "employee-types" => ("employee_types", "employee_type"),
        "exit-reasons" => ("exit_reasons", "exit_reason"),
        _ => return Err(AppError::not_found().into()),
    };
    Ok(Kind { table, target, manage: "org.lists.manage", code: "code", named: true, extra: &[] })
}

fn can_view(ctx: &Ctx, manage: &str) -> ApiResult<()> {
    if ctx.has("employee.view") || ctx.has(manage) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

fn reply(status: StatusCode, doc: Value) -> Response {
    if doc.get("version").is_some() { employee_response(status, doc) } else { (status, Json(doc)).into_response() }
}

fn reason_of(r: Option<String>) -> Option<String> {
    r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

async fn item(tx: &mut Tx, table: &str, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("SELECT {ITEM} FROM app.{table} t WHERE t.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

/// Locks the row and checks If-Match; returns it as it was.
async fn lock(tx: &mut Tx, table: &str, id: Uuid, expected: i32) -> ApiResult<Value> {
    let v: i32 = sqlx::query_scalar(&format!("SELECT row_version FROM app.{table} WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if v != expected {
        return Err(stale(v));
    }
    item(tx, table, id).await
}

/// Text fields of a list-like body: only the kind's own keys, code and name never blank.
fn fields(k: &Kind, raw: Value, creating: bool) -> ApiResult<Map<String, Value>> {
    let Value::Object(m) = raw else {
        return Err(AppError::unprocessable("invalid_value", "Request not valid", "Send a JSON object").into());
    };
    let required = |key: &str| key == k.code || key == "name" || key == "time_zone";
    let (mut out, mut errs) = (Map::new(), Vec::new());
    for (key, v) in m {
        if !(key == k.code || (k.named && key == "name") || k.extra.contains(&key.as_str())) {
            return Err(AppError::unprocessable("unknown_field", "Request not valid", format!("unknown field `{key}`")).into());
        }
        match v {
            Value::String(s) if !s.trim().is_empty() => {
                out.insert(key, json!(s.trim()));
            }
            Value::String(_) | Value::Null if !required(&key) => {
                out.insert(key, Value::Null);
            }
            Value::String(_) | Value::Null => errs.push(FieldError::new(key, "required", "This can't be blank")),
            _ => errs.push(FieldError::new(key, "invalid_value", "Must be text")),
        }
    }
    if creating {
        for key in [k.code, "name"].into_iter().filter(|key| *key == k.code || k.named) {
            if !out.contains_key(key) && !errs.iter().any(|e| e.field == key) {
                errs.push(FieldError::new(key, "required", "This is required"));
            }
        }
    }
    if k.table == "branches" {
        if let Some(code) = out.get("short_code").and_then(Value::as_str) {
            let ok = (2..=6).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            if !ok {
                errs.push(FieldError::new("short_code", "invalid_short_code", "Use 2 to 6 capital letters or digits"));
            }
        }
    }
    if errs.is_empty() { Ok(out) } else { Err(AppError::validation(errs).into()) }
}

async fn check_time_zone(tx: &mut Tx, f: &Map<String, Value>) -> ApiResult<()> {
    let Some(tz) = f.get("time_zone").and_then(Value::as_str) else { return Ok(()) };
    let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)")
        .bind(tz)
        .fetch_one(&mut **tx)
        .await?;
    if known { Ok(()) } else { Err(AppError::validation(vec![FieldError::new("time_zone", "unknown_time_zone", "Time zone not recognised")]).into()) }
}

fn changed(old: &Value, new: &Value, keys: impl Iterator<Item = String>) -> (Value, Value) {
    let (mut o, mut n) = (Map::new(), Map::new());
    for k in keys {
        if old[&k] != new[&k] {
            o.insert(k.clone(), old[&k].clone());
            n.insert(k.clone(), new[&k].clone());
        }
    }
    (Value::Object(o), Value::Object(n))
}

// ---------- shared list routes (O1–O8, O10, O19 list/deactivate) ----------

async fn items(mut ctx: Ctx, k: &Kind, raw: Option<String>) -> ApiResult<Json<Value>> {
    can_view(&ctx, k.manage)?;
    let mut qb = QueryBuilder::new(format!(
        "SELECT coalesce(jsonb_agg({ITEM} ORDER BY lower(t.{code})), '[]') FROM app.{table} t WHERE true",
        code = k.code,
        table = k.table
    ));
    for (name, v) in query_pairs(raw)? {
        match name.as_str() {
            "state" if v == "active" || v == "inactive" => {
                qb.push(" AND t.state::text = ").push_bind(v);
            }
            "state" => return Err(bad_param("state", "active or inactive").into()),
            "q" if !v.trim().is_empty() => {
                let like = format!("%{}%", v.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
                qb.push(format!(" AND (t.{} ILIKE ", k.code)).push_bind(like.clone());
                if k.named {
                    qb.push(" OR t.name ILIKE ").push_bind(like);
                }
                qb.push(")");
            }
            "q" => {}
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{name}` is not supported")).into()),
        }
    }
    let items: Value = qb.build_query_scalar().fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

async fn create(mut ctx: Ctx, k: &Kind, headers: HeaderMap, raw: Value, route: &str) -> ApiResult<Response> {
    ctx.scope(k.manage)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, route, &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let mut f = fields(k, raw, true)?;
    if k.table == "branches" {
        f.entry("time_zone").or_insert(json!("Asia/Kolkata")); // AC-OR-01.1
    }
    check_time_zone(&mut ctx.tx, &f).await?;
    let mut qb = QueryBuilder::new(format!("INSERT INTO app.{} (org_id", k.table));
    for key in f.keys() {
        qb.push(", ").push(key); // keys were checked against the kind's fixed list
    }
    qb.push(") VALUES (").push_bind(ctx.org_id);
    for v in f.values() {
        qb.push(", ").push_bind(v.as_str().map(str::to_string));
    }
    qb.push(") RETURNING id");
    let id: Uuid = qb.build_query_scalar().fetch_one(&mut *ctx.tx).await?;
    let doc = item(&mut ctx.tx, k.table, id).await?;
    ctx.audit(k.manage, k.target, id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::CREATED, doc))
}

async fn patch(mut ctx: Ctx, k: &Kind, id: Uuid, headers: HeaderMap, raw: Value) -> ApiResult<Response> {
    ctx.scope(k.manage)?;
    let expected = if_match(&headers)?;
    let f = fields(k, raw, false)?;
    check_time_zone(&mut ctx.tx, &f).await?;
    let old = lock(&mut ctx.tx, k.table, id, expected).await?;
    let mut qb = QueryBuilder::new(format!("UPDATE app.{} SET row_version = row_version + 1", k.table));
    for (key, v) in &f {
        qb.push(format!(", {key} = ")).push_bind(v.as_str().map(str::to_string));
    }
    qb.push(" WHERE id = ").push_bind(id);
    qb.build().execute(&mut *ctx.tx).await?;
    let new = item(&mut ctx.tx, k.table, id).await?;
    let (o, n) = changed(&old, &new, f.keys().cloned());
    ctx.audit(k.manage, k.target, id, Some(o), Some(n), None).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, new))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

pub(crate) async fn set_state(mut ctx: Ctx, k: &Kind, id: Uuid, headers: HeaderMap, raw: Value, to: &str) -> ApiResult<Response> {
    ctx.scope(k.manage)?;
    let expected = if_match(&headers)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    if to == "inactive" && reason.is_none() {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    }
    let old = lock(&mut ctx.tx, k.table, id, expected).await?;
    let mut doc = old.clone();
    if old["state"] != to {
        sqlx::query(&format!("UPDATE app.{} SET state = $2::app.item_state, row_version = row_version + 1 WHERE id = $1", k.table))
            .bind(id)
            .bind(to)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.audit(k.manage, k.target, id, Some(json!({ "state": old["state"] })), Some(json!({ "state": to })), reason.as_deref()).await?;
        doc = item(&mut ctx.tx, k.table, id).await?;
    }
    if k.table == "branches" && to == "inactive" {
        // Edge case 9: allowed, but HR sees who still has this branch now or later.
        doc["employees_with_this_branch"] = sqlx::query_scalar(
            "SELECT coalesce(jsonb_agg(DISTINCT jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name)), '[]')
             FROM app.employees e JOIN app.employee_job_versions v ON v.employee_id = e.id AND v.state = 'active'
             WHERE e.is_current AND v.master_branch_id = $1 AND (upper(v.valid) IS NULL OR upper(v.valid) > $2)",
        )
        .bind(id)
        .bind(today())
        .fetch_one(&mut *ctx.tx)
        .await?;
    }
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, doc))
}

// ---------- O1–O4 branches ----------

pub async fn branches(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    items(ctx, &BRANCH, raw).await
}
pub async fn branch_create(ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    create(ctx, &BRANCH, headers, raw, "POST /v1/org/branches").await
}
pub async fn branch_patch(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    patch(ctx, &BRANCH, id, headers, raw).await
}
pub async fn branch_deactivate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &BRANCH, id, headers, raw, "inactive").await
}
pub async fn branch_activate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &BRANCH, id, headers, raw, "active").await
}

// ---------- O5–O8 lists ----------

pub async fn lists(ctx: Ctx, Path(list): Path<String>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    items(ctx, &list_kind(&list)?, raw).await
}
pub async fn list_create(ctx: Ctx, Path(list): Path<String>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    create(ctx, &list_kind(&list)?, headers, raw, &format!("POST /v1/org/lists/{list}")).await
}
pub async fn list_patch(ctx: Ctx, Path((list, id)): Path<(String, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    patch(ctx, &list_kind(&list)?, id, headers, raw).await
}
pub async fn list_deactivate(ctx: Ctx, Path((list, id)): Path<(String, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &list_kind(&list)?, id, headers, raw, "inactive").await
}
pub async fn list_activate(ctx: Ctx, Path((list, id)): Path<(String, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &list_kind(&list)?, id, headers, raw, "active").await
}

// ---------- O9 POST /org/lists/apply-template ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sector {
    sector: String,
}

pub async fn apply_template(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("org.lists.manage")?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/org/lists/apply-template", &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let req: Sector = parse(raw)?;
    let t = payroll_core::templates::template(&req.sector)
        .ok_or_else(|| AppError::validation(vec![FieldError::new("sector", "invalid_value", "Unknown sector")]))?;
    let used: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM app.departments) OR EXISTS (SELECT 1 FROM app.designations)
             OR EXISTS (SELECT 1 FROM app.categories) OR EXISTS (SELECT 1 FROM app.employee_types)",
    )
    .fetch_one(&mut *ctx.tx)
    .await?;
    if used {
        return Err(AppError::conflict("lists_not_empty", "Lists already filled", "A template can only fill empty lists").into());
    }
    let mut added = Map::new();
    for (table, list) in [("departments", t.departments), ("designations", t.designations), ("categories", t.categories), ("employee_types", t.employee_types)] {
        let (codes, names): (Vec<&str>, Vec<&str>) = list.iter().copied().unzip();
        sqlx::query(&format!("INSERT INTO app.{table} (org_id, code, name) SELECT $1, c, n FROM unnest($2::text[], $3::text[]) AS x (c, n)"))
            .bind(ctx.org_id)
            .bind(codes)
            .bind(names)
            .execute(&mut *ctx.tx)
            .await?;
        added.insert(table.into(), json!(list.len()));
    }
    let body = json!({ "sector": req.sector, "added": added });
    ctx.audit("org.lists.manage", "organisation", ctx.org_id, None, Some(body.clone()), None).await?;
    ctx.idempotency_finish(&headers, 200, &body).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}

// ---------- O10–O11 contractors ----------

pub async fn contractors(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    items(ctx, &CONTRACTOR, raw).await
}
pub async fn contractor_create(ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    create(ctx, &CONTRACTOR, headers, raw, "POST /v1/org/contractors").await
}
pub async fn contractor_patch(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    patch(ctx, &CONTRACTOR, id, headers, raw).await
}
pub async fn contractor_deactivate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &CONTRACTOR, id, headers, raw, "inactive").await
}
pub async fn contractor_activate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &CONTRACTOR, id, headers, raw, "active").await
}

fn as_of_param(raw: Option<String>) -> ApiResult<NaiveDate> {
    let mut as_of = today();
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "as_of" => as_of = v.parse().map_err(|_| bad_param("as_of", "Use YYYY-MM-DD"))?,
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    Ok(as_of)
}

/// O11: workers linked to the contractor on `as_of`, limited to those the caller can see (AC-OR-03.2).
pub async fn contractor_workers(mut ctx: Ctx, Path(id): Path<Uuid>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope("employee.view")?;
    let as_of = as_of_param(raw)?;
    item(&mut ctx.tx, "contractors", id).await?;
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name,
           'master_branch_id', jv.master_branch_id, 'linked_from', lower(jv.valid)) ORDER BY e.employee_code), '[]')",
    );
    push_from(&mut qb, as_of);
    qb.push(" WHERE jv.contractor_id = ").push_bind(id).push(" AND jv.valid @> ").push_bind(as_of).push("::date AND ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    let items: Value = qb.build_query_scalar().fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "as_of": as_of, "items": items })))
}

// ---------- O12–O15 payroll groups ----------

const GROUP_ITEM: &str = "SELECT ((to_jsonb(t) - 'org_id' - 'created_by' - 'created_at' - 'updated_at' - 'row_version')
    || jsonb_build_object('version', t.row_version, 'member_count_today', (SELECT count(*) FROM app.employee_job_versions v
         WHERE v.payroll_group_id = t.id AND v.state = 'active' AND v.valid @> $1::date))) FROM app.payroll_groups t";

async fn group_item(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("{GROUP_ITEM} WHERE t.id = $2"))
        .bind(today())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    name: Option<String>,
    pay_frequency: Option<String>,
    month_start_day: Option<i16>,
    week_start_day: Option<i16>,
    cycle_anchor: Option<NaiveDate>,
}

/// Each pay frequency takes only its own cycle field (DBQ-02).
fn check_group_fields(g: &Group) -> Vec<FieldError> {
    let mut errs = Vec::new();
    if g.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
        errs.push(FieldError::new("name", "required", "Give the group a name"));
    }
    let bad = |f: &str, m: &str| FieldError::new(f, "invalid_cycle", m);
    let (month, week, anchor) = (g.month_start_day, g.week_start_day, g.cycle_anchor);
    match g.pay_frequency.as_deref() {
        Some("daily") => {}
        Some("weekly") if !week.is_some_and(|w| (1..=7).contains(&w)) => errs.push(bad("week_start_day", "Weekly groups need a start day, 1 (Monday) to 7")),
        Some("weekly") => {}
        Some("bi_weekly") if anchor.is_none() => errs.push(bad("cycle_anchor", "Bi-weekly groups need the first day of any cycle")),
        Some("bi_weekly") => {}
        Some("monthly") if month.is_some_and(|m| !(1..=28).contains(&m)) => errs.push(bad("month_start_day", "Day 1 to 28, or none for the calendar month")),
        Some("monthly") => {}
        _ => errs.push(FieldError::new("pay_frequency", "invalid_value", "daily, weekly, bi_weekly or monthly")),
    }
    let f = g.pay_frequency.as_deref().unwrap_or_default();
    if month.is_some() && f != "monthly" {
        errs.push(bad("month_start_day", "Only monthly groups have a start day of the month"));
    }
    if week.is_some() && f != "weekly" {
        errs.push(bad("week_start_day", "Only weekly groups have a start day of the week"));
    }
    if anchor.is_some() && f != "bi_weekly" {
        errs.push(bad("cycle_anchor", "Only bi-weekly groups have a cycle anchor"));
    }
    errs
}

pub async fn groups(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    can_view(&ctx, "org.payroll_group.manage")?;
    let rows: Vec<Value> = sqlx::query_scalar(&format!("{GROUP_ITEM} ORDER BY t.is_default DESC, lower(t.name)"))
        .bind(today())
        .fetch_all(&mut *ctx.tx)
        .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": rows })))
}

async fn save_group(ctx: &mut Ctx, id: Option<Uuid>, g: &Group) -> ApiResult<Uuid> {
    let sql = match id {
        None => "INSERT INTO app.payroll_groups (org_id, name, pay_frequency, month_start_day, week_start_day, cycle_anchor)
                 VALUES ($1, $2, $3::app.pay_frequency, $4, $5, $6) RETURNING id",
        Some(_) => "UPDATE app.payroll_groups SET name = $2, pay_frequency = $3::app.pay_frequency, month_start_day = $4,
                 week_start_day = $5, cycle_anchor = $6, row_version = row_version + 1 WHERE id = $7 RETURNING id",
    };
    Ok(sqlx::query_scalar(sql)
        .bind(ctx.org_id)
        .bind(g.name.as_deref().map(str::trim))
        .bind(&g.pay_frequency)
        .bind(g.month_start_day)
        .bind(g.week_start_day)
        .bind(g.cycle_anchor)
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?)
}

pub async fn group_create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("org.payroll_group.manage")?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/org/payroll-groups", &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let mut g: Group = parse(raw)?;
    g.pay_frequency.get_or_insert_with(|| "monthly".into());
    let errs = check_group_fields(&g);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let id = save_group(&mut ctx, None, &g).await?;
    let doc = group_item(&mut ctx.tx, id).await?;
    ctx.audit("org.payroll_group.manage", "payroll_group", id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::CREATED, doc))
}

pub async fn group_patch(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("org.payroll_group.manage")?;
    let expected = if_match(&headers)?;
    let _: Group = parse(raw.clone())?; // shape check; missing keys keep their current value
    lock(&mut ctx.tx, "payroll_groups", id, expected).await?;
    let old = group_item(&mut ctx.tx, id).await?;
    let mut merged = Map::new();
    for k in ["name", "pay_frequency", "month_start_day", "week_start_day", "cycle_anchor"] {
        merged.insert(k.into(), raw.get(k).unwrap_or(&old[k]).clone());
    }
    let g: Group = parse(Value::Object(merged))?;
    let errs = check_group_fields(&g);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    save_group(&mut ctx, Some(id), &g).await?;
    let new = group_item(&mut ctx.tx, id).await?;
    let keys = ["name", "pay_frequency", "month_start_day", "week_start_day", "cycle_anchor"].map(String::from);
    let (o, n) = changed(&old, &new, keys.into_iter());
    ctx.audit("org.payroll_group.manage", "payroll_group", id, Some(o), Some(n), None).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, new))
}

/// O14: only a group that never had a member on any date (AC-OR-05.5), and never the default.
pub async fn group_delete(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap) -> ApiResult<Response> {
    ctx.scope("org.payroll_group.manage")?;
    let expected = if_match(&headers)?;
    let old = lock(&mut ctx.tx, "payroll_groups", id, expected).await?;
    if old["is_default"] == true {
        return Err(AppError::conflict("cannot_delete_default", "Default group", "The default payroll group can't be deleted").into());
    }
    let used: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employee_job_versions WHERE payroll_group_id = $1)")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if used {
        return Err(AppError::conflict("group_has_members", "Group has had members", "A group that has ever had members can't be deleted").into());
    }
    sqlx::query("DELETE FROM app.payroll_groups WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
    ctx.audit("org.payroll_group.manage", "payroll_group", id, Some(old), None, None).await?;
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct Rule {
    branch_ids: Vec<Uuid>,
    department_ids: Vec<Uuid>,
    category_ids: Vec<Uuid>,
    employee_type_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assign {
    effective_from: Option<NaiveDate>,
    employee_ids: Option<Vec<Uuid>>,
    rule: Option<Rule>,
    reason: Option<String>,
}

/// O15: one dated job change per employee, as E6. Employees the caller can't edit are skipped (not failed).
pub async fn group_assign(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("org.payroll_group.manage")?;
    ctx.scope("employee.edit")?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/org/payroll-groups/{id}/assign"), &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let req: Assign = parse(raw)?;
    let mut errs = Vec::new();
    if req.effective_from.is_none() {
        errs.push(FieldError::new("effective_from", "required", "Choose the date the change starts"));
    }
    let reason = reason_of(req.reason);
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    match (&req.employee_ids, &req.rule) {
        (None, None) => errs.push(FieldError::new("employee_ids", "required", "Choose employees or a rule")),
        (Some(_), Some(_)) => errs.push(FieldError::new("rule", "invalid_value", "Send employee_ids or a rule, not both")),
        (None, Some(r)) if r.branch_ids.is_empty() && r.department_ids.is_empty() && r.category_ids.is_empty() && r.employee_type_ids.is_empty() => {
            errs.push(FieldError::new("rule", "invalid_value", "A rule needs at least one branch, department, category or employee type"))
        }
        _ => {}
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (d, reason) = (req.effective_from.unwrap(), reason.unwrap());
    let state = group_item(&mut ctx.tx, id).await?["state"].clone();
    if state != "active" {
        return Err(AppError::validation(vec![FieldError::new("payroll_group", "invalid_value", "This group is inactive")]).into());
    }
    let mut targets: Vec<Uuid> = match (req.employee_ids, req.rule) {
        (Some(ids), _) => ids,
        (_, Some(r)) => sqlx::query_scalar(
            "SELECT DISTINCT v.employee_id FROM app.employee_job_versions v JOIN app.employees e ON e.id = v.employee_id
             WHERE e.is_current AND v.state = 'active' AND v.valid @> $1::date
               AND (cardinality($2::uuid[]) = 0 OR v.master_branch_id = ANY($2))
               AND (cardinality($3::uuid[]) = 0 OR v.department_id = ANY($3))
               AND (cardinality($4::uuid[]) = 0 OR v.category_id = ANY($4))
               AND (cardinality($5::uuid[]) = 0 OR v.employee_type_id = ANY($5))",
        )
        .bind(d)
        .bind(r.branch_ids)
        .bind(r.department_ids)
        .bind(r.category_ids)
        .bind(r.employee_type_ids)
        .fetch_all(&mut *ctx.tx)
        .await?,
        _ => unreachable!("checked above"),
    };
    targets.sort();
    targets.dedup();

    let em = em_settings(&mut ctx.tx, today()).await?;
    let change: Map<String, Value> = json!({ "payroll_group_id": id }).as_object().unwrap().clone();
    let (mut done, mut skipped) = (0, Vec::new());
    for e in targets {
        let skip = |code: &str| json!({ "employee_id": e, "code": code });
        if !allowed(&mut ctx, e, "employee.edit", Access::Edit, &em).await? {
            skipped.push(skip("not_found"));
            continue;
        }
        sqlx::query("SELECT 1 FROM app.employees WHERE id = $1 FOR UPDATE").bind(e).execute(&mut *ctx.tx).await?;
        if crate::periods::lock_on(&mut ctx.tx, e, d).await?.is_some() {
            skipped.push(skip("date_in_locked_period"));
            continue;
        }
        let Some(p) = sqlx::query("SELECT id, joining_date FROM app.employment_periods WHERE employee_id = $1 AND state = 'open'")
            .bind(e)
            .fetch_optional(&mut *ctx.tx)
            .await?
        else {
            skipped.push(skip("employee_not_current"));
            continue;
        };
        let period: Uuid = p.get("id");
        if d < p.get::<NaiveDate, _>("joining_date") {
            skipped.push(skip("date_before_joining"));
            continue;
        }
        let versions = load_versions(&mut ctx.tx, e, period).await?;
        let reps = match plan_change(&versions, d, &change, &reason) {
            Ok(r) => r,
            Err(PlanError::NoChange) => {
                skipped.push(skip("no_change"));
                continue;
            }
            Err(_) => {
                skipped.push(skip("date_before_joining"));
                continue;
            }
        };
        let old_group = versions.iter().find(|v| v.id == reps[0].old).map(|v| v.content["payroll_group_id"].clone());
        apply(&mut ctx, e, period, &reps).await?;
        bump_version(&mut ctx.tx, e).await?;
        ctx.audit(
            "employee.edit",
            "employee",
            e,
            Some(json!({ "effective_from": d, "job": { "payroll_group_id": old_group } })),
            Some(json!({ "effective_from": d, "job": change })),
            Some(&reason),
        )
        .await?;
        ctx.event("JobChanged", "employee", e, json!({ "employee_id": e, "effective_from": d, "changed_fields": ["payroll_group_id"] })).await?;
        done += 1;
    }
    let body = json!({ "changed": done, "skipped": skipped });
    ctx.idempotency_finish(&headers, 200, &body).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}

// ---------- O16–O18 settings ----------

async fn in_force(tx: &mut Tx, group: &str, on: NaiveDate) -> ApiResult<Value> {
    let row = sqlx::query(
        "SELECT setting_values, valid_from FROM app.org_settings_versions
         WHERE settings_group = $1 AND valid_from <= $2 ORDER BY valid_from DESC, recorded_at DESC LIMIT 1",
    )
    .bind(group)
    .bind(on)
    .fetch_optional(&mut **tx)
    .await?;
    let stored = row.as_ref().map(|r| r.get::<Value, _>("setting_values")).unwrap_or(json!({}));
    let mut v = filled(group, stored).unwrap_or_default();
    v["valid_from"] = json!(row.map(|r| r.get::<NaiveDate, _>("valid_from")));
    Ok(v)
}

pub async fn settings(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    can_view(&ctx, "org.settings.edit")?;
    let as_of = as_of_param(raw)?;
    let mut groups = Map::new();
    for g in GROUPS {
        groups.insert(g.into(), in_force(&mut ctx.tx, g, as_of).await?);
    }
    ctx.commit().await?;
    Ok(Json(json!({ "as_of": as_of, "groups": groups })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSettings {
    valid_from: Option<NaiveDate>,
    values: Option<Map<String, Value>>,
    reason: Option<String>,
}

/// O17: a new version of one group. `values` is the whole group; keys left out take the default.
pub async fn settings_put(mut ctx: Ctx, Path(group): Path<String>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("org.settings.edit")?;
    if !GROUPS.contains(&group.as_str()) {
        return Err(AppError::not_found().into());
    }
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("PUT /v1/org/settings/{group}"), &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let req: NewSettings = parse(raw)?;
    let reason = reason_of(req.reason);
    let mut errs = Vec::new();
    if req.valid_from.is_none() {
        errs.push(FieldError::new("valid_from", "required", "Choose the date the settings start"));
    }
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    let values = match check_group(&group, &req.values.unwrap_or_default()).expect("group checked above") {
        Ok(v) => v,
        Err(bad) => {
            errs.extend(bad.into_iter().map(|(k, m)| FieldError::new(format!("values.{k}"), "invalid_setting", m)));
            Value::Null
        }
    };
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let d = req.valid_from.unwrap();
    let mut old = in_force(&mut ctx.tx, &group, d).await?;
    old.as_object_mut().map(|o| o.remove("valid_from"));
    let row = sqlx::query(
        "INSERT INTO app.org_settings_versions (org_id, settings_group, valid_from, setting_values, reason)
         VALUES ($1, $2, $3, $4, $5) RETURNING id, recorded_at",
    )
    .bind(ctx.org_id)
    .bind(&group)
    .bind(d)
    .bind(&values)
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let id: Uuid = row.get("id");
    let (o, n) = changed(&old, &values, values.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default().into_iter());
    ctx.audit("org.settings.edit", "settings", id, Some(json!({ "group": group, "valid_from": d, "values": o })),
        Some(json!({ "group": group, "valid_from": d, "values": n })), reason.as_deref()).await?;
    let body = json!({ "id": id, "group": group, "valid_from": d, "values": values,
                       "recorded_at": row.get::<chrono::DateTime<chrono::Utc>, _>("recorded_at") });
    ctx.idempotency_finish(&headers, 201, &body).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// O18: the code the next employee would get. Reads the sequence, never takes a number (AC-EM-02.2).
pub async fn code_preview(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    ctx.scope("org.settings.edit")?;
    let (mut pattern, mut branch) = (None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "pattern" => pattern = Some(v),
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not a valid id"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let invalid = || AppError::unprocessable("invalid_pattern", "Pattern not usable", "This pattern can't produce employee codes");
    let pattern: CodePattern = match pattern {
        Some(p) => serde_json::from_str(&p).map_err(|_| invalid())?,
        None => em_settings(&mut ctx.tx, today()).await?.code_pattern(),
    };
    pattern.validate().map_err(|_| invalid())?;
    let uses_branch = pattern.parts.contains(&payroll_core::employee_code::CodePart::BranchShortCode);
    if uses_branch && branch.is_none() {
        return Err(AppError::validation(vec![FieldError::new("branch_id", "required", "This pattern uses the branch short code")]).into());
    }
    let row = sqlx::query("SELECT o.short_code AS org_code, (SELECT b.short_code FROM app.branches b WHERE b.id = $2) AS branch_code FROM app.organisations o WHERE o.id = $1")
        .bind(ctx.org_id)
        .bind(branch)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let branch_code: Option<String> = row.get("branch_code");
    if uses_branch && branch_code.is_none() {
        return Err(AppError::validation(vec![FieldError::new("branch_id", "invalid_value", "Not found")]).into());
    }
    let key = pattern.sequence_key(&branch.map(|b| b.to_string()).unwrap_or_default());
    let last: Option<i32> = sqlx::query_scalar("SELECT last_value FROM app.employee_code_sequences WHERE sequence_key = $1")
        .bind(&key)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let mut n = pattern.next_number(last.unwrap_or(0) as u32);
    let org_code: String = row.get("org_code");
    // Same skip as hiring: a number whose code is already taken is passed over (edge case 16).
    let code = loop {
        let code = pattern.format(&org_code, branch_code.as_deref().unwrap_or_default(), n);
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employees WHERE employee_code = $1)")
            .bind(&code)
            .fetch_one(&mut *ctx.tx)
            .await?;
        if !taken {
            break code;
        }
        n += 1;
    };
    ctx.commit().await?;
    Ok(Json(json!({ "next_code": code })))
}

// ---------- O19 custom fields ----------

const FIELD_TYPES: [&str; 6] = ["text", "number", "date", "yes_no", "dropdown", "multiple_choice"];

/// Options trimmed, none blank, no duplicates (ignoring case).
fn clean_options(opts: &[String], field: &str, errs: &mut Vec<FieldError>) -> Vec<String> {
    let out: Vec<String> = opts.iter().map(|o| o.trim().to_string()).collect();
    let mut seen: Vec<String> = out.iter().map(|o| o.to_lowercase()).collect();
    seen.sort();
    seen.dedup();
    if out.iter().any(String::is_empty) || seen.len() != out.len() {
        errs.push(FieldError::new(field, "invalid_value", "Options must be filled in and different"));
    }
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewField {
    label: Option<String>,
    field_type: Option<String>,
    options: Option<Vec<String>>,
    #[serde(default)]
    is_mandatory: bool,
}

pub async fn custom_fields(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    items(ctx, &CUSTOM_FIELD, raw).await
}

pub async fn custom_field_create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(CUSTOM_FIELD.manage)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/org/custom-fields", &raw).await? {
        return Ok(reply(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b));
    }
    let f: NewField = parse(raw)?;
    let mut errs = Vec::new();
    let label = f.label.as_deref().map(str::trim).filter(|l| !l.is_empty());
    if label.is_none() {
        errs.push(FieldError::new("label", "required", "Give the field a label"));
    }
    let kind = f.field_type.as_deref().unwrap_or_default();
    if !FIELD_TYPES.contains(&kind) {
        errs.push(FieldError::new("field_type", "invalid_value", FIELD_TYPES.join(", ")));
    }
    let choice = kind == "dropdown" || kind == "multiple_choice";
    let options = match (&f.options, choice) {
        (Some(o), true) if !o.is_empty() => Some(clean_options(o, "options", &mut errs)),
        (_, true) => {
            errs.push(FieldError::new("options", "options_required", "Dropdown and multiple-choice fields need options"));
            None
        }
        (Some(_), false) => {
            errs.push(FieldError::new("options", "invalid_value", "Only dropdown and multiple-choice fields have options"));
            None
        }
        (None, false) => None,
    };
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.custom_field_definitions (org_id, label, field_type, options, is_mandatory)
         VALUES ($1, $2, $3::app.custom_field_type, $4, $5) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(label)
    .bind(kind)
    .bind(options)
    .bind(f.is_mandatory)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let doc = item(&mut ctx.tx, CUSTOM_FIELD.table, id).await?;
    ctx.audit(CUSTOM_FIELD.manage, CUSTOM_FIELD.target, id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::CREATED, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldPatch {
    label: Option<String>,
    field_type: Option<String>,
    options: Option<Vec<String>>,
    is_mandatory: Option<bool>,
    inactive_options: Option<Vec<String>>,
}

/// Options in use are deactivated through inactive_options, never removed (AC-EM-04.2).
pub async fn custom_field_patch(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(CUSTOM_FIELD.manage)?;
    let expected = if_match(&headers)?;
    let p: FieldPatch = parse(raw.clone())?;
    let old = lock(&mut ctx.tx, CUSTOM_FIELD.table, id, expected).await?;
    let mut errs = Vec::new();
    let label = match p.label.as_deref().map(str::trim) {
        Some("") => {
            errs.push(FieldError::new("label", "required", "The label can't be blank"));
            None
        }
        l => l.map(str::to_string),
    };
    let old_kind = old["field_type"].as_str().unwrap_or_default().to_string();
    let kind = p.field_type.clone().unwrap_or_else(|| old_kind.clone());
    let retype = kind != old_kind;
    if !FIELD_TYPES.contains(&kind.as_str()) {
        errs.push(FieldError::new("field_type", "invalid_value", FIELD_TYPES.join(", ")));
    } else if retype {
        let has_values: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.custom_field_values WHERE field_id = $1)")
            .bind(id)
            .fetch_one(&mut *ctx.tx)
            .await?;
        if has_values {
            return Err(AppError::conflict("cannot_change_type_with_values", "Field has values", "A field that employees already have values for can't change type").into());
        }
    }
    let choice = kind == "dropdown" || kind == "multiple_choice";
    let current: Vec<String> = serde_json::from_value(old["options"].clone()).unwrap_or_default();
    let options = p.options.as_ref().map(|o| clean_options(o, "options", &mut errs));
    match &options {
        Some(_) if !choice => errs.push(FieldError::new("options", "invalid_value", "Only dropdown and multiple-choice fields have options")),
        Some(o) if o.is_empty() => errs.push(FieldError::new("options", "options_required", "Dropdown and multiple-choice fields need options")),
        // A field with no values (any retype) may drop options; otherwise they are deactivated instead.
        Some(o) if !retype && current.iter().any(|c| !o.contains(c)) => {
            errs.push(FieldError::new("options", "invalid_value", "Options can't be removed; deactivate them with inactive_options"))
        }
        None if choice && current.is_empty() => errs.push(FieldError::new("options", "options_required", "Dropdown and multiple-choice fields need options")),
        _ => {}
    }
    // Retyped to text, number, date or yes/no: the options go.
    let clear_options = retype && !choice;
    let all = if clear_options { Vec::new() } else { options.clone().unwrap_or(current) };
    if let Some(inactive) = &p.inactive_options {
        if inactive.iter().any(|i| !all.contains(i)) {
            errs.push(FieldError::new("inactive_options", "invalid_value", "Only listed options can be deactivated"));
        }
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    sqlx::query(
        "UPDATE app.custom_field_definitions SET label = coalesce($2, label), field_type = $6::app.custom_field_type,
           options = CASE WHEN $7 THEN NULL ELSE coalesce($3, options) END,
           inactive_options = CASE WHEN $7 THEN '{}' ELSE coalesce($5, inactive_options) END,
           is_mandatory = coalesce($4, is_mandatory), row_version = row_version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(label)
    .bind(options)
    .bind(p.is_mandatory)
    .bind(p.inactive_options)
    .bind(&kind)
    .bind(clear_options)
    .execute(&mut *ctx.tx)
    .await?;
    let new = item(&mut ctx.tx, CUSTOM_FIELD.table, id).await?;
    let keys = raw.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    let (o, n) = changed(&old, &new, keys.into_iter());
    ctx.audit(CUSTOM_FIELD.manage, CUSTOM_FIELD.target, id, Some(o), Some(n), None).await?;
    ctx.commit().await?;
    Ok(employee_response(StatusCode::OK, new))
}

/// A deactivated field is hidden from records; its values are kept (AC-EM-04.4).
pub async fn custom_field_deactivate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(ctx, &CUSTOM_FIELD, id, headers, raw, "inactive").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(freq: &str, month: Option<i16>, week: Option<i16>, anchor: Option<&str>) -> Group {
        Group { name: Some("G".into()), pay_frequency: Some(freq.into()), month_start_day: month, week_start_day: week, cycle_anchor: anchor.map(|a| a.parse().unwrap()) }
    }

    #[test]
    fn cycle_fields_match_the_frequency() {
        let codes = |x: Group| check_group_fields(&x).into_iter().map(|e| e.field).collect::<Vec<_>>();
        assert!(codes(g("monthly", None, None, None)).is_empty());
        assert!(codes(g("monthly", Some(26), None, None)).is_empty());
        assert_eq!(codes(g("monthly", Some(29), None, None)), ["month_start_day"]);
        assert_eq!(codes(g("weekly", None, None, None)), ["week_start_day"]);
        assert!(codes(g("weekly", None, Some(1), None)).is_empty());
        assert_eq!(codes(g("bi_weekly", None, Some(1), None)), ["cycle_anchor", "week_start_day"]);
        assert!(codes(g("bi_weekly", None, None, Some("2026-04-06"))).is_empty());
        assert_eq!(codes(g("daily", Some(1), None, None)), ["month_start_day"]);
        assert_eq!(codes(g("yearly", None, None, None)), ["pay_frequency"]);
    }
}
