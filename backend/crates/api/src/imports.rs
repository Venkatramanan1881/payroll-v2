//! Bulk import B1–B5 and J5 (API_CONTRACTS §5.5, SM-7, US-EM-15).
//! Each row goes through the same code as the screens (E5 create, E6 personal, E8 job change,
//! E10 custom fields) as the uploader, in its own transaction: a row is applied whole or not at all,
//! and every row is audited like a screen edit.

use crate::auth::{read_step, step_token};
use crate::ctx::Ctx;
use crate::employees::{create_in, em_settings, limit_event, personal_in, today, Tx};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use calamine::{open_workbook_from_rs, Data, DataType, Reader, Xlsx};
use chrono::NaiveDate;
use payroll_core::settings::CodeMode;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shared::{AppError, FieldError};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_ROWS: usize = 5000;
const ERRORS_LINK_SECONDS: u64 = 10 * 60;
/// A run that has not reported progress for this long is taken to be dead; J5 may take it over.
const STALE_MINUTES: i32 = 10;

const PERSONAL: [&str; 8] = [
    "full_name", "mobile", "date_of_birth", "gender", "email", "address", "emergency_contact_name", "emergency_contact_phone",
];
const JOINER: [&str; 3] = ["joining_date", "initial_status", "probation_end_date"];
/// Job columns that name a list item: (column, request field, table, column matched, case-insensitive).
const LISTS: [(&str, &str, &str, &str); 7] = [
    ("master_branch", "master_branch_id", "branches", "short_code"),
    ("department", "department_id", "departments", "code"),
    ("designation", "designation_id", "designations", "code"),
    ("category", "category_id", "categories", "code"),
    ("employee_type", "employee_type_id", "employee_types", "code"),
    ("contractor", "contractor_id", "contractors", "code"),
    ("payroll_group", "payroll_group_id", "payroll_groups", "name"),
];
const DATES: [&str; 4] = ["date_of_birth", "joining_date", "probation_end_date", "effective_from"];

/// Every standard column, in template order.
fn standard() -> Vec<&'static str> {
    let mut v = vec!["employee_code"];
    v.extend(PERSONAL);
    v.extend(JOINER);
    v.push("effective_from");
    v.push("master_branch");
    v.push("allowed_branches");
    v.extend(LISTS.iter().skip(1).map(|l| l.0));
    v.push("reporting_manager");
    v.push("enrolment_id");
    v
}

struct CustomDef {
    id: Uuid,
    label: String,
    kind: String,
    mandatory: bool,
}

async fn custom_defs(tx: &mut Tx) -> ApiResult<Vec<CustomDef>> {
    Ok(sqlx::query("SELECT id, label, field_type::text AS kind, is_mandatory FROM app.custom_field_definitions WHERE state = 'active' ORDER BY label")
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(|r| CustomDef { id: r.get("id"), label: r.get("label"), kind: r.get("kind"), mandatory: r.get("is_mandatory") })
        .collect())
}

// ---------- reading the file ----------

fn unreadable(why: impl Into<String>) -> AppError {
    AppError::unprocessable("file_unreadable", "File not readable", why)
}

fn too_large(why: &str) -> AppError {
    AppError::new(413, "file_too_large", "File too large", why)
}

fn cell_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(_) => d.as_datetime().map(|t| t.date().to_string()).unwrap_or_default(),
        Data::DateTimeIso(s) => s.chars().take(10).collect(),
        _ => String::new(),
    }
}

/// All rows of the first sheet as trimmed text. A file starting "PK" is read as .xlsx, anything else as CSV.
fn read_sheet(bytes: &[u8]) -> Result<Vec<Vec<String>>, AppError> {
    if bytes.starts_with(b"PK") {
        let mut book: Xlsx<_> = open_workbook_from_rs(std::io::Cursor::new(bytes)).map_err(|_| unreadable("Not a readable .xlsx file"))?;
        let range = book.worksheet_range_at(0).ok_or_else(|| unreadable("The workbook has no sheet"))?.map_err(|_| unreadable("The first sheet can't be read"))?;
        return Ok(range.rows().map(|r| r.iter().map(cell_text).collect()).collect());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("Not a readable .xlsx or UTF-8 .csv file"))?;
    let mut rows = Vec::new();
    for rec in csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(text.trim_start_matches('\u{feff}').as_bytes()).records() {
        rows.push(rec.map_err(|e| unreadable(format!("CSV error: {e}")))?.iter().map(|c| c.trim().to_string()).collect());
    }
    Ok(rows)
}

#[derive(Clone, Copy, Debug)]
enum Col {
    Std(&'static str),
    Custom(usize),
    Blank,
}

/// Maps the header row to columns. A trailing `*` (the template's mandatory mark) is ignored.
fn header(row: &[String], defs: &[CustomDef]) -> Result<Vec<Col>, AppError> {
    let std = standard();
    let wrong = |m: String| AppError::unprocessable("wrong_template", "Columns don't match the template", m);
    let mut cols = Vec::new();
    let mut seen = HashSet::new();
    for h in row {
        let name = h.trim().trim_end_matches('*').trim();
        if name.is_empty() {
            cols.push(Col::Blank);
            continue;
        }
        let key = name.to_lowercase().replace(' ', "_");
        let col = if let Some(s) = std.iter().find(|s| **s == key) {
            Col::Std(s)
        } else if let Some(i) = defs.iter().position(|d| d.label.eq_ignore_ascii_case(name)) {
            Col::Custom(i)
        } else {
            return Err(wrong(format!("Unknown column \"{name}\"")));
        };
        if !seen.insert(key) {
            return Err(wrong(format!("Column \"{name}\" appears twice")));
        }
        cols.push(col);
    }
    if !cols.iter().any(|c| matches!(c, Col::Std("employee_code"))) {
        return Err(wrong("The employee_code column is missing".into()));
    }
    Ok(cols)
}

/// Data rows with their sheet row number (header = row 1); blank rows are left out.
fn data_rows(rows: &[Vec<String>]) -> Vec<(i32, &Vec<String>)> {
    rows.iter().enumerate().skip(1).filter(|(_, r)| r.iter().any(|c| !c.is_empty())).map(|(i, r)| (i as i32 + 1, r)).collect()
}

// ---------- B1 template ----------

pub async fn template(mut ctx: Ctx) -> ApiResult<Response> {
    ctx.scope("employee.bulk_import")?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    let defs = custom_defs(&mut ctx.tx).await?;
    ctx.commit().await?;
    // Mandatory for a new employee (E5), plus the organisation's own mandatory fields (AC-EM-15.1).
    let always = ["full_name", "mobile", "joining_date", "initial_status", "master_branch", "department", "category", "employee_type", "payroll_group"];
    let mut heads: Vec<String> = standard()
        .into_iter()
        .map(|c| {
            let must = always.contains(&c) || em.is_mandatory(c) || (c == "employee_code" && em.employee_code_mode == CodeMode::Manual);
            if must { format!("{c} *") } else { c.to_string() }
        })
        .collect();
    heads.extend(defs.iter().map(|d| if d.mandatory { format!("{} *", d.label) } else { d.label.clone() }));
    let bytes = xlsx(&[heads]).map_err(|_| AppError::internal())?;
    Ok(xlsx_response(bytes, "employee-import-template.xlsx"))
}

pub(crate) fn xlsx(rows: &[Vec<String>]) -> Result<Vec<u8>, rust_xlsxwriter::XlsxError> {
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if r == 0 {
                sheet.write_string_with_format(0, c as u16, v, &bold)?;
                sheet.set_column_width(c as u16, (v.len() as f64 + 4.0).max(14.0))?;
            } else {
                sheet.write_string(r as u32, c as u16, v)?;
            }
        }
    }
    book.save_to_buffer()
}

pub(crate) fn xlsx_response(bytes: Vec<u8>, name: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        bytes,
    )
        .into_response()
}

// ---------- B2 upload ----------

// shortcut: files are kept on local disk under FILE_DIR, not Cloud Storage (TSD §11); on Cloud Run
// that disk is per instance and wiped on restart. Move to the bucket before deploying.
pub async fn upload(State(state): State<AppState>, mut ctx: Ctx, headers: HeaderMap, mut form: Multipart) -> ApiResult<Response> {
    ctx.scope("employee.bulk_import")?;
    let mut file = None;
    while let Some(field) = form.next_field().await.map_err(|_| unreadable("The upload could not be read"))? {
        if field.name() == Some("file") {
            let bytes = field.bytes().await.map_err(|e| match e.status() {
                StatusCode::PAYLOAD_TOO_LARGE => too_large("The file is over 5 MB"),
                _ => unreadable("The upload could not be read"),
            })?;
            file = Some(bytes);
        }
    }
    let bytes = file.ok_or_else(|| AppError::validation(vec![FieldError::new("file", "required", "Attach the file")]))?;
    if bytes.len() > MAX_BYTES {
        return Err(too_large("The file is over 5 MB").into());
    }
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    if let Some((status, body)) = ctx.idempotency_begin(&headers, "POST /v1/employee-imports", &json!({ "checksum": checksum })).await? {
        ctx.commit().await?;
        return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body)).into_response());
    }
    let rows = read_sheet(&bytes)?;
    let head = rows.first().ok_or_else(|| unreadable("The file is empty"))?;
    header(head, &custom_defs(&mut ctx.tx).await?)?;
    let total = data_rows(&rows).len();
    if total > MAX_ROWS {
        return Err(too_large("The file has more than 5,000 rows").into());
    }

    let duplicate_of: Option<Uuid> = sqlx::query_scalar("SELECT id FROM app.bulk_imports WHERE file_checksum = $1 ORDER BY created_at LIMIT 1")
        .bind(&checksum)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let id = Uuid::new_v4();
    let ext = if bytes.starts_with(b"PK") { "xlsx" } else { "csv" };
    let file_ref = format!("{}/imports/{}/{id}.{ext}", ctx.org_id, chrono::Utc::now().format("%Y-%m"));
    let path = state.files_dir.join(&file_ref);
    tokio::fs::create_dir_all(path.parent().unwrap()).await.map_err(|_| AppError::internal())?;
    tokio::fs::write(&path, &bytes).await.map_err(|_| AppError::internal())?;
    sqlx::query(
        "INSERT INTO app.bulk_imports (id, org_id, file_ref, file_checksum, uploaded_by, total_rows) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(&file_ref)
    .bind(&checksum)
    .bind(ctx.user_id)
    .bind(total as i32)
    .execute(&mut *ctx.tx)
    .await?;
    ctx.audit("employee.bulk_import", "bulk_import", id, None, Some(json!({ "checksum": checksum, "total_rows": total })), None).await?;
    let body = json!({ "import_id": id, "state": "uploaded", "duplicate_of": duplicate_of });
    ctx.idempotency_finish(&headers, 202, &body).await?;
    let org = ctx.org_id;
    ctx.commit().await?;
    // shortcut: processed in this server process right after upload; once Cloud Tasks is set up it should call J5 instead.
    tokio::spawn(async move {
        if let Err(e) = process(&state, org, id).await {
            tracing::error!(import = %id, error = ?e.0, "bulk import failed to run");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

// ---------- B3 / B5 ----------

const IMPORT: &str = "SELECT jsonb_build_object('id', i.id, 'state', i.state, 'total_rows', i.total_rows,
        'applied_count', i.applied_count, 'error_count', i.error_count,
        'skipped_count', (SELECT count(*) FROM app.bulk_import_rows r WHERE r.import_id = i.id AND r.outcome = 'skipped'),
        'failure_reason', i.failure_reason, 'uploaded_by', i.uploaded_by, 'created_at', i.created_at, 'completed_at', i.completed_at)
    FROM app.bulk_imports i";

pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    ctx.scope("employee.bulk_import")?;
    let doc: Value = sqlx::query_scalar(&format!("{IMPORT} WHERE i.id = $1"))
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    ctx.commit().await?;
    Ok(Json(doc))
}

// shortcut: the latest 100, no paging yet.
pub async fn list(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    ctx.scope("employee.bulk_import")?;
    let items: Vec<Value> = sqlx::query_scalar(&format!("{IMPORT} ORDER BY i.created_at DESC LIMIT 100")).fetch_all(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

// ---------- B4 error report ----------

/// A 10-minute link; the report is built from the stored row results when it is downloaded.
pub async fn errors(State(state): State<AppState>, mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    ctx.scope("employee.bulk_import")?;
    // Kept 30 days after the import finished (B4); J7 deletes the upload then too.
    let expired: bool = sqlx::query_scalar("SELECT coalesce(completed_at < now() - interval '30 days', false) FROM app.bulk_imports WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if expired {
        return Err(AppError::new(410, "link_expired", "Report expired", "Error reports are kept for 30 days").into());
    }
    let token = step_token(&state, id, ctx.org_id, "import_errors", ERRORS_LINK_SECONDS);
    ctx.commit().await?;
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ERRORS_LINK_SECONDS as i64);
    Ok(Json(json!({ "url": format!("/v1/files/import-errors/{token}"), "expires_at": expires_at })))
}

/// The link itself: no sign-in needed, the token is the permission (like a signed storage link).
pub async fn errors_file(State(state): State<AppState>, Path(token): Path<String>) -> ApiResult<Response> {
    let gone = || AppError::new(410, "link_expired", "Link expired", "Ask for a new link");
    let claims = read_step(&state, &token, "import_errors").ok_or_else(gone)?;
    let mut tx = db::begin_tenant(&state.pool, claims.org, None).await?;
    let found = sqlx::query("SELECT row_number, employee_code, error_message FROM app.bulk_import_rows WHERE import_id = $1 AND outcome = 'error' ORDER BY row_number")
        .bind(claims.sub)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut rows = vec![["row", "employee_code", "field", "code", "message"].map(String::from).to_vec()];
    for r in found {
        let list: Vec<Value> = serde_json::from_str(r.get("error_message")).unwrap_or_default();
        for e in list {
            let s = |k: &str| e[k].as_str().unwrap_or_default().to_string();
            rows.push(vec![r.get::<i32, _>("row_number").to_string(), r.get::<Option<String>, _>("employee_code").unwrap_or_default(), s("field"), s("code"), s("message")]);
        }
    }
    let bytes = xlsx(&rows).map_err(|_| AppError::internal())?;
    Ok(xlsx_response(bytes, "import-errors.xlsx"))
}

// ---------- J5 processing ----------

async fn set_failed(state: &AppState, org: Uuid, id: Uuid, reason: &str) -> ApiResult<Value> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    sqlx::query("UPDATE app.bulk_imports SET state = 'failed', failure_reason = $2, completed_at = now() WHERE id = $1")
        .bind(id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({ "state": "failed", "failure_reason": reason }))
}

async fn set_counts(state: &AppState, org: Uuid, id: Uuid, applied: i32, errors: i32, done: bool) -> ApiResult<()> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    sqlx::query(
        "UPDATE app.bulk_imports SET applied_count = $2, error_count = $3,
           state = CASE WHEN $4 THEN 'completed' ELSE state END, completed_at = CASE WHEN $4 THEN now() END
         WHERE id = $1",
    )
    .bind(id)
    .bind(applied)
    .bind(errors)
    .bind(done)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// J5: validate and apply every row. Safe to run twice: a run takes the import over only if it is
/// new or its last run went quiet, and rows already recorded for this import are not redone.
pub async fn process(state: &AppState, org: Uuid, id: Uuid) -> ApiResult<Value> {
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    let claimed = sqlx::query(
        "UPDATE app.bulk_imports SET state = 'validating'
         WHERE id = $1 AND (state = 'uploaded' OR (state IN ('validating', 'applying') AND updated_at < now() - make_interval(mins => $2)))
         RETURNING file_ref, file_checksum, uploaded_by",
    )
    .bind(id)
    .bind(STALE_MINUTES)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(claimed) = claimed else {
        let st: Option<String> = sqlx::query_scalar("SELECT state::text FROM app.bulk_imports WHERE id = $1").bind(id).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        return match st {
            Some(st) => Ok(json!({ "state": st, "note": "already finished or running" })),
            None => Err(AppError::not_found().into()),
        };
    };
    tx.commit().await?;
    let (file_ref, checksum, uploader): (String, String, Uuid) = (claimed.get("file_ref"), claimed.get("file_checksum"), claimed.get("uploaded_by"));

    let Ok(bytes) = tokio::fs::read(state.files_dir.join(&file_ref)).await else {
        return set_failed(state, org, id, "file_unreadable: the uploaded file is no longer available").await;
    };
    let rows = match read_sheet(&bytes) {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return set_failed(state, org, id, "file_unreadable: the file is empty").await,
        Err(e) => return set_failed(state, org, id, &format!("{}: {}", e.code, e.detail)).await,
    };
    let mut c = match Ctx::for_user(&state.pool, org, uploader).await {
        Ok(c) => c,
        Err(_) => return set_failed(state, org, id, "uploader_not_active: the uploader can no longer make changes, or the organisation is suspended").await,
    };
    let defs = custom_defs(&mut c.tx).await?;
    let cols = match header(&rows[0], &defs) {
        Ok(cols) => cols,
        Err(e) => return set_failed(state, org, id, &format!("{}: {}", e.code, e.detail)).await,
    };
    let lookups = load_lookups(&mut c.tx).await?;
    let done: HashMap<i32, String> = sqlx::query("SELECT row_number, outcome::text AS outcome FROM app.bulk_import_rows WHERE import_id = $1")
        .bind(id)
        .fetch_all(&mut *c.tx)
        .await?
        .iter()
        .map(|r| (r.get("row_number"), r.get("outcome")))
        .collect();
    let applied_before: HashSet<i32> =
        sqlx::query_scalar("SELECT row_number FROM app.bulk_import_rows WHERE file_checksum = $1 AND outcome = 'applied' AND import_id <> $2")
            .bind(&checksum)
            .bind(id)
            .fetch_all(&mut *c.tx)
            .await?
            .into_iter()
            .collect();
    sqlx::query("UPDATE app.bulk_imports SET state = 'applying' WHERE id = $1").bind(id).execute(&mut *c.tx).await?;
    c.commit_and_continue(&state.pool).await?;

    // One transaction per row: a row is applied whole, or rolled back and reported.
    let (mut applied, mut errors) = (0, 0);
    for (n, (row_number, cells)) in data_rows(&rows).into_iter().enumerate() {
        if let Some(outcome) = done.get(&row_number) {
            applied += (outcome == "applied") as i32;
            errors += (outcome == "error") as i32;
            continue;
        }
        let code_cell = cols.iter().zip(cells.iter()).find(|(c, _)| matches!(c, Col::Std("employee_code"))).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
        if applied_before.contains(&row_number) {
            record(&mut c.tx, org, id, &checksum, row_number, code_cell.as_deref(), None, "skipped", None).await?;
            c.commit_and_continue(&state.pool).await?;
            continue;
        }
        let row = ImportRow { number: row_number, cols: &cols, cells, defs: &defs, lookups: &lookups, import: id };
        match apply_row(state, &mut c, &row).await {
            Ok((emp, code)) => match record(&mut c.tx, org, id, &checksum, row_number, Some(&code), Some(emp), "applied", None).await {
                Ok(()) => {
                    c.commit_and_continue(&state.pool).await?;
                    applied += 1;
                }
                // The same file's row was applied by another run meanwhile: keep that one.
                Err(_) => {
                    c.rollback_and_continue(&state.pool).await?;
                    record(&mut c.tx, org, id, &checksum, row_number, Some(&code), None, "skipped", None).await?;
                    c.commit_and_continue(&state.pool).await?;
                }
            },
            Err(e) => {
                c.rollback_and_continue(&state.pool).await?;
                let list = error_list(&e.0, &defs);
                record(&mut c.tx, org, id, &checksum, row_number, code_cell.as_deref(), None, "error", Some(&list.to_string())).await?;
                c.commit_and_continue(&state.pool).await?;
                errors += 1;
            }
        }
        if n % 50 == 49 {
            set_counts(state, org, id, applied, errors, false).await?; // progress, and keeps the run's claim fresh
        }
    }
    c.commit().await?;
    set_counts(state, org, id, applied, errors, true).await?;
    Ok(json!({ "state": "completed", "applied_count": applied, "error_count": errors }))
}

#[allow(clippy::too_many_arguments)]
async fn record(tx: &mut Tx, org: Uuid, import: Uuid, checksum: &str, row: i32, code: Option<&str>, emp: Option<Uuid>, outcome: &str, err: Option<&str>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app.bulk_import_rows (org_id, import_id, file_checksum, row_number, employee_code, employee_id, outcome, error_message)
         VALUES ($1, $2, $3, $4, $5, $6, $7::app.bulk_row_outcome, $8)",
    )
    .bind(org)
    .bind(import)
    .bind(checksum)
    .bind(row)
    .bind(code)
    .bind(emp)
    .bind(outcome)
    .bind(err)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// Each problem as {field, code, message}, with request field names turned back into column names.
fn error_list(e: &AppError, defs: &[CustomDef]) -> Value {
    let column = |f: &str| -> String {
        let f = f.trim_start_matches("personal.").trim_start_matches("job.").trim_start_matches("changes.");
        if let Some(l) = LISTS.iter().find(|l| l.1 == f) {
            return l.0.into();
        }
        if let Some(d) = f.parse::<Uuid>().ok().and_then(|u| defs.iter().find(|d| d.id == u)) {
            return d.label.clone();
        }
        match f {
            "allowed_branch_ids" => "allowed_branches".into(),
            "reporting_manager_id" => "reporting_manager".into(),
            _ => f.into(),
        }
    };
    if e.errors.is_empty() {
        return json!([{ "field": "", "code": e.code, "message": e.detail }]);
    }
    Value::Array(e.errors.iter().map(|f| json!({ "field": column(&f.field), "code": f.code, "message": f.message })).collect())
}

type Lookups = HashMap<&'static str, HashMap<String, Uuid>>;

async fn load_lookups(tx: &mut Tx) -> ApiResult<Lookups> {
    let mut out = HashMap::new();
    for (_, _, table, col) in LISTS {
        let rows = sqlx::query(&format!("SELECT lower({col}) AS k, id FROM app.{table}")).fetch_all(&mut **tx).await?;
        out.insert(table, rows.iter().map(|r| (r.get("k"), r.get("id"))).collect());
    }
    Ok(out)
}

struct ImportRow<'a> {
    number: i32,
    cols: &'a [Col],
    cells: &'a [String],
    defs: &'a [CustomDef],
    lookups: &'a Lookups,
    import: Uuid,
}

impl ImportRow<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.cols.iter().zip(self.cells).find(|(c, _)| matches!(c, Col::Std(k) if *k == key)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
}

fn bad(field: &str, code: &'static str, msg: impl Into<String>) -> FieldError {
    FieldError::new(field, code, msg)
}

/// Custom field cell → the JSON value E10 takes.
fn custom_value(def: &CustomDef, v: &str) -> Result<Value, String> {
    match def.kind.as_str() {
        "number" => v.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).ok_or_else(|| "Enter a number".into()),
        "yes_no" => match v.to_lowercase().as_str() {
            "yes" | "y" | "true" => Ok(Value::Bool(true)),
            "no" | "n" | "false" => Ok(Value::Bool(false)),
            _ => Err("Enter yes or no".into()),
        },
        "multiple_choice" => Ok(Value::Array(v.split(',').map(|s| Value::from(s.trim())).filter(|s| s != "").collect())),
        _ => Ok(Value::from(v)),
    }
}

/// One row: a new employee when the code is blank or unknown, otherwise changes to that employee.
async fn apply_row(state: &AppState, ctx: &mut Ctx, row: &ImportRow<'_>) -> Result<(Uuid, String), ApiError> {
    let mut errs = Vec::new();
    for d in DATES {
        if let Some(v) = row.get(d) {
            if v.parse::<NaiveDate>().is_err() {
                errs.push(bad(d, "invalid_value", "Use a date, written YYYY-MM-DD"));
            }
        }
    }
    let mut job = Map::new();
    for (col, field, table, _) in LISTS {
        if let Some(v) = row.get(col) {
            match row.lookups[table].get(&v.to_lowercase()) {
                Some(id) => {
                    job.insert(field.into(), json!(id));
                }
                None => errs.push(bad(col, "not_found", format!("No {} \"{v}\"", col.replace('_', " ")))),
            }
        }
    }
    if let Some(v) = row.get("allowed_branches") {
        let mut ids = Vec::new();
        for code in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match row.lookups["branches"].get(&code.to_lowercase()) {
                Some(id) => ids.push(json!(id)),
                None => errs.push(bad("allowed_branches", "not_found", format!("No branch \"{code}\""))),
            }
        }
        job.insert("allowed_branch_ids".into(), Value::Array(ids));
    }
    if let Some(v) = row.get("reporting_manager") {
        let m: Option<Uuid> = sqlx::query_scalar("SELECT id FROM app.employees WHERE employee_code = $1 AND is_current").bind(v).fetch_optional(&mut *ctx.tx).await?;
        match m {
            Some(m) => {
                job.insert("reporting_manager_id".into(), json!(m));
            }
            None => errs.push(bad("reporting_manager", "not_found", format!("No current employee with code \"{v}\""))),
        }
    }
    if let Some(v) = row.get("enrolment_id") {
        job.insert("enrolment_id".into(), json!(v));
    }
    let mut custom = Map::new();
    for (c, v) in row.cols.iter().zip(row.cells).filter(|(_, v)| !v.is_empty()) {
        if let Col::Custom(i) = c {
            let def = &row.defs[*i];
            match custom_value(def, v) {
                Ok(val) => {
                    custom.insert(def.id.to_string(), val);
                }
                Err(m) => errs.push(bad(&def.label, "invalid_value", m)),
            }
        }
    }
    let personal: Map<String, Value> = PERSONAL.iter().filter_map(|f| row.get(f).map(|v| (f.to_string(), json!(v)))).collect();

    let code = row.get("employee_code");
    let existing: Option<Uuid> = match code {
        Some(c) => sqlx::query_scalar("SELECT id FROM app.employees WHERE employee_code = $1").bind(c).fetch_optional(&mut *ctx.tx).await?,
        None => None,
    };
    let id = match existing {
        None => {
            if row.get("effective_from").is_some() {
                errs.push(bad("effective_from", "invalid_value", "Only for changes to an existing employee"));
            }
            if !errs.is_empty() {
                return Err(AppError::validation(errs).into());
            }
            let mut body = json!({ "personal": personal, "job": job });
            for f in JOINER {
                if let Some(v) = row.get(f) {
                    body[f] = json!(if f == "initial_status" { v.to_lowercase().replace(' ', "_") } else { v.to_string() });
                }
            }
            if let Some(c) = code {
                body["employee_code"] = json!(c);
            }
            let (id, over, _) = create_in(state, ctx, body).await?;
            limit_event(ctx, over).await?;
            if !custom.is_empty() {
                crate::employee_details::custom_fields_in(ctx, id, Value::Object(custom), None).await?;
            }
            id
        }
        Some(id) => {
            for f in JOINER {
                if row.get(f).is_some() {
                    errs.push(bad(f, "invalid_value", "Only for a new employee; this code already exists"));
                }
            }
            if !job.is_empty() && row.get("effective_from").is_none() {
                errs.push(bad("effective_from", "required", "Give the date the job change starts (AC-EM-15.4)"));
            }
            if personal.is_empty() && job.is_empty() && custom.is_empty() {
                return Err(AppError::unprocessable("no_change", "Nothing to change", "The row has only an employee code").into());
            }
            if !errs.is_empty() {
                return Err(AppError::validation(errs).into());
            }
            // Outside the uploader's scope reads as unknown (edge case 6).
            let hide = |e: ApiError| if e.0.status == 403 { ApiError(AppError::not_found()) } else { e };
            if !personal.is_empty() {
                personal_in(ctx, id, Value::Object(personal), None, "employee.edit").await.map_err(hide)?;
            }
            if !job.is_empty() {
                let body = json!({ "effective_from": row.get("effective_from"), "changes": job, "reason": format!("Bulk import {}, row {}", row.import, row.number) });
                match crate::job_changes::change_in(ctx, id, body, None).await.map_err(hide) {
                    Err(e) if e.0.code == "no_change" => {}
                    other => {
                        other?;
                    }
                }
            }
            if !custom.is_empty() {
                crate::employee_details::custom_fields_in(ctx, id, Value::Object(custom), None).await.map_err(hide)?;
            }
            id
        }
    };
    let code: String = sqlx::query_scalar("SELECT employee_code FROM app.employees WHERE id = $1").bind(id).fetch_one(&mut *ctx.tx).await?;
    Ok((id, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_rows() {
        let defs = vec![CustomDef { id: Uuid::nil(), label: "Shirt size".into(), kind: "dropdown".into(), mandatory: false }];
        let rows = read_sheet(b"\xef\xbb\xbfEmployee Code *,full_name,SHIRT SIZE\nE1,\"Ravi, K\",M\n,,\nE2,Anu,\n").unwrap();
        let cols = header(&rows[0], &defs).unwrap();
        assert!(matches!(cols[0], Col::Std("employee_code")) && matches!(cols[2], Col::Custom(0)));
        let data = data_rows(&rows);
        assert_eq!(data.iter().map(|d| d.0).collect::<Vec<_>>(), [2, 4]); // blank row 3 left out
        assert_eq!(data[0].1[1], "Ravi, K");
        assert_eq!(header(&["full_name".into()], &defs).unwrap_err().code, "wrong_template");
        assert_eq!(header(&["employee_code".into(), "salary".into()], &defs).unwrap_err().code, "wrong_template");
        assert_eq!(read_sheet(&[0xff, 0xfe, 0x00]).unwrap_err().code, "file_unreadable");
    }

    #[test]
    fn xlsx_round_trip() {
        let bytes = xlsx(&[vec!["employee_code".into(), "mobile".into()], vec!["E1".into(), "+919876543210".into()]]).unwrap();
        let rows = read_sheet(&bytes).unwrap();
        assert_eq!(rows[1], ["E1", "+919876543210"]);
    }
}
