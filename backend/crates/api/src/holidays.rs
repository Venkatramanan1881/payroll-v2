//! Holiday calendars H1–H5 and optional holiday picks H6–H10 (API_CONTRACTS Package 2 §4, US-AT-06, 08).
//! Holidays are never edited in place: a change removes the old row and adds a new one, so recalculation
//! can still see what was a holiday before (removed_at).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{allowed, bad_param, check_item, em_settings, push_from, push_scope, query_pairs, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::periods::date_locked;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

const MANAGE: &str = "attendance.calendar.manage";
const REQUEST: &str = "attendance.optional_holiday.request";
const APPROVE: &str = "attendance.optional_holiday.approve";
const KINDS: [&str; 3] = ["national", "festival", "optional"];

const CALENDAR: &str = "jsonb_build_object('id', c.id, 'name', c.name, 'year', c.year, 'state', c.state, 'version', c.row_version,
    'holidays_count', (SELECT count(*) FROM app.holidays h WHERE h.calendar_id = c.id AND h.removed_at IS NULL),
    'branches', (SELECT coalesce(jsonb_agg(jsonb_build_object('id', b.id, 'short_code', b.short_code) ORDER BY b.short_code), '[]')
                 FROM app.branch_calendars bc JOIN app.branches b ON b.id = bc.branch_id WHERE bc.calendar_id = c.id))";
const HOLIDAY: &str = "jsonb_build_object('id', h.id, 'date', h.holiday_date, 'name', h.name, 'kind', h.kind, 'removed_at', h.removed_at)";
const PICK: &str = "jsonb_build_object('id', p.id, 'employee_id', p.employee_id, 'state', p.state, 'raised_by', p.raised_by,
    'decided_by', p.decided_by, 'decided_at', p.decided_at, 'decision_reason', p.decision_reason, 'created_at', p.created_at,
    'version', p.row_version, 'holiday', jsonb_build_object('id', h.id, 'date', h.holiday_date, 'name', h.name))";

fn can_view(ctx: &Ctx) -> ApiResult<()> {
    if ctx.has("attendance.roster.view") || ctx.has(MANAGE) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

fn reason_of(r: Option<String>) -> Option<String> {
    r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

async fn calendar_doc(tx: &mut Tx, id: Uuid, include_removed: bool) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {CALENDAR} || jsonb_build_object('holidays', (SELECT coalesce(jsonb_agg({HOLIDAY} ORDER BY h.holiday_date, h.created_at), '[]')
           FROM app.holidays h WHERE h.calendar_id = c.id AND ($2 OR h.removed_at IS NULL)))
         FROM app.holiday_calendars c WHERE c.id = $1"
    ))
    .bind(id)
    .bind(include_removed)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::not_found().into())
}

/// The lock unit, not open, of any of `branches` whose period holds any of `dates` (BR-AT-18).
async fn branch_lock(tx: &mut Tx, branches: &[Uuid], dates: &[NaiveDate]) -> ApiResult<Option<(Uuid, String, NaiveDate)>> {
    Ok(sqlx::query_as(
        "SELECT lu.id, lu.state::text, d.d FROM app.lock_units lu JOIN app.attendance_periods p ON p.id = lu.period_id
         JOIN unnest($2::date[]) d(d) ON p.period @> d.d
         WHERE lu.state <> 'open' AND lu.branch_id = ANY($1) ORDER BY d.d LIMIT 1",
    )
    .bind(branches)
    .bind(dates)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Locks the calendar row, checks If-Match and returns (year, linked branches).
async fn lock_calendar(tx: &mut Tx, id: Uuid, expected: Option<i32>) -> ApiResult<(i32, Vec<Uuid>)> {
    let row: Option<(i16, i32)> = sqlx::query_as("SELECT year, row_version FROM app.holiday_calendars WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some((year, v)) = row else { return Err(AppError::not_found().into()) };
    if expected.is_some_and(|e| e != v) {
        return Err(stale(v));
    }
    let branches = sqlx::query_scalar("SELECT branch_id FROM app.branch_calendars WHERE calendar_id = $1").bind(id).fetch_all(&mut **tx).await?;
    Ok((year as i32, branches))
}

/// After a holiday change: refuse locked dates, bump the calendar version, tell linked branches.
async fn holiday_changed(ctx: &mut Ctx, calendar: Uuid, branches: &[Uuid], dates: &[NaiveDate]) -> ApiResult<()> {
    if let Some((unit, state, d)) = branch_lock(&mut ctx.tx, branches, dates).await? {
        return Err(date_locked((unit, state), d));
    }
    sqlx::query("UPDATE app.holiday_calendars SET row_version = row_version + 1, updated_at = now() WHERE id = $1")
        .bind(calendar)
        .execute(&mut *ctx.tx)
        .await?;
    if !branches.is_empty() {
        let (from, to) = (dates.iter().min(), dates.iter().max());
        ctx.event("ScheduleChanged", "holiday_calendar", calendar, json!({ "branch_ids": branches, "from": from, "to": to })).await?;
    }
    Ok(())
}

// ---------- H1 GET /attendance/holiday-calendars ----------

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    can_view(&ctx)?;
    let mut year: Option<i16> = None;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "year" => year = Some(v.parse().map_err(|_| bad_param("year", "A year such as 2026"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({CALENDAR} ORDER BY c.year DESC, lower(c.name)), '[]') FROM app.holiday_calendars c WHERE $1::smallint IS NULL OR c.year = $1"
    ))
    .bind(year)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

// ---------- H2 POST /attendance/holiday-calendars ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCalendar {
    name: Option<String>,
    year: Option<i16>,
    copy_from_calendar_id: Option<Uuid>,
}

pub async fn create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/holiday-calendars", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewCalendar = parse(raw)?;
    let mut errs = Vec::new();
    let name = reason_of(req.name);
    if name.is_none() {
        errs.push(FieldError::new("name", "required", "Give the calendar a name"));
    }
    if !req.year.is_some_and(|y| (2000..=2100).contains(&y)) {
        errs.push(FieldError::new("year", "invalid_value", "A year from 2000 to 2100"));
    }
    check_item(&mut ctx.tx, "holiday_calendars", req.copy_from_calendar_id, "copy_from_calendar_id", &mut errs).await?;
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let year = req.year.unwrap();
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.holiday_calendars WHERE year = $1 AND lower(name) = lower($2))")
        .bind(year)
        .bind(&name)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if taken {
        return Err(AppError::conflict("name_in_use", "Name in use", "Another calendar for this year has this name").into());
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.holiday_calendars (org_id, year, name) VALUES ($1, $2, $3) RETURNING id")
        .bind(ctx.org_id)
        .bind(year)
        .bind(&name)
        .fetch_one(&mut *ctx.tx)
        .await?;
    // Copy: same day and month in the new year (AC-AT-06.6); 29 February is dropped in a non-leap year.
    if let Some(src) = req.copy_from_calendar_id {
        sqlx::query(
            "INSERT INTO app.holidays (org_id, calendar_id, holiday_date, name, kind)
             SELECT $1, $2, make_date($3, extract(month FROM holiday_date)::int, extract(day FROM holiday_date)::int), name, kind
             FROM app.holidays WHERE calendar_id = $4 AND removed_at IS NULL
               AND (to_char(holiday_date, 'MM-DD') <> '02-29' OR ($3 % 4 = 0 AND ($3 % 100 <> 0 OR $3 % 400 = 0)))",
        )
        .bind(ctx.org_id)
        .bind(id)
        .bind(year as i32)
        .bind(src)
        .execute(&mut *ctx.tx)
        .await?;
    }
    let doc = calendar_doc(&mut ctx.tx, id, false).await?;
    ctx.audit(MANAGE, "holiday_calendar", id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

// ---------- H3 GET /attendance/holiday-calendars/{id} ----------

pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    can_view(&ctx)?;
    let mut include_removed = false;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "include_removed" => include_removed = v == "true",
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let doc = calendar_doc(&mut ctx.tx, id, include_removed).await?;
    ctx.commit().await?;
    Ok(Json(doc))
}

// ---------- H4 holidays ----------

/// Validates a holiday's fields; `None` fields are required unless `old` gives them.
fn holiday_fields(m: &Map<String, Value>, year: i32, old: Option<(NaiveDate, String, String)>) -> ApiResult<(NaiveDate, String, String)> {
    if let Some(k) = m.keys().find(|k| !["date", "name", "kind"].contains(&k.as_str())) {
        return Err(AppError::unprocessable("unknown_field", "Request not valid", format!("unknown field `{k}`")).into());
    }
    let mut errs = Vec::new();
    let date = match m.get("date") {
        None => old.as_ref().map(|o| o.0),
        Some(v) => v.as_str().and_then(|s| s.parse().ok()),
    };
    let name = match m.get("name") {
        None => old.as_ref().map(|o| o.1.clone()),
        Some(v) => v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
    };
    let kind = match m.get("kind") {
        None => old.as_ref().map(|o| o.2.clone()),
        Some(v) => v.as_str().filter(|k| KINDS.contains(k)).map(str::to_string),
    };
    match date {
        None => errs.push(FieldError::new("date", "invalid_value", "Use YYYY-MM-DD")),
        Some(d) if d.year() != year => errs.push(FieldError::new("date", "date_outside_year", format!("The calendar is for {year}"))),
        _ => {}
    }
    if name.is_none() {
        errs.push(FieldError::new("name", "required", "Give the holiday a name"));
    }
    if kind.is_none() {
        errs.push(FieldError::new("kind", "invalid_value", "national, festival or optional"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    Ok((date.unwrap(), name.unwrap(), kind.unwrap()))
}

async fn insert_holiday(ctx: &mut Ctx, calendar: Uuid, (date, name, kind): &(NaiveDate, String, String)) -> ApiResult<Uuid> {
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.holidays WHERE calendar_id = $1 AND holiday_date = $2 AND removed_at IS NULL)")
        .bind(calendar)
        .bind(date)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if taken {
        return Err(AppError::conflict("holiday_exists_on_date", "Holiday exists on date", format!("The calendar already has a holiday on {date}")).into());
    }
    Ok(sqlx::query_scalar("INSERT INTO app.holidays (org_id, calendar_id, holiday_date, name, kind) VALUES ($1, $2, $3, $4, $5::app.holiday_kind) RETURNING id")
        .bind(ctx.org_id)
        .bind(calendar)
        .bind(date)
        .bind(name)
        .bind(kind)
        .fetch_one(&mut *ctx.tx)
        .await?)
}

async fn holiday_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    Ok(sqlx::query_scalar(&format!("SELECT {HOLIDAY} FROM app.holidays h WHERE h.id = $1")).bind(id).fetch_one(&mut **tx).await?)
}

/// The holiday as (date, name, kind) if it is in this calendar and not removed.
async fn current_holiday(tx: &mut Tx, calendar: Uuid, hid: Uuid) -> ApiResult<(NaiveDate, String, String)> {
    let row: Option<(NaiveDate, String, String)> =
        sqlx::query_as("SELECT holiday_date, name, kind::text FROM app.holidays WHERE id = $1 AND calendar_id = $2 AND removed_at IS NULL")
            .bind(hid)
            .bind(calendar)
            .fetch_optional(&mut **tx)
            .await?;
    row.ok_or_else(|| AppError::not_found().into())
}

async fn remove_holiday(tx: &mut Tx, hid: Uuid, by: Uuid) -> ApiResult<()> {
    sqlx::query("UPDATE app.holidays SET removed_at = now(), removed_by = $2 WHERE id = $1").bind(hid).bind(by).execute(&mut **tx).await?;
    Ok(())
}

pub async fn holiday_add(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/attendance/holiday-calendars/{id}/holidays"), &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let Value::Object(m) = raw else { return Err(bad_param("body", "Send a JSON object").into()) };
    let (year, branches) = lock_calendar(&mut ctx.tx, id, None).await?;
    let fields = holiday_fields(&m, year, None)?;
    let hid = insert_holiday(&mut ctx, id, &fields).await?;
    holiday_changed(&mut ctx, id, &branches, &[fields.0]).await?;
    let doc = holiday_doc(&mut ctx.tx, hid).await?;
    ctx.audit(MANAGE, "holiday", hid, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

/// A change is a replacement: the old holiday is removed and the new one gets a new id.
/// If-Match carries the calendar's version.
pub async fn holiday_patch(mut ctx: Ctx, Path((id, hid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let Value::Object(m) = raw else { return Err(bad_param("body", "Send a JSON object").into()) };
    let (year, branches) = lock_calendar(&mut ctx.tx, id, Some(expected)).await?;
    let old = current_holiday(&mut ctx.tx, id, hid).await?;
    let new = holiday_fields(&m, year, Some(old.clone()))?;
    if new == old {
        let doc = holiday_doc(&mut ctx.tx, hid).await?;
        ctx.commit().await?;
        return Ok(Json(doc).into_response());
    }
    remove_holiday(&mut ctx.tx, hid, ctx.user_id).await?;
    let nid = insert_holiday(&mut ctx, id, &new).await?;
    holiday_changed(&mut ctx, id, &branches, &[old.0, new.0]).await?;
    let before = holiday_doc(&mut ctx.tx, hid).await?;
    let doc = holiday_doc(&mut ctx.tx, nid).await?;
    ctx.audit(MANAGE, "holiday", nid, Some(before), Some(doc.clone()), None).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

pub async fn holiday_remove(mut ctx: Ctx, Path((id, hid)): Path<(Uuid, Uuid)>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let expected = if_match(&headers)?;
    let Some(reason) = reason_of(parse::<Reason>(raw)?.reason) else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    let (_, branches) = lock_calendar(&mut ctx.tx, id, Some(expected)).await?;
    let (date, ..) = current_holiday(&mut ctx.tx, id, hid).await?;
    remove_holiday(&mut ctx.tx, hid, ctx.user_id).await?;
    holiday_changed(&mut ctx, id, &branches, &[date]).await?;
    let doc = holiday_doc(&mut ctx.tx, hid).await?;
    ctx.audit(MANAGE, "holiday", hid, None, Some(doc.clone()), Some(&reason)).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

// ---------- H5 PUT /attendance/branches/{id}/holiday-calendars/{year} ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    calendar_id: Uuid,
}

pub async fn branch_link(mut ctx: Ctx, Path((branch, year)): Path<(Uuid, i16)>, Body(raw): Body) -> ApiResult<Json<Value>> {
    let scope = ctx.scope(MANAGE)?;
    if !(scope.org || scope.branches.contains(&branch)) {
        return Err(AppError::forbidden().into());
    }
    let req: Link = parse(raw)?;
    let mut errs = Vec::new();
    check_item(&mut ctx.tx, "branches", Some(branch), "branch_id", &mut errs).await?;
    check_item(&mut ctx.tx, "holiday_calendars", Some(req.calendar_id), "calendar_id", &mut errs).await?;
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (cal_year, _) = lock_calendar(&mut ctx.tx, req.calendar_id, None).await?;
    if cal_year != year as i32 {
        return Err(AppError::validation(vec![FieldError::new("calendar_id", "invalid_value", format!("That calendar is for {cal_year}"))]).into());
    }
    let old: Option<Uuid> = sqlx::query_scalar("SELECT calendar_id FROM app.branch_calendars WHERE branch_id = $1 AND year = $2 FOR UPDATE")
        .bind(branch)
        .bind(year)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let body = json!({ "branch_id": branch, "year": year, "calendar_id": req.calendar_id });
    if old != Some(req.calendar_id) {
        // Dates whose holiday differs between the two calendars are the ones this change touches.
        let holidays = |c: Uuid| sqlx::query_as::<_, (NaiveDate, String)>("SELECT holiday_date, kind::text FROM app.holidays WHERE calendar_id = $1 AND removed_at IS NULL").bind(c);
        let a: BTreeSet<(NaiveDate, String)> = match old {
            Some(c) => holidays(c).fetch_all(&mut *ctx.tx).await?.into_iter().collect(),
            None => BTreeSet::new(),
        };
        let b: BTreeSet<(NaiveDate, String)> = holidays(req.calendar_id).fetch_all(&mut *ctx.tx).await?.into_iter().collect();
        let dates: Vec<NaiveDate> = a.symmetric_difference(&b).map(|(d, _)| *d).collect::<BTreeSet<_>>().into_iter().collect();
        if let Some((unit, state, d)) = branch_lock(&mut ctx.tx, &[branch], &dates).await? {
            return Err(date_locked((unit, state), d));
        }
        sqlx::query(
            "INSERT INTO app.branch_calendars (org_id, branch_id, year, calendar_id) VALUES ($1, $2, $3, $4)
             ON CONFLICT (branch_id, year) DO UPDATE SET calendar_id = EXCLUDED.calendar_id, updated_at = now()",
        )
        .bind(ctx.org_id)
        .bind(branch)
        .bind(year)
        .bind(req.calendar_id)
        .execute(&mut *ctx.tx)
        .await?;
        ctx.audit(MANAGE, "branch", branch, Some(json!({ "year": year, "calendar_id": old })), Some(body.clone()), None).await?;
        if let (Some(from), Some(to)) = (dates.first(), dates.last()) {
            ctx.event("ScheduleChanged", "branch", branch, json!({ "branch_ids": [branch], "from": from, "to": to })).await?;
        }
    }
    ctx.commit().await?;
    Ok(Json(body))
}

// ---------- optional holiday picks ----------

/// HD-01 from the employee's resolved policy set on `on`.
async fn hd01(tx: &mut Tx, employee: Uuid, on: NaiveDate) -> ApiResult<i64> {
    Ok(crate::policies::settings_for(tx, employee, on).await?.optional_holidays_per_year as i64)
}

/// The calendar of the employee's master branch for `on`'s year.
async fn my_calendar(tx: &mut Tx, employee: Uuid, on: NaiveDate) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT bc.calendar_id FROM app.employee_job_versions v JOIN app.branch_calendars bc ON bc.branch_id = v.master_branch_id AND bc.year = $3
         WHERE v.employee_id = $1 AND v.state = 'active' AND v.valid @> $2::date",
    )
    .bind(employee)
    .bind(on)
    .bind(on.year() as i16)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn used(tx: &mut Tx, employee: Uuid, year: i32) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM app.optional_holiday_picks p JOIN app.holidays h ON h.id = p.holiday_id
         WHERE p.employee_id = $1 AND p.state IN ('pending', 'approved') AND extract(year FROM h.holiday_date) = $2",
    )
    .bind(employee)
    .bind(year)
    .fetch_one(&mut **tx)
    .await?)
}

fn me(ctx: &Ctx, permission: &str) -> ApiResult<Uuid> {
    ctx.scope(permission)?;
    ctx.employee_id.ok_or_else(|| AppError::forbidden().into())
}

async fn pick_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    Ok(sqlx::query_scalar(&format!("SELECT {PICK} FROM app.optional_holiday_picks p JOIN app.holidays h ON h.id = p.holiday_id WHERE p.id = $1"))
        .bind(id)
        .fetch_one(&mut **tx)
        .await?)
}

// H6 GET /attendance/me/optional-holidays — shown even when HD-01 = 0 (AC-AT-08.4).
pub async fn my_optional(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let employee = me(&ctx, REQUEST)?;
    let mut year = today().year();
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "year" => year = v.parse().map_err(|_| bad_param("year", "A year such as 2026"))?,
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let on = if year == today().year() { today() } else { NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(|| bad_param("year", "Not a year"))? };
    let allowed = hd01(&mut ctx.tx, employee, on).await?;
    let used = used(&mut ctx.tx, employee, year).await?;
    let holidays: Value = match my_calendar(&mut ctx.tx, employee, on).await? {
        None => json!([]),
        Some(c) => sqlx::query_scalar(
            "SELECT coalesce(jsonb_agg(jsonb_build_object('id', h.id, 'date', h.holiday_date, 'name', h.name,
               'pick', (SELECT jsonb_build_object('id', p.id, 'state', p.state) FROM app.optional_holiday_picks p
                        WHERE p.holiday_id = h.id AND p.employee_id = $2 ORDER BY p.created_at DESC LIMIT 1)) ORDER BY h.holiday_date), '[]')
             FROM app.holidays h WHERE h.calendar_id = $1 AND h.kind = 'optional' AND h.removed_at IS NULL",
        )
        .bind(c)
        .bind(employee)
        .fetch_one(&mut *ctx.tx)
        .await?,
    };
    ctx.commit().await?;
    Ok(Json(json!({ "year": year, "allowed": allowed, "used": used, "holidays": holidays })))
}

// H7 POST /attendance/optional-holiday-picks
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPick {
    holiday_id: Uuid,
}

// shortcut: "manager told" (NT) waits for the notification slice; the pick is visible to them in H8 meanwhile.
pub async fn pick(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let employee = me(&ctx, REQUEST)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/optional-holiday-picks", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: NewPick = parse(raw)?;
    let row: Option<(NaiveDate, String, Uuid)> =
        sqlx::query_as("SELECT holiday_date, kind::text, calendar_id FROM app.holidays WHERE id = $1 AND removed_at IS NULL")
            .bind(req.holiday_id)
            .fetch_optional(&mut *ctx.tx)
            .await?;
    let Some((date, kind, calendar)) = row else { return Err(AppError::not_found().into()) };
    // One employee's picks are counted and added one at a time (INV-AT-17).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))").bind(employee).execute(&mut *ctx.tx).await?;
    let allowed = hd01(&mut ctx.tx, employee, date).await?;
    if allowed == 0 {
        return Err(AppError::new(403, "feature_disabled", "Switched off", "Optional holidays are not allowed under your policy")
            .with_meta(json!({ "setting": "HD-01" }))
            .into());
    }
    if kind != "optional" {
        return Err(AppError::unprocessable("holiday_not_optional", "Not an optional holiday", "Only optional holidays can be picked").into());
    }
    if my_calendar(&mut ctx.tx, employee, date).await? != Some(calendar) {
        return Err(AppError::unprocessable("not_in_my_calendar", "Not in your calendar", "This holiday isn't in your branch's calendar").into());
    }
    let live: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.optional_holiday_picks WHERE employee_id = $1 AND holiday_id = $2 AND state IN ('pending', 'approved'))")
        .bind(employee)
        .bind(req.holiday_id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if live {
        return Err(AppError::conflict("already_picked", "Already picked", "You have already picked this holiday").into());
    }
    if used(&mut ctx.tx, employee, date.year()).await? >= allowed {
        return Err(AppError::unprocessable("pick_limit_reached", "Limit reached", format!("You can pick {allowed} optional holidays a year")).into());
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.optional_holiday_picks (org_id, employee_id, holiday_id) VALUES ($1, $2, $3) RETURNING id")
        .bind(ctx.org_id)
        .bind(employee)
        .bind(req.holiday_id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    let doc = pick_doc(&mut ctx.tx, id).await?;
    ctx.audit(REQUEST, "optional_holiday_pick", id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

// H8 GET /attendance/optional-holiday-picks — approvers see their scope, everyone else their own.
// shortcut: newest 200 only, no cursor; add one when a list grows past that.
pub async fn picks(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let (mut states, mut awaiting_me) = (Vec::new(), false);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "state" => states.push(v),
            "awaiting_me" => awaiting_me = v == "true",
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let as_of = today();
    let mut qb = QueryBuilder::new(format!("SELECT coalesce(jsonb_agg(x.doc ORDER BY x.created_at DESC), '[]') FROM (SELECT {PICK} AS doc, p.created_at"));
    if let Ok(scope) = ctx.scope(APPROVE) {
        let em = em_settings(&mut ctx.tx, as_of).await?;
        push_from(&mut qb, as_of);
        qb.push(" JOIN app.optional_holiday_picks p ON p.employee_id = e.id JOIN app.holidays h ON h.id = p.holiday_id WHERE ");
        push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    } else {
        let employee = me(&ctx, REQUEST)?;
        qb.push(" FROM app.optional_holiday_picks p JOIN app.holidays h ON h.id = p.holiday_id WHERE p.employee_id = ").push_bind(employee);
    }
    if !states.is_empty() {
        qb.push(" AND p.state::text = ANY(").push_bind(states).push(")");
    }
    if awaiting_me {
        qb.push(" AND p.state = 'pending' AND p.raised_by <> ").push_bind(ctx.user_id);
        if let Some(e) = ctx.employee_id {
            qb.push(" AND p.employee_id <> ").push_bind(e);
        }
    }
    qb.push(" ORDER BY p.created_at DESC LIMIT 200) x");
    let items: Value = qb.build().fetch_one(&mut *ctx.tx).await?.get(0);
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

/// Locks a pick: (employee, state, raised_by, holiday date, version).
async fn lock_pick(tx: &mut Tx, id: Uuid) -> ApiResult<(Uuid, String, Option<Uuid>, NaiveDate, i32)> {
    let row = sqlx::query_as(
        "SELECT p.employee_id, p.state::text, p.raised_by, h.holiday_date, p.row_version FROM app.optional_holiday_picks p
         JOIN app.holidays h ON h.id = p.holiday_id WHERE p.id = $1 FOR UPDATE OF p",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| AppError::not_found().into())
}

// H9 approve / reject
async fn decide(mut ctx: Ctx, id: Uuid, headers: HeaderMap, raw: Value, to: &str) -> ApiResult<Response> {
    ctx.scope(APPROVE)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let (employee, state, raised_by, date, version) = lock_pick(&mut ctx.tx, id).await?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, employee, APPROVE, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    if state != to {
        if state != "pending" {
            return Err(AppError::conflict("not_pending", "Already decided", format!("This pick is {state}")).into());
        }
        let expected = if_match(&headers)?;
        if expected != version {
            return Err(stale(version));
        }
        if raised_by == Some(ctx.user_id) || ctx.employee_id == Some(employee) {
            return Err(AppError::new(403, "cannot_decide_own_request", "Not your own request", "Someone else must decide a request you raised").into());
        }
        if to == "rejected" && reason.is_none() {
            return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
        }
        sqlx::query(
            "UPDATE app.optional_holiday_picks SET state = $2::app.pick_state, decided_by = $3, decided_at = now(), decision_reason = $4,
               row_version = row_version + 1, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(to)
        .bind(ctx.user_id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
        ctx.audit(APPROVE, "optional_holiday_pick", id, Some(json!({ "state": state })), Some(json!({ "state": to })), reason.as_deref()).await?;
        if to == "approved" {
            ctx.event("ScheduleChanged", "employee", employee, json!({ "employee_ids": [employee], "from": date, "to": date })).await?;
        }
    }
    let doc = pick_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

pub async fn approve(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "approved").await
}
pub async fn reject(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "rejected").await
}

// H10 withdraw (pending) / cancel (approved, before the date) — the employee only.
async fn take_back(mut ctx: Ctx, id: Uuid, headers: HeaderMap, raw: Value, from: &str, to: &str) -> ApiResult<Response> {
    let me = me(&ctx, REQUEST)?;
    let reason = reason_of(parse::<Reason>(raw)?.reason);
    let (employee, state, _, date, version) = lock_pick(&mut ctx.tx, id).await?;
    if employee != me {
        return Err(AppError::not_found().into());
    }
    if state != to {
        if state != from {
            return Err(AppError::conflict("not_pending", "Can't be changed", format!("This pick is {state}")).into());
        }
        let expected = if_match(&headers)?;
        if expected != version {
            return Err(stale(version));
        }
        if to == "cancelled" && date <= today() {
            return Err(AppError::conflict("date_passed", "Date passed", format!("{date} has passed; it can no longer be cancelled")).into());
        }
        sqlx::query("UPDATE app.optional_holiday_picks SET state = $2::app.pick_state, row_version = row_version + 1, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(to)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.audit(REQUEST, "optional_holiday_pick", id, Some(json!({ "state": state })), Some(json!({ "state": to })), reason.as_deref()).await?;
        if to == "cancelled" {
            ctx.event("ScheduleChanged", "employee", employee, json!({ "employee_ids": [employee], "from": date, "to": date })).await?;
        }
    }
    let doc = pick_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

pub async fn withdraw(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    take_back(ctx, id, headers, raw, "pending", "withdrawn").await
}
pub async fn cancel(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    take_back(ctx, id, headers, raw, "approved", "cancelled").await
}
