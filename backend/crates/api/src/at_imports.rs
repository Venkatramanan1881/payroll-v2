//! Attendance imports: P10 punches and R6 roster, processed by J-AT6 (API_CONTRACTS Package 2 §3 R6, §5.5 P10,
//! US-AT-02, US-AT-17). Same flow as the employee imports (B1–B5, J5): the file is stored, then each row is
//! applied in its own transaction as the uploader, and every row's outcome is kept.

use crate::auth::{read_step, step_token};
use crate::ctx::Ctx;
use crate::employees::{allowed, em_settings, today, Access};
use crate::error::{parse, ApiResult, Body};
use crate::imports::{data_rows, read_sheet, too_large, unreadable, xlsx, xlsx_response, ERRORS_LINK_SECONDS, MAX_BYTES, MAX_ROWS, STALE_MINUTES};
use crate::AppState;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use payroll_core::settings::EmSettings;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shared::{AppError, FieldError};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const PUNCH_COLS: [&str; 8] = ["external_id", "punch_time", "employee_code", "enrolment_id", "device_serial", "direction", "latitude", "longitude"];
const ROSTER_COLS: [&str; 4] = ["employee_code", "work_date", "shift_code", "weekly_off"];
const PUNCH_IMPORT: &str = "attendance.punch.import";
const ROSTER_MANAGE: &str = "attendance.roster.manage";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Punch,
    Roster,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Punch => "punch",
            Kind::Roster => "roster",
        }
    }
    fn permission(self) -> &'static str {
        match self {
            Kind::Punch => PUNCH_IMPORT,
            Kind::Roster => ROSTER_MANAGE,
        }
    }
    fn columns(self) -> &'static [&'static str] {
        match self {
            Kind::Punch => &PUNCH_COLS,
            Kind::Roster => &ROSTER_COLS,
        }
    }
    fn required(self) -> &'static [&'static str] {
        match self {
            Kind::Punch => &["external_id", "punch_time"],
            Kind::Roster => &["employee_code", "work_date"],
        }
    }
}

/// Column positions by name. Unknown, repeated or missing required columns refuse the file.
fn columns(head: &[String], kind: Kind) -> Result<HashMap<&'static str, usize>, AppError> {
    let wrong = |m: String| AppError::unprocessable("wrong_template", "Columns don't match the template", m);
    let mut cols = HashMap::new();
    for (i, h) in head.iter().enumerate() {
        let key = h.trim().trim_end_matches('*').trim().to_lowercase().replace(' ', "_");
        if key.is_empty() {
            continue;
        }
        let Some(name) = kind.columns().iter().find(|c| **c == key) else {
            return Err(wrong(format!("Unknown column \"{}\"", h.trim())));
        };
        if cols.insert(*name, i).is_some() {
            return Err(wrong(format!("Column \"{name}\" appears twice")));
        }
    }
    if let Some(m) = kind.required().iter().find(|c| !cols.contains_key(*c)) {
        return Err(wrong(format!("The {m} column is missing")));
    }
    Ok(cols)
}

// ---------- upload (P10, R6) ----------

async fn file_field(mut form: Multipart) -> ApiResult<axum::body::Bytes> {
    while let Some(field) = form.next_field().await.map_err(|_| unreadable("The upload could not be read"))? {
        if field.name() == Some("file") {
            return Ok(field.bytes().await.map_err(|e| match e.status() {
                StatusCode::PAYLOAD_TOO_LARGE => too_large("The file is over 5 MB"),
                _ => unreadable("The upload could not be read"),
            })?);
        }
    }
    Err(AppError::validation(vec![FieldError::new("file", "required", "Attach the file")]).into())
}

// shortcut: files are kept on local disk under FILE_DIR like the employee imports (TSD §11 Cloud Storage at deployment).
async fn store(state: AppState, mut ctx: Ctx, headers: &HeaderMap, kind: Kind, bytes: &[u8], route: &str) -> ApiResult<Response> {
    ctx.scope(kind.permission())?;
    if bytes.len() > MAX_BYTES {
        return Err(too_large("The file is over 5 MB").into());
    }
    let checksum = format!("{:x}", Sha256::digest(bytes));
    if let Some((status, body)) = ctx.idempotency_begin(headers, route, &json!({ "checksum": checksum })).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let rows = read_sheet(bytes)?;
    columns(rows.first().ok_or_else(|| unreadable("The file is empty"))?, kind)?;
    let total = data_rows(&rows).len();
    if total > MAX_ROWS {
        return Err(too_large("The file has more than 5,000 rows").into());
    }
    let id = Uuid::new_v4();
    let ext = if bytes.starts_with(b"PK") { "xlsx" } else { "csv" };
    let file_ref = format!("{}/attendance-imports/{}/{id}.{ext}", ctx.org_id, Utc::now().format("%Y-%m"));
    let path = state.files_dir.join(&file_ref);
    tokio::fs::create_dir_all(path.parent().unwrap()).await.map_err(|_| AppError::internal())?;
    tokio::fs::write(&path, bytes).await.map_err(|_| AppError::internal())?;
    sqlx::query(
        "INSERT INTO app.attendance_imports (id, org_id, kind, file_ref, file_checksum, uploaded_by, total_rows)
         VALUES ($1, $2, $3::app.attendance_import_kind, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(kind.name())
    .bind(&file_ref)
    .bind(&checksum)
    .bind(ctx.user_id)
    .bind(total as i32)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.audit(kind.permission(), "attendance_import", id, None, Some(json!({ "kind": kind.name(), "checksum": checksum, "total_rows": total })), None).await?;
    let body = json!({ "import_id": id, "state": "uploaded" });
    ctx.idempotency_finish(headers, 202, &body).await?;
    let org = ctx.org_id;
    ctx.commit().await?;
    // shortcut: processed in this server process right after upload; once Cloud Tasks is set up it should call J-AT6 instead.
    tokio::spawn(async move {
        if let Err(e) = process(&state, org, id).await {
            tracing::error!(import = %id, error = ?e.0, "attendance import failed to run");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

pub async fn punch_upload(State(state): State<AppState>, ctx: Ctx, headers: HeaderMap, form: Multipart) -> ApiResult<Response> {
    let bytes = file_field(form).await?;
    store(state, ctx, &headers, Kind::Punch, &bytes, "POST /v1/attendance/punch-imports").await
}

/// The JSON form is turned into the same CSV and stored like an upload, so one path processes both.
// shortcut: file_ref is set for JSON imports too (the schema comment says NULL); keeps J-AT6 single-path.
pub async fn punch_json(State(state): State<AppState>, ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Req {
        rows: Vec<Map<String, Value>>,
    }
    let req: Req = parse(raw)?;
    if req.rows.len() > MAX_ROWS {
        return Err(too_large("More than 5,000 rows").into());
    }
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(PUNCH_COLS).map_err(|_| AppError::internal())?;
    for (i, r) in req.rows.iter().enumerate() {
        if let Some(k) = r.keys().find(|k| !PUNCH_COLS.contains(&k.as_str())) {
            return Err(AppError::validation(vec![FieldError::new(format!("rows[{i}].{k}"), "invalid_value", "Unknown field")]).into());
        }
        let cell = |c: &str| match r.get(c) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
        };
        w.write_record(PUNCH_COLS.map(cell)).map_err(|_| AppError::internal())?;
    }
    let bytes = w.into_inner().map_err(|_| AppError::internal())?;
    store(state, ctx, &headers, Kind::Punch, &bytes, "POST /v1/attendance/punch-imports/json").await
}

pub async fn roster_upload(State(state): State<AppState>, ctx: Ctx, headers: HeaderMap, form: Multipart) -> ApiResult<Response> {
    let bytes = file_field(form).await?;
    store(state, ctx, &headers, Kind::Roster, &bytes, "POST /v1/attendance/roster-imports").await
}

pub async fn roster_template(ctx: Ctx) -> ApiResult<Response> {
    ctx.scope(ROSTER_MANAGE)?;
    ctx.commit().await?;
    let rows = vec![
        vec!["employee_code*".into(), "work_date*".into(), "shift_code".into(), "weekly_off".into()],
        vec!["E001".into(), "2026-12-01".into(), "GEN".into(), String::new()],
        vec!["E001".into(), "2026-12-07".into(), String::new(), "yes".into()],
    ];
    Ok(xlsx_response(xlsx(&rows).map_err(|_| AppError::internal())?, "roster-import-template.xlsx"))
}

// ---------- list, get, errors ----------

const IMPORT: &str = "SELECT jsonb_build_object('id', i.id, 'kind', i.kind, 'state', i.state, 'total_rows', i.total_rows,
        'applied_count', i.applied_count, 'error_count', i.error_count,
        'skipped_count', (SELECT count(*) FROM app.attendance_import_rows r WHERE r.import_id = i.id AND r.outcome = 'skipped'),
        'failure_reason', i.failure_reason, 'uploaded_by', i.uploaded_by, 'created_at', i.created_at, 'completed_at', i.completed_at)
    FROM app.attendance_imports i WHERE i.kind = $1::app.attendance_import_kind";

async fn one(mut ctx: Ctx, kind: Kind, id: Uuid) -> ApiResult<Json<Value>> {
    ctx.scope(kind.permission())?;
    let doc: Value = sqlx::query_scalar(&format!("{IMPORT} AND i.id = $2"))
        .bind(kind.name())
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    ctx.commit().await?;
    Ok(Json(doc))
}

// shortcut: the latest 100, no paging yet (as the employee imports).
async fn many(mut ctx: Ctx, kind: Kind) -> ApiResult<Json<Value>> {
    ctx.scope(kind.permission())?;
    let items: Vec<Value> = sqlx::query_scalar(&format!("{IMPORT} ORDER BY i.created_at DESC LIMIT 100")).bind(kind.name()).fetch_all(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

async fn error_link(state: AppState, mut ctx: Ctx, kind: Kind, id: Uuid) -> ApiResult<Json<Value>> {
    ctx.scope(kind.permission())?;
    let expired: bool = sqlx::query_scalar(
        "SELECT coalesce(completed_at < now() - interval '30 days', false) FROM app.attendance_imports WHERE id = $1 AND kind = $2::app.attendance_import_kind",
    )
    .bind(id)
    .bind(kind.name())
    .fetch_optional(&mut *ctx.tx)
    .await?
    .ok_or_else(AppError::not_found)?;
    if expired {
        return Err(AppError::new(410, "link_expired", "Report expired", "Error reports are kept for 30 days").into());
    }
    let token = step_token(&state, id, ctx.org_id, "attendance_import_errors", ERRORS_LINK_SECONDS);
    ctx.commit().await?;
    let expires_at = Utc::now() + chrono::Duration::seconds(ERRORS_LINK_SECONDS as i64);
    Ok(Json(json!({ "url": format!("/v1/files/attendance-import-errors/{token}"), "expires_at": expires_at })))
}

pub async fn punch_list(ctx: Ctx) -> ApiResult<Json<Value>> {
    many(ctx, Kind::Punch).await
}
pub async fn punch_get(ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    one(ctx, Kind::Punch, id).await
}
pub async fn punch_errors(State(state): State<AppState>, ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    error_link(state, ctx, Kind::Punch, id).await
}
pub async fn roster_list(ctx: Ctx) -> ApiResult<Json<Value>> {
    many(ctx, Kind::Roster).await
}
pub async fn roster_get(ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    one(ctx, Kind::Roster, id).await
}
pub async fn roster_errors(State(state): State<AppState>, ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    error_link(state, ctx, Kind::Roster, id).await
}

/// The link itself: the token is the permission, as for the employee import report.
pub async fn errors_file(State(state): State<AppState>, Path(token): Path<String>) -> ApiResult<Response> {
    let claims = read_step(&state, &token, "attendance_import_errors").ok_or_else(|| AppError::new(410, "link_expired", "Link expired", "Ask for a new link"))?;
    let mut tx = db::begin_tenant(&state.pool, claims.org, None).await?;
    let found = sqlx::query(
        "SELECT r.row_number, e.employee_code, r.error_message FROM app.attendance_import_rows r
         LEFT JOIN app.employees e ON e.id = r.employee_id WHERE r.import_id = $1 AND r.outcome = 'error' ORDER BY r.row_number",
    )
    .bind(claims.sub)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut rows = vec![["row", "employee_code", "field", "code", "message"].map(String::from).to_vec()];
    for r in found {
        let e: Value = serde_json::from_str(r.get("error_message")).unwrap_or_default();
        let s = |k: &str| e[k].as_str().unwrap_or_default().to_string();
        rows.push(vec![r.get::<i32, _>("row_number").to_string(), r.get::<Option<String>, _>("employee_code").unwrap_or_default(), s("field"), s("code"), s("message")]);
    }
    Ok(xlsx_response(xlsx(&rows).map_err(|_| AppError::internal())?, "attendance-import-errors.xlsx"))
}

// ---------- J-AT6 processing ----------

async fn set_state(state: &AppState, org: Uuid, id: Uuid, applied: i32, errors: i32, done: bool, failed: Option<&str>) -> ApiResult<()> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    sqlx::query(
        "UPDATE app.attendance_imports SET applied_count = $2, error_count = $3, failure_reason = $5,
           state = CASE WHEN $5 IS NOT NULL THEN 'failed' WHEN $4 THEN 'completed' ELSE state END,
           completed_at = CASE WHEN $4 OR $5 IS NOT NULL THEN now() END
         WHERE id = $1",
    )
    .bind(id)
    .bind(applied)
    .bind(errors)
    .bind(done)
    .bind(failed)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn fail(state: &AppState, org: Uuid, id: Uuid, reason: &str) -> ApiResult<Value> {
    set_state(state, org, id, 0, 0, false, Some(reason)).await?;
    Ok(json!({ "state": "failed", "failure_reason": reason }))
}

/// A rejected row: the employee if known, and {field, code, message}.
type RowErr = (Option<Uuid>, Value);

fn row_err(emp: Option<Uuid>, field: &str, code: &str, message: &str) -> RowErr {
    (emp, json!({ "field": field, "code": code, "message": message }))
}

/// J-AT6: validate and apply every row. Safe to run twice: a run takes the import over only if it is new or
/// its last run went quiet, and rows already recorded are not redone. The same file uploaded again skips
/// rows already applied (AC-AT-02.3); for punches a repeated external_id is ignored too (AC-AT-17.2).
pub async fn process(state: &AppState, org: Uuid, id: Uuid) -> ApiResult<Value> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    let claimed = sqlx::query(
        "UPDATE app.attendance_imports SET state = 'validating'
         WHERE id = $1 AND (state = 'uploaded' OR (state IN ('validating', 'applying') AND updated_at < now() - make_interval(mins => $2)))
         RETURNING kind::text AS kind, file_ref, file_checksum, uploaded_by",
    )
    .bind(id)
    .bind(STALE_MINUTES)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(claimed) = claimed else {
        let st: Option<String> = sqlx::query_scalar("SELECT state::text FROM app.attendance_imports WHERE id = $1").bind(id).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        return match st {
            Some(st) => Ok(json!({ "state": st, "note": "already finished or running" })),
            None => Err(AppError::not_found().into()),
        };
    };
    tx.commit().await?;
    let kind = if claimed.get::<String, _>("kind") == "punch" { Kind::Punch } else { Kind::Roster };
    let (file_ref, checksum, uploader): (Option<String>, String, Uuid) = (claimed.get("file_ref"), claimed.get("file_checksum"), claimed.get("uploaded_by"));
    let Some(Ok(bytes)) = (match &file_ref {
        Some(f) => Some(tokio::fs::read(state.files_dir.join(f)).await),
        None => None,
    }) else {
        return fail(state, org, id, "file_unreadable: the uploaded file is no longer available").await;
    };
    let rows = match read_sheet(&bytes) {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return fail(state, org, id, "file_unreadable: the file is empty").await,
        Err(e) => return fail(state, org, id, &format!("{}: {}", e.code, e.detail)).await,
    };
    let cols = match columns(&rows[0], kind) {
        Ok(c) => c,
        Err(e) => return fail(state, org, id, &format!("{}: {}", e.code, e.detail)).await,
    };
    let mut c = match Ctx::for_user(&state.pool, org, uploader).await {
        Ok(c) => c,
        Err(_) => return fail(state, org, id, "uploader_not_active: the uploader can no longer make changes, or the organisation is suspended").await,
    };
    let em = em_settings(&mut c.tx, today()).await?;
    let done: HashMap<i32, String> = sqlx::query("SELECT row_number, outcome::text AS outcome FROM app.attendance_import_rows WHERE import_id = $1")
        .bind(id)
        .fetch_all(&mut *c.tx)
        .await?
        .iter()
        .map(|r| (r.get("row_number"), r.get("outcome")))
        .collect();
    let applied_before: HashSet<i32> = sqlx::query_scalar(
        "SELECT r.row_number FROM app.attendance_import_rows r JOIN app.attendance_imports i ON i.id = r.import_id
         WHERE r.file_checksum = $1 AND r.outcome = 'applied' AND r.import_id <> $2 AND i.kind = $3::app.attendance_import_kind",
    )
    .bind(&checksum)
    .bind(id)
    .bind(kind.name())
    .fetch_all(&mut *c.tx)
    .await?
    .into_iter()
    .collect();
    sqlx::query("UPDATE app.attendance_imports SET state = 'applying' WHERE id = $1").bind(id).execute(&mut *c.tx).await?;
    c.commit_and_continue(&state.pool).await?;

    let (mut applied, mut errors) = (0, 0);
    let mut todo: Vec<(i32, Cells)> = Vec::new();
    for (row_number, cells) in data_rows(&rows) {
        if let Some(outcome) = done.get(&row_number) {
            applied += (outcome == "applied") as i32;
            errors += (outcome == "error") as i32;
            continue;
        }
        if applied_before.contains(&row_number) {
            record(&mut c, id, &checksum, row_number, None, "skipped", None).await?;
            c.commit_and_continue(&state.pool).await?;
            continue;
        }
        todo.push((row_number, cols.iter().filter_map(|(k, &i)| cells.get(i).map(|v| (*k, v.trim().to_string()))).filter(|(_, v)| !v.is_empty()).collect()));
    }
    let mut seen = Seen::default();
    for chunk in todo.chunks(BATCH) {
        // Punches go in one transaction per batch; a batch that hits a database error is redone row by row.
        let fast = match kind {
            Kind::Punch => punch_batch(&mut c, &em, id, &checksum, chunk, &mut seen).await.ok(),
            Kind::Roster => None,
        };
        match fast {
            Some((a, e)) => {
                c.commit_and_continue(&state.pool).await?;
                applied += a;
                errors += e;
            }
            None => {
                c.rollback_and_continue(&state.pool).await?;
                for (row_number, cell) in chunk {
                    let (a, e) = one_row(state, &mut c, &em, kind, id, &checksum, *row_number, cell, &mut seen).await?;
                    applied += a;
                    errors += e;
                }
            }
        }
        set_state(state, org, id, applied, errors, false, None).await?; // progress, and keeps the claim fresh
    }
    c.commit().await?;
    set_state(state, org, id, applied, errors, true, None).await?;
    Ok(json!({ "state": "completed", "applied_count": applied, "error_count": errors }))
}

/// Rows per transaction when importing punches.
const BATCH: usize = 50;

/// One row in its own transaction; returns (applied, errors) to add. Used for rosters and for punch batches that failed.
#[allow(clippy::too_many_arguments)]
async fn one_row(state: &AppState, c: &mut Ctx, em: &EmSettings, kind: Kind, id: Uuid, checksum: &str, row_number: i32, cell: &Cells, seen: &mut Seen) -> ApiResult<(i32, i32)> {
    let result = match kind {
        Kind::Punch => punch_row(c, em, id, cell, seen).await,
        Kind::Roster => roster_row(c, em, cell).await,
    };
    Ok(match result {
        Ok(Ok((emp, outcome))) => match record(c, id, checksum, row_number, Some(emp), outcome, None).await {
            Ok(()) => {
                c.commit_and_continue(&state.pool).await?;
                ((outcome == "applied") as i32, 0)
            }
            // The same file's row was applied by another run meanwhile: keep that one.
            Err(_) => {
                c.rollback_and_continue(&state.pool).await?;
                record(c, id, checksum, row_number, Some(emp), "skipped", None).await?;
                c.commit_and_continue(&state.pool).await?;
                (0, 0)
            }
        },
        Ok(Err((emp, e))) => {
            c.rollback_and_continue(&state.pool).await?;
            record(c, id, checksum, row_number, emp, "error", Some(&e.to_string())).await?;
            c.commit_and_continue(&state.pool).await?;
            (0, 1)
        }
        Err(e) => {
            c.rollback_and_continue(&state.pool).await?;
            let e = row_err(None, "", e.0.code, &e.0.detail);
            record(c, id, checksum, row_number, None, "error", Some(&e.1.to_string())).await?;
            c.commit_and_continue(&state.pool).await?;
            (0, 1)
        }
    })
}

/// Checks every row of a punch batch (lookups cached), then writes keys, punches, events and row outcomes with one
/// statement each. Any database error fails the whole batch and nothing of it is kept. Returns (applied, errors).
async fn punch_batch(c: &mut Ctx, em: &EmSettings, import: Uuid, checksum: &str, batch: &[(i32, Cells)], seen: &mut Seen) -> ApiResult<(i32, i32)> {
    let mut ok = Vec::new();
    let mut out: Vec<(i32, Option<Uuid>, &str, Option<String>)> = Vec::new();
    for (n, cells) in batch {
        match punch_check(c, em, cells, seen).await? {
            Ok(p) => ok.push((*n, p)),
            Err((emp, e)) => out.push((*n, emp, "error", Some(e.to_string()))),
        }
    }
    let claimed = write_punches(c, import, ok.iter().map(|(_, p)| p).collect()).await?;
    for (n, p) in &ok {
        out.push((*n, Some(p.emp), if claimed.contains(&p.id) { "applied" } else { "skipped" }, None));
    }
    sqlx::query(
        "INSERT INTO app.attendance_import_rows (org_id, import_id, file_checksum, row_number, employee_id, outcome, error_message)
         SELECT $1, $2, $3, x.n, x.e, x.o::app.bulk_row_outcome, x.m FROM unnest($4::int4[], $5::uuid[], $6::text[], $7::text[]) AS x(n, e, o, m)",
    )
    .bind(c.org_id)
    .bind(import)
    .bind(checksum)
    .bind(out.iter().map(|r| r.0).collect::<Vec<_>>())
    .bind(out.iter().map(|r| r.1).collect::<Vec<_>>())
    .bind(out.iter().map(|r| r.2).collect::<Vec<_>>())
    .bind(out.iter().map(|r| r.3.clone()).collect::<Vec<_>>())
    .execute(&mut *c.tx)
    .await?;
    let count = |o: &str| out.iter().filter(|r| r.2 == o).count() as i32;
    Ok((count("applied"), count("error")))
}

/// Lookups that repeat across the rows of one file.
#[derive(Default)]
struct Seen {
    by_code: HashMap<String, Option<Uuid>>,
    allowed: HashMap<Uuid, bool>,
    import_on: HashMap<(Uuid, NaiveDate), bool>,
}

/// A punch row that passed every check, ready to write.
struct PunchIn {
    id: Uuid,
    emp: Uuid,
    key: String,
    at: DateTime<Utc>,
    direction: String,
    lat: Option<f64>,
    lon: Option<f64>,
    enrolment: Option<String>,
    device: Option<Uuid>,
    master: Uuid,
    work_date: NaiveDate,
}

/// Writes the punches whose external_id is new (key, raw punch, `PunchRecorded`); returns the ids written.
async fn write_punches(c: &mut Ctx, import: Uuid, ps: Vec<&PunchIn>) -> ApiResult<HashSet<Uuid>> {
    if ps.is_empty() {
        return Ok(HashSet::new());
    }
    // A key seen before, or twice in this batch, is claimed once; the others become `skipped` (AC-AT-17.2).
    let claimed: HashSet<Uuid> = sqlx::query_scalar(
        "INSERT INTO app.punch_keys (org_id, source, idempotency_key, punch_id, punched_at)
         SELECT $1, 'import', x.k, x.p, x.t FROM unnest($2::text[], $3::uuid[], $4::timestamptz[]) AS x(k, p, t)
         ON CONFLICT DO NOTHING RETURNING punch_id",
    )
    .bind(c.org_id)
    .bind(ps.iter().map(|p| p.key.clone()).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.id).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.at).collect::<Vec<_>>())
    .fetch_all(&mut *c.tx)
    .await?
    .into_iter()
    .collect();
    let ps: Vec<&PunchIn> = ps.into_iter().filter(|p| claimed.contains(&p.id)).collect();
    sqlx::query(
        "INSERT INTO app.raw_punches (id, org_id, punched_at, source, employee_id, enrolment_id, device_id, import_id, direction, source_time, branch_id,
                                      latitude, longitude, idempotency_key)
         SELECT x.id, $1, x.at, 'import', x.emp, x.enrol, x.dev, $2, x.dir::app.punch_direction, x.at, x.br, x.lat, x.lon, x.k
         FROM unnest($3::uuid[], $4::timestamptz[], $5::uuid[], $6::text[], $7::uuid[], $8::text[], $9::uuid[], $10::float8[], $11::float8[], $12::text[])
           AS x(id, at, emp, enrol, dev, dir, br, lat, lon, k)",
    )
    .bind(c.org_id)
    .bind(import)
    .bind(ps.iter().map(|p| p.id).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.at).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.emp).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.enrolment.clone()).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.device).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.direction.clone()).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.master).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.lat).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.lon).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.key.clone()).collect::<Vec<_>>())
    .execute(&mut *c.tx)
    .await?;
    // Same payload as Ctx::event, one row per punch.
    sqlx::query(
        "INSERT INTO app.outbox_events (org_id, event_type, aggregate_type, aggregate_id, payload)
         SELECT $1, 'PunchRecorded', 'employee', x.emp, jsonb_build_object('employee_id', x.emp, 'work_date', x.d, 'punch_id', x.id)
         FROM unnest($2::uuid[], $3::date[], $4::uuid[]) AS x(emp, d, id)",
    )
    .bind(c.org_id)
    .bind(ps.iter().map(|p| p.emp).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.work_date).collect::<Vec<_>>())
    .bind(ps.iter().map(|p| p.id).collect::<Vec<_>>())
    .execute(&mut *c.tx)
    .await?;
    Ok(claimed)
}

async fn record(c: &mut Ctx, import: Uuid, checksum: &str, row: i32, emp: Option<Uuid>, outcome: &str, err: Option<&str>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app.attendance_import_rows (org_id, import_id, file_checksum, row_number, employee_id, outcome, error_message)
         VALUES ($1, $2, $3, $4, $5, $6::app.bulk_row_outcome, $7)",
    )
    .bind(c.org_id)
    .bind(import)
    .bind(checksum)
    .bind(row)
    .bind(emp)
    .bind(outcome)
    .bind(err)
    .execute(&mut *c.tx)
    .await
    .map(|_| ())
}

async fn employee_by_code(c: &mut Ctx, code: &str) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM app.employees WHERE lower(employee_code) = lower($1)").bind(code).fetch_optional(&mut *c.tx).await?)
}

/// A row's non-blank cells by column name.
type Cells = HashMap<&'static str, String>;

fn get<'a>(row: &'a Cells, name: &str) -> Option<&'a str> {
    row.get(name).map(String::as_str)
}

type RowResult = ApiResult<Result<(Uuid, &'static str), RowErr>>;

/// One punch row → a raw punch (source `import`) and `PunchRecorded`, or `skipped` for a known external_id.
async fn punch_row(c: &mut Ctx, em: &EmSettings, import: Uuid, row: &Cells, seen: &mut Seen) -> RowResult {
    Ok(match punch_check(c, em, row, seen).await? {
        Ok(p) => {
            let written = write_punches(c, import, vec![&p]).await?;
            Ok((p.emp, if written.contains(&p.id) { "applied" } else { "skipped" }))
        }
        Err(e) => Err(e),
    })
}

/// Checks one punch row and resolves its employee and work date, writing nothing.
// shortcut: imported punches skip the PC-06 off-day rule and fences (files carry no proof to check); add if HR asks.
async fn punch_check(c: &mut Ctx, em: &EmSettings, row: &Cells, seen: &mut Seen) -> ApiResult<Result<PunchIn, RowErr>> {
    let Some(key) = get(row, "external_id").filter(|k| k.len() <= 100) else {
        return Ok(Err(row_err(None, "external_id", "required", "A unique ID for the punch, up to 100 characters")));
    };
    let Some(at) = get(row, "punch_time").and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc)) else {
        return Ok(Err(row_err(None, "punch_time", "invalid_value", "Date, time and offset, e.g. 2026-11-25T09:05:00+05:30")));
    };
    let direction = get(row, "direction").unwrap_or("untagged");
    if !["in", "out", "untagged"].contains(&direction) {
        return Ok(Err(row_err(None, "direction", "invalid_value", "in, out or blank")));
    }
    let num = |n: &str, max: f64| get(row, n).map(|v| v.parse::<f64>().ok().filter(|x| x.abs() <= max));
    let (lat, lon) = match (num("latitude", 90.0), num("longitude", 180.0)) {
        (None, None) => (None, None),
        (Some(Some(a)), Some(Some(b))) => (Some(a), Some(b)),
        _ => return Ok(Err(row_err(None, "latitude", "invalid_value", "Give both latitude and longitude, as decimal degrees"))),
    };
    let mut device = None;
    let emp = if let Some(code) = get(row, "employee_code") {
        match seen.by_code.get(&code.to_lowercase()) {
            Some(e) => *e,
            None => {
                let e = employee_by_code(c, code).await?;
                seen.by_code.insert(code.to_lowercase(), e);
                e
            }
        }
    } else if let (Some(enrol), Some(serial)) = (get(row, "enrolment_id"), get(row, "device_serial")) {
        device = sqlx::query_scalar::<_, Uuid>("SELECT id FROM app.devices WHERE lower(serial) = lower($1)").bind(serial).fetch_optional(&mut *c.tx).await?;
        if device.is_none() {
            return Ok(Err(row_err(None, "device_serial", "not_found", "No device with this serial")));
        }
        sqlx::query_scalar(
            "SELECT jv.employee_id FROM app.employee_job_versions jv JOIN app.branches b ON b.id = jv.master_branch_id
             WHERE jv.enrolment_id = $1 AND jv.state = 'active' AND jv.valid @> ($2 AT TIME ZONE b.time_zone)::date",
        )
        .bind(enrol)
        .bind(at)
        .fetch_optional(&mut *c.tx)
        .await?
    } else {
        return Ok(Err(row_err(None, "employee_code", "required", "Give employee_code, or enrolment_id with device_serial")));
    };
    let Some(emp) = emp else {
        return Ok(Err(row_err(None, "employee_code", "not_found", "No such employee")));
    };
    let in_scope = match seen.allowed.get(&emp) {
        Some(a) => *a,
        None => {
            let a = allowed(c, emp, PUNCH_IMPORT, Access::Edit, em).await?;
            seen.allowed.insert(emp, a);
            a
        }
    };
    if !in_scope {
        return Ok(Err(row_err(None, "employee_code", "not_found", "No such employee in your scope")));
    }
    let Some((_, master, _, work_date)) = crate::punches::placement(&mut c.tx, emp, at).await? else {
        return Ok(Err(row_err(Some(emp), "punch_time", "outside_employment", "There is no employment on that date")));
    };
    let import_on = match seen.import_on.get(&(emp, work_date)) {
        Some(on) => *on,
        None => {
            let on = crate::policies::settings_for(&mut c.tx, emp, work_date).await?.punch_import;
            seen.import_on.insert((emp, work_date), on);
            on
        }
    };
    if !import_on {
        return Ok(Err(row_err(Some(emp), "", "feature_disabled", "Punch imports are switched off for this employee (PC-01)")));
    }
    Ok(Ok(PunchIn {
        id: Uuid::new_v4(),
        emp,
        key: key.to_string(),
        at,
        direction: direction.to_string(),
        lat,
        lon,
        enrolment: get(row, "enrolment_id").map(str::to_string),
        device,
        master,
        work_date,
    }))
}

/// One roster row through the same code as the roster screen (R2).
async fn roster_row(c: &mut Ctx, em: &EmSettings, row: &Cells) -> RowResult {
    let Some(code) = get(row, "employee_code") else {
        return Ok(Err(row_err(None, "employee_code", "required", "Give the employee code")));
    };
    let Some(emp) = employee_by_code(c, code).await? else {
        return Ok(Err(row_err(None, "employee_code", "not_found", "No such employee")));
    };
    let Some(date) = get(row, "work_date").and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
        return Ok(Err(row_err(Some(emp), "work_date", "invalid_value", "A date as YYYY-MM-DD")));
    };
    let off = get(row, "weekly_off").map(|v| ["yes", "y", "true", "1"].contains(&v.to_lowercase().as_str())).unwrap_or(false);
    let shift = match get(row, "shift_code") {
        Some(s) => match sqlx::query_scalar::<_, Uuid>("SELECT id FROM app.shifts WHERE lower(code) = lower($1)").bind(s).fetch_optional(&mut *c.tx).await? {
            Some(id) => Some(id),
            None => return Ok(Err(row_err(Some(emp), "shift_code", "not_found", "No shift with this code"))),
        },
        None => None,
    };
    if shift.is_some() == off {
        return Ok(Err(row_err(Some(emp), "shift_code", "invalid_value", "Give a shift_code or weekly_off = yes, not both")));
    }
    let entry = crate::roster::Entry { employee_id: emp, work_date: date, shift_id: shift, is_weekly_off: off };
    let out = crate::roster::apply_entries(c, em, vec![entry], "upload").await?;
    if let Some(f) = out.failed.first() {
        let code = f["code"].as_str().unwrap_or("invalid_value");
        return Ok(Err((Some(emp), json!({ "field": "work_date", "code": code, "message": code.replace('_', " ") }))));
    }
    Ok(Ok((emp, "applied")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_check() {
        let h = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let c = columns(&h(&["External ID", "punch_time", "employee_code"]), Kind::Punch).unwrap();
        assert_eq!(c["external_id"], 0);
        assert!(columns(&h(&["external_id"]), Kind::Punch).is_err()); // punch_time missing
        assert!(columns(&h(&["employee_code*", "work_date*", "colour"]), Kind::Roster).is_err());
        assert!(columns(&h(&["employee_code", "work_date", "work_date"]), Kind::Roster).is_err());
    }
}
