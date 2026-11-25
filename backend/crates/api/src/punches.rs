//! Punching P1–P2 and punch administration P4–P9 (API_CONTRACTS Package 2 §5.4–5.5, US-AT-14–19).
//! Raw punches never change (BR-01): reviews, adjudications and voids are separate records, and the day
//! is recalculated from a `PunchRecorded` event.
// shortcut: a punch, review or adjudication on a locked date is stored and announced but not yet posted as
// a post-lock adjustment (§1.3 "Adjust"); that arrives with periods and lock (S9, on hold).

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{allowed, bad_param, em_settings, push_from, push_scope, query_pairs, require_edit, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::{FromRequest, Multipart, Path, RawQuery, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use payroll_core::policy::{AtSettings, OffDayPunch};
use payroll_core::punches::{self as rules, check_location, effective_time, Fence, Location, Verdict};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

const VIEW: &str = "attendance.punch.view";
const REVIEW: &str = "attendance.punch.review";
const ADJUDICATE: &str = "attendance.punch.adjudicate";
const MANUAL: &str = "attendance.punch.manual";
const MAX_SELFIE: usize = 1024 * 1024;

/// §2.1 Punch for a raw punch `p` (joins `e` employee, `b` branch where made, `dv` device).
const RAW: &str = "jsonb_build_object('id', p.id,
    'employee', CASE WHEN e.id IS NULL THEN NULL ELSE jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name) END,
    'source', p.source, 'direction', p.direction, 'punched_at', p.punched_at,
    'local_time', p.punched_at AT TIME ZONE b.time_zone, 'local_zone', b.time_zone, 'source_time', p.source_time, 'received_at', p.received_at,
    'branch', CASE WHEN b.id IS NULL THEN NULL ELSE jsonb_build_object('id', b.id, 'short_code', b.short_code) END,
    'device', CASE WHEN dv.id IS NULL THEN NULL ELSE jsonb_build_object('id', dv.id, 'serial', dv.serial) END, 'enrolment_id', p.enrolment_id,
    'location', CASE WHEN p.latitude IS NULL THEN NULL ELSE jsonb_build_object('latitude', p.latitude, 'longitude', p.longitude,
                'accuracy_m', p.accuracy_m, 'fence_distance_m', p.fence_distance_m) END,
    'selfie_url', NULL, 'has_selfie', p.selfie_ref IS NOT NULL, 'flags', to_jsonb(p.flags),
    'review', (SELECT jsonb_build_object('id', r.id, 'state', r.state, 'version', r.row_version) FROM app.punch_reviews r WHERE r.punch_id = p.id),
    'adjudications', (SELECT coalesce(jsonb_agg(jsonb_build_object('kind', a.kind, 'new_work_date', a.new_work_date, 'new_direction', a.new_direction,
                      'reason', a.reason, 'by', a.created_by, 'at', a.created_at) ORDER BY a.created_at), '[]') FROM app.punch_adjudications a WHERE a.punch_id = p.id),
    'counted', NOT coalesce((SELECT a.kind = 'exclude' FROM app.punch_adjudications a WHERE a.punch_id = p.id AND a.kind IN ('exclude', 'include')
                             ORDER BY a.created_at DESC LIMIT 1), false),
    'voided', false)";
const RAW_FROM: &str = "FROM app.raw_punches p LEFT JOIN app.employees e ON e.id = p.employee_id
    LEFT JOIN app.branches b ON b.id = p.branch_id LEFT JOIN app.devices dv ON dv.id = p.device_id";
/// The same shape for a manual punch `m`.
const MANUAL_DOC: &str = "jsonb_build_object('id', m.id,
    'employee', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
    'source', 'manual', 'origin', m.origin, 'direction', m.direction, 'punched_at', m.punched_at,
    'local_time', m.punched_at AT TIME ZONE b.time_zone, 'local_zone', b.time_zone, 'source_time', m.punched_at, 'received_at', m.created_at,
    'branch', CASE WHEN b.id IS NULL THEN NULL ELSE jsonb_build_object('id', b.id, 'short_code', b.short_code) END,
    'device', NULL, 'enrolment_id', NULL, 'location', NULL, 'selfie_url', NULL, 'has_selfie', false, 'flags', '[]'::jsonb,
    'review', NULL, 'adjudications', '[]'::jsonb, 'reason', m.reason, 'created_by', m.created_by,
    'counted', m.voided_at IS NULL, 'voided', m.voided_at IS NOT NULL, 'void_reason', m.void_reason)";
const MANUAL_FROM: &str = "FROM app.manual_punches m JOIN app.employees e ON e.id = m.employee_id LEFT JOIN app.branches b ON b.id = m.branch_id";

async fn punch_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("SELECT {RAW} {RAW_FROM} WHERE p.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

/// The employee's placement at instant `at`: (job version, master branch, its time zone, local work date).
pub(crate) async fn placement(tx: &mut Tx, employee: Uuid, at: DateTime<Utc>) -> ApiResult<Option<(Uuid, Uuid, String, NaiveDate)>> {
    Ok(sqlx::query_as(
        "SELECT jv.id, jv.master_branch_id, b.time_zone, ($2 AT TIME ZONE b.time_zone)::date
         FROM app.employee_job_versions jv JOIN app.branches b ON b.id = jv.master_branch_id
         WHERE jv.employee_id = $1 AND jv.state = 'active' AND jv.valid @> ($2 AT TIME ZONE b.time_zone)::date",
    )
    .bind(employee)
    .bind(at)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Is `date` a weekly off, a holiday or a day without a shift for the employee (PC-06)?
async fn off_day(tx: &mut Tx, employee: Uuid, date: NaiveDate, s: &AtSettings) -> ApiResult<bool> {
    Ok(crate::days::schedule(tx, employee, date, s).await?.is_none_or(|x| x.holiday || x.weekly_off || x.shift.is_none()))
}

fn disabled(setting: &str, detail: &str) -> crate::error::ApiError {
    AppError::new(403, "feature_disabled", "Switched off", detail.to_string()).with_meta(json!({ "setting": setting })).into()
}

// ---------- P1 POST /attendance/me/punches ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Loc {
    latitude: f64,
    longitude: f64,
    accuracy_m: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dev {
    id: String,
    #[allow(dead_code)]
    platform: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    direction: String,
    client_time: Option<DateTime<Utc>>,
    uptime_time: Option<DateTime<Utc>>,
    #[serde(default)]
    queued: bool,
    client_key: String,
    location: Option<Loc>,
    device: Option<Dev>,
}

/// Reads `meta` (and `selfie`) from multipart, or the whole body as `meta` when it is JSON.
async fn read_punch(state: &AppState, req: Request) -> ApiResult<(Value, Option<axum::body::Bytes>)> {
    let multipart = req.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("multipart/"));
    let unreadable = || AppError::unprocessable("unreadable_body", "Request not readable", "Send meta as JSON and the selfie as a JPEG");
    if !multipart {
        let Body(v) = Body::from_request(req, state).await?;
        return Ok((v, None));
    }
    let mut form = Multipart::from_request(req, state).await.map_err(|_| unreadable())?;
    let (mut meta, mut selfie) = (None, None);
    while let Some(field) = form.next_field().await.map_err(|_| unreadable())? {
        match field.name() {
            Some("meta") => meta = Some(serde_json::from_slice(&field.bytes().await.map_err(|_| unreadable())?).map_err(|_| unreadable())?),
            Some("selfie") => selfie = Some(field.bytes().await.map_err(|_| unreadable())?),
            _ => {}
        }
    }
    Ok((meta.ok_or_else(unreadable)?, selfie))
}

pub async fn punch(State(state): State<AppState>, mut ctx: Ctx, req: Request) -> ApiResult<Response> {
    ctx.scope("attendance.punch.self")?;
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    let ip = crate::rate::client_ip(req.headers());
    let (raw, selfie) = read_punch(&state, req).await?;
    let m: Meta = parse(raw)?;
    let mut errs = Vec::new();
    if m.direction != "in" && m.direction != "out" {
        errs.push(FieldError::new("direction", "invalid_value", "in or out"));
    }
    if m.client_key.trim().is_empty() || m.client_key.len() > 100 {
        errs.push(FieldError::new("client_key", "invalid_value", "A unique key for this punch, up to 100 characters"));
    }
    let app = m.device.is_some();
    if app && m.client_time.is_none() {
        errs.push(FieldError::new("client_time", "required", "Send the phone's time"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let source = if app { "app" } else { "web" };
    // A repeat of the same offline punch returns the stored one (AC-AT-14.6).
    let key = m.client_key.trim().to_string();
    let seen: Option<Uuid> = sqlx::query_scalar("SELECT punch_id FROM app.punch_keys WHERE source = $1::app.punch_source AND idempotency_key = $2")
        .bind(source)
        .bind(&key)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    if let Some(id) = seen {
        let doc = punch_doc(&mut ctx.tx, id).await?;
        ctx.commit().await?;
        return Ok((StatusCode::OK, Json(json!({ "punch": doc, "flags": doc["flags"], "review_opened": !doc["review"].is_null(), "repeat": true }))).into_response());
    }

    let received = Utc::now();
    let defaults = AtSettings::default();
    // The web punch is at server time; the app punch follows the time rules (DEC-21, §3.4).
    let (at, mut flags) = if app { effective_time(m.client_time.unwrap(), m.uptime_time, received, m.queued, &defaults) } else { (received, vec![]) };
    let Some((job, master, _zone, work_date)) = placement(&mut ctx.tx, me, at).await? else {
        return Err(AppError::unprocessable("outside_employment", "Not employed then", "There is no employment on that date").into());
    };
    let s = crate::policies::settings_for(&mut ctx.tx, me, work_date).await?;
    if app {
        // Time flags with the employee's own PC-07/PC-08.
        flags = effective_time(m.client_time.unwrap(), m.uptime_time, received, m.queued, &s).1;
    }
    if (app && !s.punch_app) || (!app && !s.punch_web) {
        return Err(disabled(if app { "PC-01.app" } else { "PC-01.web" }, "Punching from here is switched off"));
    }
    if let Some(d) = &m.device {
        let bound: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM app.user_devices WHERE user_id = $1 AND device_identifier = $2 AND bound_at IS NOT NULL AND unbound_at IS NULL)",
        )
        .bind(ctx.user_id)
        .bind(d.id.trim())
        .fetch_one(&mut *ctx.tx)
        .await?;
        if !bound {
            return Err(AppError::new(403, "device_not_bound", "Phone not bound", "Punches are accepted only from the phone bound to you; ask HR to reset it").into());
        }
    }
    // PC-05 / PC-15: the web IP list; with "either", a good IP makes the location check unnecessary.
    let mut skip_location = false;
    if !app && s.web_ip_restricted {
        let ok: bool = sqlx::query_scalar("SELECT coalesce($1::inet <<= ANY($2::cidr[]), false)")
            .bind(&ip)
            .bind(&s.web_allowed_ips)
            .fetch_one(&mut *ctx.tx)
            .await
            .unwrap_or(false);
        let either = s.web_location_and_ip == payroll_core::policy::BothOrEither::Either && s.web_requires_location;
        match (ok, either) {
            (true, true) => skip_location = true,
            (false, false) => return Err(AppError::new(403, "ip_not_allowed", "Network not allowed", "Web punches are allowed only from the organisation's network").into()),
            _ => {}
        }
    }
    // Fences in force at the punch time, marked allowed for the master (and allowed) branches (PC-16, BR-AT-09).
    let rows: Vec<(Uuid, Uuid, f64, f64, f64, bool)> = sqlx::query_as(
        "SELECT v.id, f.branch_id, v.latitude::float8, v.longitude::float8, v.radius_m::float8,
                f.branch_id = $2 OR ($4 AND EXISTS (SELECT 1 FROM app.job_version_allowed_branches ab WHERE ab.job_version_id = $3 AND ab.branch_id = f.branch_id))
         FROM app.geo_fences f
         JOIN LATERAL (SELECT * FROM app.geo_fence_versions w WHERE w.fence_id = f.id AND w.valid_from <= $1 ORDER BY w.valid_from DESC LIMIT 1) v ON v.in_use",
    )
    .bind(at)
    .bind(master)
    .bind(job)
    .bind(s.fences_apply == payroll_core::policy::FenceScope::MasterAndAllowed)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let fences: Vec<Fence<(Uuid, Uuid)>> = rows
        .iter()
        .map(|(v, b, la, lo, r, ok)| Fence { id: (*v, *b), branch: (*b, *b), latitude: *la, longitude: *lo, radius_m: *r, allowed: *ok })
        .collect();
    let loc = m.location.as_ref().map(|l| Location { latitude: l.latitude, longitude: l.longitude, accuracy_m: l.accuracy_m });
    let mut nearest = None;
    if !skip_location {
        match check_location(!app, loc.as_ref(), &fences, &s) {
            Verdict::Refused("location_required") => return Err(AppError::unprocessable("location_required", "Location required", "Allow location to punch").into()),
            Verdict::Refused("outside_fence") if !app && s.web_ip_restricted => {
                return Err(AppError::new(403, "ip_not_allowed", "Network not allowed", "Punch from the organisation's network or inside a branch fence").into())
            }
            Verdict::Refused("outside_fence") => return Err(AppError::unprocessable("outside_fence", "Outside the fence", "You are outside every allowed fence").into()),
            Verdict::Refused(_) => return Err(AppError::unprocessable("poor_accuracy", "Location not accurate", "The location reading is not accurate enough").into()),
            Verdict::Accepted { flags: f, nearest: n } => {
                flags.extend(f);
                nearest = n;
            }
        }
    }
    if app && s.app_requires_selfie && selfie.is_none() {
        return Err(AppError::unprocessable("selfie_required", "Selfie required", "Take a selfie to punch").into());
    }
    if let Some(b) = &selfie {
        if b.len() > MAX_SELFIE || !b.starts_with(&[0xFF, 0xD8]) {
            return Err(AppError::validation(vec![FieldError::new("selfie", "invalid_value", "A JPEG up to 1 MB")]).into());
        }
    }
    if s.punch_on_off_day != OffDayPunch::Allow && off_day(&mut ctx.tx, me, work_date, &s).await? {
        if s.punch_on_off_day == OffDayPunch::Block {
            return Err(disabled("PC-06", "Punching on a weekly off, holiday or without a shift is switched off"));
        }
        flags.push(rules::OFF_DAY);
    }

    let id = Uuid::new_v4();
    let claimed: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO app.punch_keys (org_id, source, idempotency_key, punch_id, punched_at) VALUES ($1, $2::app.punch_source, $3, $4, $5)
         ON CONFLICT DO NOTHING RETURNING punch_id",
    )
    .bind(ctx.org_id)
    .bind(source)
    .bind(&key)
    .bind(id)
    .bind(at)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    if claimed.is_none() {
        return Err(AppError::conflict("punch_in_progress", "Already received", "This punch is being saved; send it again to get it").into());
    }
    // shortcut: selfies sit on local disk like imports (TSD §11 Cloud Storage at deployment); selfie_url stays
    // null until signed links exist, and the 13-month deletion (DEC-51) comes with the cleanup job.
    let selfie_ref = match &selfie {
        Some(b) => {
            let r = format!("{}/selfies/{}/{id}.jpg", ctx.org_id, at.format("%Y-%m"));
            let path = state.files_dir.join(&r);
            tokio::fs::create_dir_all(path.parent().unwrap()).await.map_err(|_| AppError::internal())?;
            tokio::fs::write(&path, b).await.map_err(|_| AppError::internal())?;
            Some(r)
        }
        None => None,
    };
    flags.sort_unstable();
    flags.dedup();
    let flag_list: Vec<String> = flags.iter().map(|f| f.to_string()).collect();
    let (fence_version, branch, distance) = match nearest {
        Some(((v, b), _, d)) => (Some(v), b, Some(d)),
        None => (None, master, None),
    };
    sqlx::query(
        "INSERT INTO app.raw_punches (id, org_id, punched_at, source, employee_id, direction, source_time, uptime_time, received_at, branch_id,
                                      latitude, longitude, accuracy_m, fence_distance_m, fence_version_id, selfie_ref, client_ip, idempotency_key, flags)
         VALUES ($1, $2, $3, $4::app.punch_source, $5, $6::app.punch_direction, $7, $8, $9, $10, $11::float8, $12::float8, $13::float8, $14::float8,
                 $15, $16, $17::inet, $18, $19::app.punch_flag[])",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(at)
    .bind(source)
    .bind(me)
    .bind(&m.direction)
    .bind(if app { m.client_time.unwrap() } else { received })
    .bind(m.uptime_time)
    .bind(received)
    .bind(branch)
    .bind(loc.as_ref().map(|l| l.latitude))
    .bind(loc.as_ref().map(|l| l.longitude))
    .bind(loc.as_ref().map(|l| l.accuracy_m))
    .bind(distance.map(|d| (d * 10.0).round() / 10.0))
    .bind(fence_version)
    .bind(selfie_ref)
    .bind((!app).then_some(ip).filter(|i| i.parse::<std::net::IpAddr>().is_ok()))
    .bind(&key)
    .bind(&flag_list)
    .execute(&mut *ctx.tx)
    .await?;
    // A flagged punch counts provisionally and waits for review (DEC-18).
    if !flag_list.is_empty() {
        sqlx::query(
            "INSERT INTO app.punch_reviews (org_id, punch_id, punched_at, employee_id, flags, punched_by) VALUES ($1, $2, $3, $4, $5::app.punch_flag[], $6)",
        )
        .bind(ctx.org_id)
        .bind(id)
        .bind(at)
        .bind(me)
        .bind(&flag_list)
        .bind(ctx.user_id)
        .execute(&mut *ctx.tx)
        .await?;
    }
    ctx.event("PunchRecorded", "employee", me, json!({ "employee_id": me, "work_date": work_date, "punch_id": id })).await?;
    let doc = punch_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "punch": doc, "flags": flag_list, "review_opened": !flag_list.is_empty(), "recalculation": "queued" }))).into_response())
}

// ---------- P2 GET /attendance/me/punches/today ----------

pub async fn today_status(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    ctx.scope("attendance.punch.self")?;
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    let Some((_, _, zone, date)) = placement(&mut ctx.tx, me, Utc::now()).await? else {
        ctx.commit().await?;
        return Ok(Json(json!({ "date": null, "punches": [], "next_direction": "in", "shift": null })));
    };
    let s = crate::policies::settings_for(&mut ctx.tx, me, date).await?;
    let punches: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg(x.doc ORDER BY x.at), '[]') FROM (
           SELECT {RAW} AS doc, p.punched_at AS at {RAW_FROM} WHERE p.employee_id = $1 AND (p.punched_at AT TIME ZONE $3)::date = $2
           UNION ALL
           SELECT {MANUAL_DOC}, m.punched_at {MANUAL_FROM} WHERE m.employee_id = $1 AND m.voided_at IS NULL AND (m.punched_at AT TIME ZONE $3)::date = $2) x"
    ))
    .bind(me)
    .bind(date)
    .bind(&zone)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let last = punches.as_array().and_then(|a| a.iter().rev().find(|p| p["counted"] == true)).map(|p| p["direction"].clone());
    let shift: Option<Value> = sqlx::query_scalar(
        "SELECT CASE WHEN re.is_weekly_off THEN jsonb_build_object('is_weekly_off', true) ELSE
                jsonb_build_object('id', s.id, 'code', s.code, 'name', s.name, 'start', v.start_time, 'end', v.end_time, 'is_weekly_off', false) END
         FROM app.employee_job_versions jv
         LEFT JOIN app.roster_entries re ON re.employee_id = $1 AND re.work_date = $2 AND re.superseded_at IS NULL
         LEFT JOIN app.shifts s ON s.id = CASE WHEN re.id IS NOT NULL THEN re.shift_id ELSE jv.default_shift_id END
         LEFT JOIN LATERAL (SELECT * FROM app.shift_versions w WHERE w.shift_id = s.id AND w.state = 'approved' AND w.valid_from <= $2
                            ORDER BY w.valid_from DESC LIMIT 1) v ON true
         WHERE jv.employee_id = $1 AND jv.state = 'active' AND jv.valid @> $2::date AND (re.is_weekly_off OR s.id IS NOT NULL)",
    )
    .bind(me)
    .bind(date)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({
        "date": date, "punches": punches, "next_direction": if last == Some(json!("in")) { "out" } else { "in" }, "shift": shift,
        "features": { "punch": { "app": s.punch_app, "web": s.punch_web, "location_required": s.app_requires_location,
                                 "web_location_required": s.web_requires_location, "selfie_required": s.app_requires_selfie } },
    })))
}

// ---------- P4 GET /attendance/punches ----------

pub async fn list(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope(VIEW)?;
    let (mut from, mut to, mut employee, mut branch, mut source, mut flag, mut review, mut cursor) = (None, None, None, None, None, None, None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "from" => from = Some(v.parse::<NaiveDate>().map_err(|_| bad_param("from", "Use YYYY-MM-DD"))?),
            "to" => to = Some(v.parse::<NaiveDate>().map_err(|_| bad_param("to", "Use YYYY-MM-DD"))?),
            "employee_id" => employee = Some(v.parse::<Uuid>().map_err(|_| bad_param("employee_id", "Not an id"))?),
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            "source" => source = Some(v),
            "flag" => flag = Some(v),
            "review" => review = Some(v),
            "cursor" => cursor = Some(v),
            // shortcut: unmapped device punches come with the device gateway (S11, on hold).
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let (Some(from), Some(to)) = (from, to) else {
        return Err(AppError::validation(vec![FieldError::new("from", "required", "Give from and to dates")]).into());
    };
    if to < from || (to - from).num_days() >= 62 {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", "to must be on or after from, at most 62 days").into());
    }
    let as_of = today();
    let em = em_settings(&mut ctx.tx, as_of).await?;
    // Both kinds of punch, then the caller's scope on their employee; dates are UTC-day bounds widened by a day.
    let (lo, hi) = (from.and_time(NaiveTime::MIN).and_utc() - Duration::days(1), to.and_time(NaiveTime::MIN).and_utc() + Duration::days(2));
    let mut qb = QueryBuilder::new("SELECT x.doc, x.at, x.id");
    push_from(&mut qb, as_of);
    qb.push(format!(
        " JOIN (SELECT {RAW} AS doc, p.punched_at AS at, p.id, p.employee_id AS emp, p.source::text AS src, p.branch_id AS br,
                       p.flags::text[] AS fl, (SELECT r.state::text FROM app.punch_reviews r WHERE r.punch_id = p.id) AS rv,
                       (p.punched_at AT TIME ZONE b.time_zone)::date AS d {RAW_FROM} WHERE p.punched_at >= "
    ))
    .push_bind(lo)
    .push(" AND p.punched_at < ")
    .push_bind(hi)
    .push(format!(
        " UNION ALL SELECT {MANUAL_DOC}, m.punched_at, m.id, m.employee_id, 'manual', m.branch_id, '{{}}'::text[], NULL,
                       (m.punched_at AT TIME ZONE b.time_zone)::date {MANUAL_FROM} WHERE m.punched_at >= "
    ))
    .push_bind(lo)
    .push(" AND m.punched_at < ")
    .push_bind(hi)
    .push(") x ON x.emp = e.id WHERE x.d BETWEEN ")
    .push_bind(from)
    .push(" AND ")
    .push_bind(to)
    .push(" AND ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    if let Some(e) = employee {
        qb.push(" AND x.emp = ").push_bind(e);
    }
    if let Some(b) = branch {
        qb.push(" AND x.br = ").push_bind(b);
    }
    if let Some(s) = source {
        qb.push(" AND x.src = ").push_bind(s);
    }
    if let Some(f) = flag {
        qb.push(" AND ").push_bind(f).push(" = ANY(x.fl)");
    }
    if let Some(r) = review {
        qb.push(" AND x.rv = ").push_bind(r);
    }
    if let Some(c) = cursor {
        let (ts, id) = c.split_once('|').ok_or_else(|| bad_param("cursor", "Not a valid cursor"))?;
        let ts: DateTime<Utc> = ts.parse().map_err(|_| bad_param("cursor", "Not a valid cursor"))?;
        let id: Uuid = id.parse().map_err(|_| bad_param("cursor", "Not a valid cursor"))?;
        qb.push(" AND (x.at, x.id) < (").push_bind(ts).push(", ").push_bind(id).push(")");
    }
    qb.push(" ORDER BY x.at DESC, x.id DESC LIMIT 101");
    let rows = qb.build().fetch_all(&mut *ctx.tx).await?;
    let more = rows.len() > 100;
    let rows = &rows[..rows.len().min(100)];
    let items: Vec<Value> = rows.iter().map(|r| r.get("doc")).collect();
    let next = more.then(|| rows.last().map(|r| format!("{}|{}", r.get::<DateTime<Utc>, _>("at").to_rfc3339(), r.get::<Uuid, _>("id")))).flatten();
    ctx.commit().await?;
    Ok(Json(json!({ "items": items, "next_cursor": next })))
}

// ---------- P5 / P6 reviews ----------

const REVIEW_DOC: &str = "jsonb_build_object('id', r.id, 'state', r.state, 'flags', to_jsonb(r.flags), 'opened_at', r.created_at,
    'decided_by', r.decided_by, 'decided_at', r.decided_at, 'decision_reason', r.decision_reason, 'version', r.row_version)";

pub async fn reviews(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope(REVIEW)?;
    let (mut state, mut branch, mut awaiting) = (None, None, false);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "state" => state = Some(v),
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            "awaiting_me" => awaiting = v == "true",
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let as_of = today();
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(format!("SELECT coalesce(jsonb_agg(y.doc ORDER BY y.at), '[]') FROM (SELECT {REVIEW_DOC} || jsonb_build_object('punch', {RAW}) AS doc, r.created_at AS at"));
    push_from(&mut qb, as_of);
    qb.push(" JOIN app.punch_reviews r ON r.employee_id = e.id JOIN app.raw_punches p ON p.id = r.punch_id AND p.punched_at = r.punched_at
              LEFT JOIN app.branches b ON b.id = p.branch_id LEFT JOIN app.devices dv ON dv.id = p.device_id WHERE ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    // The caller's own punches are never in their queue (DEC-18).
    qb.push(" AND r.punched_by IS DISTINCT FROM ").push_bind(ctx.user_id);
    if let Some(me) = ctx.employee_id {
        qb.push(" AND r.employee_id <> ").push_bind(me);
    }
    if let Some(s) = state.or(awaiting.then(|| "pending".to_string())) {
        qb.push(" AND r.state::text = ").push_bind(s);
    }
    if let Some(b) = branch {
        qb.push(" AND p.branch_id = ").push_bind(b);
    }
    qb.push(" ORDER BY r.created_at LIMIT 200) y");
    let items: Value = qb.build().fetch_one(&mut *ctx.tx).await?.get(0);
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

fn reason_of(raw: Value) -> ApiResult<Option<String>> {
    Ok(parse::<Reason>(raw)?.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()))
}

async fn decide(mut ctx: Ctx, id: Uuid, headers: HeaderMap, raw: Value, to: &str) -> ApiResult<Response> {
    ctx.scope(REVIEW)?;
    let reason = reason_of(raw)?;
    type ReviewRow = (Uuid, DateTime<Utc>, Uuid, String, Option<Uuid>, i32);
    let row: Option<ReviewRow> = sqlx::query_as(
        "SELECT punch_id, punched_at, employee_id, state::text, punched_by, row_version FROM app.punch_reviews WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some((punch, at, employee, state, punched_by, version)) = row else { return Err(AppError::not_found().into()) };
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, employee, REVIEW, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    if state != to {
        if state != "pending" {
            return Err(AppError::conflict("not_pending", "Already decided", format!("This review is {state}")).into());
        }
        if if_match(&headers)? != version {
            return Err(stale(version));
        }
        if punched_by == Some(ctx.user_id) || ctx.employee_id == Some(employee) {
            return Err(AppError::new(403, "cannot_decide_own_request", "Not your own punch", "Someone else must decide a punch you made").into());
        }
        if to == "rejected" && reason.is_none() {
            return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
        }
        sqlx::query(
            "UPDATE app.punch_reviews SET state = $2::app.review_state, decided_by = $3, decided_at = now(), decision_reason = $4, row_version = row_version + 1
             WHERE id = $1",
        )
        .bind(id)
        .bind(to)
        .bind(ctx.user_id)
        .bind(&reason)
        .execute(&mut *ctx.tx)
        .await?;
        // Rejecting excludes the punch through an adjudication; the raw punch is unchanged (AC-AT-16.3).
        if to == "rejected" {
            sqlx::query(
                "INSERT INTO app.punch_adjudications (org_id, punch_id, punched_at, employee_id, kind, origin, reason) VALUES ($1, $2, $3, $4, 'exclude', 'review', $5)",
            )
            .bind(ctx.org_id)
            .bind(punch)
            .bind(at)
            .bind(employee)
            .bind(&reason)
            .execute(&mut *ctx.tx)
            .await?;
        }
        let date = placement(&mut ctx.tx, employee, at).await?.map(|p| p.3);
        ctx.audit(REVIEW, "punch_review", id, Some(json!({ "state": state })), Some(json!({ "state": to })), reason.as_deref()).await?;
        // shortcut: the employee is told the outcome (NT-08) with notifications (S10).
        ctx.event("PunchRecorded", "employee", employee, json!({ "employee_id": employee, "work_date": date, "punch_id": punch })).await?;
    }
    let doc: Value = sqlx::query_scalar(&format!("SELECT {REVIEW_DOC} FROM app.punch_reviews r WHERE r.id = $1")).bind(id).fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}

pub async fn approve(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "approved").await
}
pub async fn reject(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    decide(ctx, id, headers, raw, "rejected").await
}

// ---------- P7 adjudications ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adjudication {
    kind: String,
    new_work_date: Option<NaiveDate>,
    new_direction: Option<String>,
    reason: Option<String>,
}

pub async fn adjudicate(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/attendance/punches/{id}/adjudications"), &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: Adjudication = parse(raw)?;
    let row: Option<(DateTime<Utc>, Option<Uuid>, String)> =
        sqlx::query_as("SELECT punched_at, employee_id, direction::text FROM app.raw_punches WHERE id = $1").bind(id).fetch_optional(&mut *ctx.tx).await?;
    let Some((at, Some(employee), direction)) = row else { return Err(AppError::not_found().into()) };
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, employee, ADJUDICATE, &em).await?;
    let reason = req.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let mut errs = Vec::new();
    if reason.is_none() {
        errs.push(FieldError::new("reason", "required", "Give a reason"));
    }
    if !["exclude", "include", "move_date", "change_direction"].contains(&req.kind.as_str()) {
        errs.push(FieldError::new("kind", "invalid_value", "exclude, include, move_date or change_direction"));
    }
    if (req.kind == "move_date") != req.new_work_date.is_some() {
        errs.push(FieldError::new("new_work_date", "invalid_value", "Give the new date with move_date only"));
    }
    if (req.kind == "change_direction") != req.new_direction.as_deref().is_some_and(|d| d == "in" || d == "out") {
        errs.push(FieldError::new("new_direction", "invalid_value", "Give in or out with change_direction only"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    // The punch as adjudicated so far: counted, its date and its direction.
    let base_date = placement(&mut ctx.tx, employee, at).await?.map(|p| p.3).unwrap_or(at.date_naive());
    let (counted, cur_date, cur_dir): (bool, NaiveDate, String) = sqlx::query_as(
        "SELECT NOT coalesce((SELECT kind = 'exclude' FROM app.punch_adjudications WHERE punch_id = $1 AND kind IN ('exclude', 'include') ORDER BY created_at DESC LIMIT 1), false),
                coalesce((SELECT new_work_date FROM app.punch_adjudications WHERE punch_id = $1 AND kind = 'move_date' ORDER BY created_at DESC LIMIT 1), $2),
                coalesce((SELECT new_direction::text FROM app.punch_adjudications WHERE punch_id = $1 AND kind = 'change_direction' ORDER BY created_at DESC LIMIT 1), $3)",
    )
    .bind(id)
    .bind(base_date)
    .bind(&direction)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let nothing = match req.kind.as_str() {
        "exclude" => !counted,
        "include" => counted,
        "move_date" => req.new_work_date == Some(cur_date),
        _ => req.new_direction.as_deref() == Some(cur_dir.as_str()),
    };
    if nothing {
        return Err(AppError::unprocessable("invalid_adjudication", "Nothing to change", "The adjudication changes nothing or has no earlier exclusion to undo").into());
    }
    let aid: Uuid = sqlx::query_scalar(
        "INSERT INTO app.punch_adjudications (org_id, punch_id, punched_at, employee_id, kind, new_work_date, new_direction, origin, reason)
         VALUES ($1, $2, $3, $4, $5::app.adjudication_kind, $6, $7::app.punch_direction, 'hr', $8) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(at)
    .bind(employee)
    .bind(&req.kind)
    .bind(req.new_work_date)
    .bind(&req.new_direction)
    .bind(&reason)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.audit(ADJUDICATE, "punch", id, None, Some(json!({ "adjudication_id": aid, "kind": req.kind, "new_work_date": req.new_work_date, "new_direction": req.new_direction })), reason.as_deref()).await?;
    let mut dates = vec![cur_date];
    dates.extend(req.new_work_date);
    for d in dates {
        ctx.event("PunchRecorded", "employee", employee, json!({ "employee_id": employee, "work_date": d, "punch_id": id })).await?;
    }
    let doc = punch_doc(&mut ctx.tx, id).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok((StatusCode::CREATED, Json(doc)).into_response())
}

// ---------- P8 manual attendance ----------

fn manual_scope(ctx: &Ctx, branch: Uuid) -> ApiResult<()> {
    let s = ctx.scope(MANUAL)?;
    if s.org || s.branches.contains(&branch) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

/// GET /attendance/manual-attendance/sheet: employees placed at the branch that day, with their manual entries (AC-AT-18.1).
pub async fn sheet(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let (mut branch, mut date) = (None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            "work_date" => date = Some(v.parse::<NaiveDate>().map_err(|_| bad_param("work_date", "Use YYYY-MM-DD"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let (Some(branch), Some(date)) = (branch, date) else { return Err(AppError::validation(vec![FieldError::new("branch_id", "required", "Give branch_id and work_date")]).into()) };
    manual_scope(&ctx, branch)?;
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('employee', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
           'entries', (SELECT coalesce(jsonb_agg({MANUAL_DOC} ORDER BY m.punched_at), '[]') {MANUAL_FROM} WHERE m.employee_id = jv.employee_id AND m.voided_at IS NULL
                       AND (m.punched_at AT TIME ZONE b.time_zone)::date = $2)) ORDER BY e.employee_code), '[]')
         FROM app.employee_job_versions jv JOIN app.employees e ON e.id = jv.employee_id
         WHERE jv.master_branch_id = $1 AND jv.state = 'active' AND jv.valid @> $2::date"
    ))
    .bind(branch)
    .bind(date)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "branch_id": branch, "work_date": date, "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualEntry {
    employee_id: Uuid,
    in_time: Option<NaiveTime>,
    out_time: Option<NaiveTime>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualBatch {
    branch_id: Uuid,
    work_date: NaiveDate,
    reason: Option<String>,
    entries: Vec<ManualEntry>,
}

/// POST /attendance/manual-attendance: each entry's in and out become manual punches at the branch's local
/// time (an out before the in is the next morning). Bad entries are listed; the rest are saved (PUN-08).
pub async fn manual(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/attendance/manual-attendance", &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let req: ManualBatch = parse(raw)?;
    manual_scope(&ctx, req.branch_id)?;
    let Some(reason) = req.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give one reason for the batch")]).into());
    };
    if req.entries.len() > 2000 {
        return Err(AppError::unprocessable("too_many_entries", "Too many entries", "At most 2,000 entries").into());
    }
    let zone: String = sqlx::query_scalar("SELECT time_zone FROM app.branches WHERE id = $1").bind(req.branch_id).fetch_optional(&mut *ctx.tx).await?.ok_or_else(AppError::not_found)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    let batch = Uuid::new_v4();
    let (mut created, mut failed) = (0, Vec::new());
    for en in req.entries {
        let fail = |code: &str| json!({ "employee_id": en.employee_id, "code": code });
        if en.in_time.is_none() && en.out_time.is_none() {
            failed.push(fail("invalid_value"));
            continue;
        }
        if !allowed(&mut ctx, en.employee_id, MANUAL, Access::Edit, &em).await? {
            failed.push(fail("not_found"));
            continue;
        }
        let employed: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.employee_job_versions WHERE employee_id = $1 AND state = 'active' AND valid @> $2::date)")
            .bind(en.employee_id)
            .bind(req.work_date)
            .fetch_one(&mut *ctx.tx)
            .await?;
        if !employed {
            failed.push(fail("outside_employment"));
            continue;
        }
        let next_day = matches!((en.in_time, en.out_time), (Some(i), Some(o)) if o <= i);
        for (dir, t) in [("in", en.in_time), ("out", en.out_time)] {
            let Some(t) = t else { continue };
            let local = req.work_date.and_time(t) + Duration::days((dir == "out" && next_day) as i64);
            let mid: Uuid = sqlx::query_scalar(
                "INSERT INTO app.manual_punches (org_id, employee_id, punched_at, direction, branch_id, origin, reason, batch_id)
                 VALUES ($1, $2, $3 AT TIME ZONE $4, $5::app.punch_direction, $6, 'device_failure', $7, $8) RETURNING id",
            )
            .bind(ctx.org_id)
            .bind(en.employee_id)
            .bind(local)
            .bind(&zone)
            .bind(dir)
            .bind(req.branch_id)
            .bind(&reason)
            .bind(batch)
            .fetch_one(&mut *ctx.tx)
            .await?;
            ctx.audit(MANUAL, "manual_punch", mid, None, Some(json!({ "employee_id": en.employee_id, "local_time": local, "direction": dir, "batch_id": batch })), Some(&reason)).await?;
        }
        ctx.event("PunchRecorded", "employee", en.employee_id, json!({ "employee_id": en.employee_id, "work_date": req.work_date })).await?;
        created += 1;
    }
    let body = json!({ "batch_id": batch, "created": created, "failed": failed });
    ctx.idempotency_finish(&headers, 200, &body).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}

// ---------- P9 void a manual punch ----------

// shortcut: manual_punches has no row_version column, so If-Match isn't checked; the void is one-way.
pub async fn void(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let Some(reason) = reason_of(raw)? else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    let row: Option<(Uuid, DateTime<Utc>, bool)> =
        sqlx::query_as("SELECT employee_id, punched_at, voided_at IS NOT NULL FROM app.manual_punches WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut *ctx.tx).await?;
    let Some((employee, at, voided)) = row else { return Err(AppError::not_found().into()) };
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, employee, MANUAL, &em).await?;
    if voided {
        return Err(AppError::conflict("already_voided", "Already voided", "This manual punch is already voided").into());
    }
    sqlx::query("UPDATE app.manual_punches SET voided_at = now(), voided_by = $2, void_reason = $3 WHERE id = $1").bind(id).bind(ctx.user_id).bind(&reason).execute(&mut *ctx.tx).await?;
    ctx.audit(MANUAL, "manual_punch", id, Some(json!({ "voided": false })), Some(json!({ "voided": true })), Some(&reason)).await?;
    let date = placement(&mut ctx.tx, employee, at).await?.map(|p| p.3);
    ctx.event("PunchRecorded", "employee", employee, json!({ "employee_id": employee, "work_date": date })).await?;
    let doc: Value = sqlx::query_scalar(&format!("SELECT {MANUAL_DOC} {MANUAL_FROM} WHERE m.id = $1")).bind(id).fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(doc).into_response())
}
