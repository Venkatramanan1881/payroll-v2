//! Reports, exports and the audit viewer P1–P5, J6 (API_CONTRACTS §8, US-EM-22, AC-EM-21.2).
//! Rows always follow the caller's employee-view scope (AC-EM-22.3); exports are audited.

use crate::auth::{read_step, step_token};
use crate::ctx::Ctx;
use crate::employees::{as_of_param, bad_param, check_params, em_settings, push_filtered, push_from, push_scope, query_pairs, today, Access};
use crate::error::{parse, ApiResult, Body};
use crate::imports::{xlsx, xlsx_response};
use crate::AppState;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

/// Above this many rows an export is built in the background (P3).
const SYNC_ROWS: usize = 2000;
const LINK_SECONDS: u64 = 10 * 60;
const KEEP_DAYS: i32 = 30;
const STALE_MINUTES: i32 = 10;

/// R10 grouping: (group_by value, job column, table, code column).
const GROUPS: [(&str, &str, &str, &str); 4] = [
    ("branch", "master_branch_id", "branches", "short_code"),
    ("department", "department_id", "departments", "code"),
    ("category", "category_id", "categories", "code"),
    ("employee_type", "employee_type_id", "employee_types", "code"),
];

/// Headcount on `as_of`: joined by then and not exited, unless the caller filters by status.
fn push_headcount(qb: &mut QueryBuilder<'static, sqlx::Postgres>, pairs: &[(String, String)]) {
    if !pairs.iter().any(|(k, _)| k == "status") {
        qb.push(" AND st.status IS NOT NULL AND st.status <> 'exited'");
    }
}

// ---------- P1 R10 ----------

async fn r10_data(ctx: &mut Ctx, pairs: &[(String, String)]) -> ApiResult<Value> {
    ctx.scope("report.r10")?;
    let scope = ctx.scope("employee.view")?;
    check_params(pairs, &["group_by"])?;
    let as_of = as_of_param(pairs)?;
    let by = pairs.iter().find(|(k, _)| k == "group_by").map(|(_, v)| v.as_str()).unwrap_or("branch");
    let (_, col, table, code) = GROUPS.iter().find(|g| g.0 == by).ok_or_else(|| bad_param("group_by", "branch, department, category or employee_type"))?;
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(format!("SELECT jsonb_build_object('group', jsonb_build_object('id', g.id, 'code', g.{code}, 'name', g.name), 'count', c.n)
        FROM (SELECT jv.{col} AS gid, count(*) AS n"));
    push_filtered(&mut qb, ctx, &scope, &em, as_of, pairs)?;
    push_headcount(&mut qb, pairs);
    qb.push(format!(" GROUP BY 1) c LEFT JOIN app.{table} g ON g.id = c.gid ORDER BY g.{code}"));
    let rows: Vec<Value> = qb.build_query_scalar().fetch_all(&mut *ctx.tx).await?;
    let total: i64 = rows.iter().filter_map(|r| r["count"].as_i64()).sum();
    Ok(json!({ "as_of": as_of, "group_by": by, "rows": rows, "total": total }))
}

/// P1. The employee list itself is E1 with the same filters.
pub async fn r10(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let body = r10_data(&mut ctx, &query_pairs(raw)?).await?;
    ctx.commit().await?;
    Ok(Json(body))
}

// ---------- P2 R11 ----------

fn date_param(pairs: &[(String, String)], name: &str) -> ApiResult<NaiveDate> {
    let v = pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v).ok_or_else(|| bad_param(name, "Required, YYYY-MM-DD"))?;
    Ok(v.parse().map_err(|_| bad_param(name, "Use YYYY-MM-DD"))?)
}

// shortcut: scope is judged on each employee's job on `to` (or their last job before leaving), not on the movement date.
async fn r11_data(ctx: &mut Ctx, pairs: &[(String, String)]) -> ApiResult<Value> {
    ctx.scope("report.r11")?;
    let scope = ctx.scope("employee.view")?;
    if let Some((k, _)) = pairs.iter().find(|(k, _)| k != "from" && k != "to") {
        return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into());
    }
    let (from, to) = (date_param(pairs, "from")?, date_param(pairs, "to")?);
    if from > to {
        return Err(bad_param("to", "Must be on or after from").into());
    }
    let em = em_settings(&mut ctx.tx, to).await?;
    let mut out = Map::new();
    // A rejoin opens a new period, so it counts as a joiner (AC-EM-22.2).
    for (key, date_col) in [("joiners", "joining_date"), ("leavers", "last_working_day")] {
        let mut qb = QueryBuilder::new(
            "SELECT jsonb_build_object('employee_id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name,
               'master_branch', (SELECT jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name) FROM app.branches b WHERE b.id = jv.master_branch_id),
               'department', (SELECT jsonb_build_object('id', d.id, 'code', d.code, 'name', d.name) FROM app.departments d WHERE d.id = jv.department_id),
               'joining_date', p.joining_date, 'last_working_day', p.last_working_day,
               'exit_reason', (SELECT jsonb_build_object('id', x.id, 'code', x.code, 'name', x.name) FROM app.exit_reasons x WHERE x.id = p.exit_reason_id))",
        );
        push_from(&mut qb, to);
        qb.push(" JOIN app.employment_periods p ON p.employee_id = e.id WHERE ");
        push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, to);
        qb.push(format!(" AND p.{date_col} BETWEEN ")).push_bind(from).push(" AND ").push_bind(to);
        qb.push(format!(" ORDER BY p.{date_col}, e.employee_code"));
        let rows: Vec<Value> = qb.build_query_scalar().fetch_all(&mut *ctx.tx).await?;
        out.insert(key.into(), Value::Array(rows));
    }
    out.insert("from".into(), json!(from));
    out.insert("to".into(), json!(to));
    Ok(Value::Object(out))
}

pub async fn r11(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let body = r11_data(&mut ctx, &query_pairs(raw)?).await?;
    ctx.commit().await?;
    Ok(Json(body))
}

// ---------- spreadsheet content ----------

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

async fn employees_sheet(ctx: &mut Ctx, pairs: &[(String, String)]) -> ApiResult<Vec<Vec<String>>> {
    let scope = ctx.scope("employee.view")?;
    check_params(pairs, &[])?;
    let as_of = as_of_param(pairs)?;
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(
        "SELECT ARRAY[e.employee_code, e.full_name, e.mobile, st.status,
            (SELECT b.short_code FROM app.branches b WHERE b.id = jv.master_branch_id),
            (SELECT d.name FROM app.departments d WHERE d.id = jv.department_id),
            (SELECT d.name FROM app.designations d WHERE d.id = jv.designation_id),
            (SELECT d.name FROM app.categories d WHERE d.id = jv.category_id),
            (SELECT d.name FROM app.employee_types d WHERE d.id = jv.employee_type_id),
            (SELECT g.name FROM app.payroll_groups g WHERE g.id = jv.payroll_group_id),
            (SELECT max(p.joining_date)::text FROM app.employment_periods p WHERE p.employee_id = e.id AND p.joining_date <= ",
    );
    qb.push_bind(as_of).push("::date)]::text[]");
    push_filtered(&mut qb, ctx, &scope, &em, as_of, pairs)?;
    qb.push(" ORDER BY e.employee_code");
    let rows: Vec<Vec<Option<String>>> = qb.build_query_scalar().fetch_all(&mut *ctx.tx).await?;
    let mut out = vec![["employee_code", "full_name", "mobile", "status", "master_branch", "department", "designation", "category", "employee_type", "payroll_group", "joining_date"]
        .map(String::from)
        .to_vec()];
    out.extend(rows.into_iter().map(|r| r.into_iter().map(Option::unwrap_or_default).collect()));
    Ok(out)
}

/// The report as rows, header first. Checks the report's own permission (P3: "+ the report's permission").
async fn sheet(ctx: &mut Ctx, report: &str, pairs: &[(String, String)]) -> ApiResult<Vec<Vec<String>>> {
    match report {
        "r10" => {
            let d = r10_data(ctx, pairs).await?;
            let mut out = vec![vec!["code".into(), "name".into(), "count".into()]];
            for r in d["rows"].as_array().into_iter().flatten() {
                out.push(vec![text(&r["group"]["code"]), text(&r["group"]["name"]), text(&r["count"])]);
            }
            out.push(vec!["".into(), "Total".into(), text(&d["total"])]);
            Ok(out)
        }
        "r11" => {
            let d = r11_data(ctx, pairs).await?;
            let mut out = vec![["movement", "employee_code", "full_name", "master_branch", "department", "date", "exit_reason"].map(String::from).to_vec()];
            for (kind, date) in [("joiners", "joining_date"), ("leavers", "last_working_day")] {
                for r in d[kind].as_array().into_iter().flatten() {
                    out.push(vec![
                        kind.trim_end_matches('s').into(),
                        text(&r["employee_code"]),
                        text(&r["full_name"]),
                        text(&r["master_branch"]["short_code"]),
                        text(&r["department"]["name"]),
                        text(&r[date]),
                        text(&r["exit_reason"]["name"]),
                    ]);
                }
            }
            Ok(out)
        }
        "employees" => employees_sheet(ctx, pairs).await,
        code => crate::at_reports::sheet(ctx, code, pairs).await,
    }
}

/// `parameters` as query pairs: a string is one value, an array of strings several.
fn pairs_of(params: &Map<String, Value>) -> ApiResult<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (k, v) in params {
        match v {
            Value::String(s) => out.push((k.clone(), s.clone())),
            Value::Array(a) if a.iter().all(Value::is_string) => out.extend(a.iter().map(|s| (k.clone(), text(s)))),
            _ => return Err(AppError::validation(vec![FieldError::new(format!("parameters.{k}"), "invalid_value", "Use text or a list of text")]).into()),
        }
    }
    Ok(out)
}

// ---------- P3 / P4 exports, J6 ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewExport {
    report_code: String,
    #[serde(default)]
    parameters: Map<String, Value>,
}

const REPORTS: [&str; 3] = ["r10", "r11", "employees"];

// shortcut: export files sit on local disk under FILE_DIR like imports, until Cloud Storage is set up.
async fn write_file(state: &AppState, org: Uuid, id: Uuid, rows: &[Vec<String>]) -> ApiResult<String> {
    let bytes = xlsx(rows).map_err(|_| AppError::internal())?;
    let file_ref = format!("{org}/exports/{}/{id}.xlsx", chrono::Utc::now().format("%Y-%m"));
    let path = state.files_dir.join(&file_ref);
    tokio::fs::create_dir_all(path.parent().unwrap()).await.map_err(|_| AppError::internal())?;
    tokio::fs::write(&path, bytes).await.map_err(|_| AppError::internal())?;
    Ok(file_ref)
}

async fn mark_ready(tx: &mut crate::employees::Tx, id: Uuid, file_ref: &str) -> ApiResult<()> {
    sqlx::query(
        "UPDATE app.export_jobs SET state = 'ready', file_ref = $2, completed_at = now(), expires_at = now() + make_interval(days => $3) WHERE id = $1",
    )
    .bind(id)
    .bind(file_ref)
    .bind(KEEP_DAYS)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn export_doc(state: &AppState, ctx: &mut Ctx, id: Uuid) -> ApiResult<Value> {
    let row = sqlx::query("SELECT state::text AS state, expires_at, error FROM app.export_jobs WHERE id = $1 AND requested_by = $2")
        .bind(id)
        .bind(ctx.user_id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let st: String = row.get("state");
    let url = (st == "ready").then(|| format!("/v1/files/exports/{}", step_token(state, id, ctx.org_id, "export", LINK_SECONDS)));
    Ok(json!({ "export_id": id, "state": st, "download_url": url, "expires_at": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at"), "error": row.get::<Option<String>, _>("error") }))
}

/// P3. Up to 2,000 rows: built now, 200 with a link. Larger: 202, built in the background (J6).
pub async fn create_export(State(state): State<AppState>, mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("report.export")?;
    if let Some((status, body)) = ctx.idempotency_begin(&headers, "POST /v1/exports", &raw).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let req: NewExport = parse(raw)?;
    // shortcut: payroll_input arrives with periods and lock (S9, on hold).
    if !REPORTS.contains(&req.report_code.as_str()) && !crate::at_reports::CODES.contains(&req.report_code.as_str()) {
        return Err(AppError::validation(vec![FieldError::new("report_code", "invalid_value", "r10, r11, employees, r1–r5, r23 or r26")]).into());
    }
    let pairs = pairs_of(&req.parameters)?;
    let rows = sheet(&mut ctx, &req.report_code, &pairs).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO app.export_jobs (id, org_id, requested_by, report_code, parameters) VALUES ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(ctx.user_id)
        .bind(&req.report_code)
        .bind(Value::Object(req.parameters.clone()))
        .execute(&mut *ctx.tx)
        .await?;
    let data_rows = rows.len() - 1;
    ctx.audit("report.export", "export", id, None, Some(json!({ "report_code": req.report_code, "parameters": req.parameters, "rows": data_rows })), None).await?;
    let background = data_rows > SYNC_ROWS;
    let status = if background {
        StatusCode::ACCEPTED
    } else {
        let file_ref = write_file(&state, ctx.org_id, id, &rows).await?;
        mark_ready(&mut ctx.tx, id, &file_ref).await?;
        StatusCode::OK
    };
    let body = export_doc(&state, &mut ctx, id).await?;
    ctx.idempotency_finish(&headers, status.as_u16(), &body).await?;
    let org = ctx.org_id;
    ctx.commit().await?;
    if background {
        // shortcut: built in this server process; Cloud Tasks should call J6 once it is set up.
        tokio::spawn(async move {
            if let Err(e) = process_export(&state, org, id).await {
                tracing::error!(export = %id, error = ?e.0, "export failed to run");
            }
        });
    }
    Ok((status, Json(body)).into_response())
}

/// P4: only the person who asked for it.
pub async fn get_export(State(state): State<AppState>, mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let doc = export_doc(&state, &mut ctx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc))
}

/// The download link: the token is the permission, valid 10 minutes, and only while the file is kept.
pub async fn export_file(State(state): State<AppState>, Path(token): Path<String>) -> ApiResult<Response> {
    let gone = || AppError::new(410, "link_expired", "Link expired", "Ask for a new link");
    let claims = read_step(&state, &token, "export").ok_or_else(gone)?;
    let mut tx = db::begin_tenant(&state.pool, claims.org, None).await?;
    let row = sqlx::query("SELECT file_ref, report_code FROM app.export_jobs WHERE id = $1 AND state = 'ready'")
        .bind(claims.sub)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(gone)?;
    tx.commit().await?;
    let bytes = tokio::fs::read(state.files_dir.join(row.get::<String, _>("file_ref"))).await.map_err(|_| gone())?;
    Ok(xlsx_response(bytes, &format!("{}.xlsx", row.get::<String, _>("report_code"))))
}

/// J6: build a queued export as the person who asked, with their permissions now. Safe to run twice.
pub async fn process_export(state: &AppState, org: Uuid, id: Uuid) -> ApiResult<Value> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    let claimed = sqlx::query(
        "UPDATE app.export_jobs SET state = 'running'
         WHERE id = $1 AND (state = 'queued' OR (state = 'running' AND updated_at < now() - make_interval(mins => $2)))
         RETURNING requested_by, report_code, parameters",
    )
    .bind(id)
    .bind(STALE_MINUTES)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let Some(job) = claimed else {
        return Ok(json!({ "note": "already finished or running" }));
    };
    let fail = |reason: String| async move {
        let mut tx = db::begin_tenant(&state.pool, org, None).await?;
        sqlx::query("UPDATE app.export_jobs SET state = 'failed', error = $2, completed_at = now() WHERE id = $1").bind(id).bind(&reason).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok::<Value, crate::error::ApiError>(json!({ "state": "failed", "error": reason }))
    };
    let mut ctx = match Ctx::for_user(&state.pool, org, job.get("requested_by")).await {
        Ok(c) => c,
        Err(_) => return fail("The requester can no longer use reports".into()).await,
    };
    let params: Map<String, Value> = job.get::<Value, _>("parameters").as_object().cloned().unwrap_or_default();
    let rows = match async { ctx.scope("report.export")?; sheet(&mut ctx, &job.get::<String, _>("report_code"), &pairs_of(&params)?).await }.await {
        Ok(r) => r,
        Err(e) => return fail(format!("{}: {}", e.0.code, e.0.detail)).await,
    };
    let file_ref = write_file(state, org, id, &rows).await?;
    mark_ready(&mut ctx.tx, id, &file_ref).await?;
    ctx.commit().await?;
    Ok(json!({ "state": "ready", "rows": rows.len() - 1 }))
}

// ---------- P5 audit ----------

const AUDIT_PARAMS: [&str; 8] = ["target_type", "target_id", "actor_id", "from", "to", "action", "limit", "before"];

/// P5, newest first. Entries about employees show only for employees the caller may view.
// shortcut: from/to are UTC calendar days.
pub async fn audit(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    ctx.scope("audit.view")?;
    let pairs = query_pairs(raw)?;
    if let Some((k, _)) = pairs.iter().find(|(k, _)| !AUDIT_PARAMS.contains(&k.as_str())) {
        return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into());
    }
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let uuid = |k: &str| get(k).map(|v| v.parse::<Uuid>().map_err(|_| bad_param(k, "Not a valid id"))).transpose();
    let day = |k: &str| get(k).map(|v| v.parse::<NaiveDate>().map_err(|_| bad_param(k, "Use YYYY-MM-DD"))).transpose();
    let limit: i64 = match get("limit") {
        Some(l) => l.parse().ok().filter(|n| (1..=200).contains(n)).ok_or_else(|| bad_param("limit", "1 to 200"))?,
        None => 100,
    };
    let before: Option<i64> = get("before").map(|v| v.parse().map_err(|_| bad_param("before", "Use next_before from the last page"))).transpose()?;

    let mut qb = QueryBuilder::new(
        "SELECT a.id, jsonb_build_object('id', a.id, 'occurred_at', a.occurred_at, 'actor_kind', a.actor_kind, 'actor_id', a.actor_id,
            'actor_name', (SELECT u.display_name FROM app.users u WHERE u.id = a.actor_id),
            'action', a.action, 'target_type', a.target_type, 'target_id', a.target_id,
            'old_values', a.old_values, 'new_values', a.new_values, 'reason', a.reason, 'correlation_id', a.correlation_id) AS item
         FROM app.audit_log a WHERE (a.target_type <> 'employee'",
    );
    if let Ok(view) = ctx.scope("employee.view") {
        let as_of = today();
        let em = em_settings(&mut ctx.tx, as_of).await?;
        qb.push(" OR a.target_id IN (SELECT e.id");
        push_from(&mut qb, as_of);
        qb.push(" WHERE ");
        push_scope(&mut qb, &view, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
        qb.push(")");
    }
    qb.push(")");
    if let Some(v) = get("target_type") {
        qb.push(" AND a.target_type = ").push_bind(v);
    }
    if let Some(v) = get("action") {
        qb.push(" AND a.action = ").push_bind(v);
    }
    if let Some(v) = uuid("target_id")? {
        qb.push(" AND a.target_id = ").push_bind(v);
    }
    if let Some(v) = uuid("actor_id")? {
        qb.push(" AND a.actor_id = ").push_bind(v);
    }
    if let Some(d) = day("from")? {
        qb.push(" AND a.occurred_at >= ").push_bind(d).push("::date");
    }
    if let Some(d) = day("to")? {
        qb.push(" AND a.occurred_at < ").push_bind(d).push("::date + 1");
    }
    if let Some(b) = before {
        qb.push(" AND a.id < ").push_bind(b);
    }
    qb.push(" ORDER BY a.id DESC LIMIT ").push_bind(limit + 1);
    let rows = qb.build().fetch_all(&mut *ctx.tx).await?;
    ctx.commit().await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<Value> = rows.iter().take(limit as usize).map(|r| r.get("item")).collect();
    let next_before = more.then(|| rows[limit as usize - 1].get::<i64, _>("id"));
    Ok(Json(json!({ "items": items, "next_before": next_before })))
}
