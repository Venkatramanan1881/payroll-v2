//! Rosters R1–R5 (API_CONTRACTS Package 2 §3, US-AT-02). A roster entry gives an employee-date a shift or a
//! weekly off and beats the default shift (AC-AT-02.2). Entries are replaced, never edited: the old one is
//! marked superseded (INV-AT-06, one current entry per employee-date).

use crate::ctx::Ctx;
use crate::employees::{allowed, bad_param, em_settings, push_filtered, query_pairs, today, Access};
use crate::error::{parse, ApiResult, Body};
use crate::periods::{date_locked, lock_on};
use axum::extract::RawQuery;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Days, NaiveDate};
use payroll_core::settings::EmSettings;
use payroll_core::shifts::{rotation, Slot};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

const MANAGE: &str = "attendance.roster.manage";
const MAX_DAYS: i64 = 62;
const MAX_ENTRIES: usize = 2000;

fn range(from: Option<NaiveDate>, to: Option<NaiveDate>) -> ApiResult<(NaiveDate, NaiveDate)> {
    let (Some(f), Some(t)) = (from, to) else {
        return Err(AppError::validation(vec![FieldError::new("from", "required", "Give from and to dates")]).into());
    };
    if t < f || (t - f).num_days() >= MAX_DAYS {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", format!("to must be on or after from, at most {MAX_DAYS} days")).into());
    }
    Ok((f, t))
}

// ---------- R1 GET /attendance/roster ----------

pub async fn view(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let scope = ctx.scope("attendance.roster.view")?;
    let pairs = query_pairs(raw)?;
    let (mut from, mut to, mut employee, mut cursor, mut filters) = (None, None, None, None, Vec::new());
    for (k, v) in &pairs {
        match k.as_str() {
            "from" => from = Some(v.parse().map_err(|_| bad_param("from", "Use YYYY-MM-DD"))?),
            "to" => to = Some(v.parse().map_err(|_| bad_param("to", "Use YYYY-MM-DD"))?),
            "employee_id" => employee = Some(v.parse::<Uuid>().map_err(|_| bad_param("employee_id", "Not an id"))?),
            "cursor" => cursor = Some(v.clone()),
            "branch_id" | "department_id" => filters.push((k.clone(), v.clone())),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let (from, to) = range(from, to)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    // Employees visible to the caller, as placed on the first day of the range; 100 per page.
    let mut qb = QueryBuilder::new("SELECT e.id, e.employee_code, e.full_name");
    push_filtered(&mut qb, &ctx, &scope, &em, from, &filters)?;
    qb.push(" AND jv.id IS NOT NULL");
    if let Some(e) = employee {
        qb.push(" AND e.id = ").push_bind(e);
    }
    if let Some(c) = &cursor {
        qb.push(" AND e.employee_code > ").push_bind(c.clone());
    }
    qb.push(" ORDER BY e.employee_code LIMIT 101");
    let people = qb.build().fetch_all(&mut *ctx.tx).await?;
    let more = people.len() > 100;
    let people = &people[..people.len().min(100)];
    let ids: Vec<Uuid> = people.iter().map(|r| r.get("id")).collect();

    let entries: Vec<(Uuid, Value)> = sqlx::query_as(
        "SELECT x.employee_id, jsonb_object_agg(x.d::text, x.entry) FROM (
           SELECT e.id AS employee_id, d::date AS d,
             CASE WHEN re.id IS NOT NULL THEN jsonb_build_object('shift_id', re.shift_id, 'code', s.code, 'is_weekly_off', re.is_weekly_off, 'source', re.source)
                  WHEN jv.default_shift_id IS NOT NULL THEN jsonb_build_object('shift_id', jv.default_shift_id, 'code', s2.code, 'is_weekly_off', false, 'source', 'default')
                  WHEN hol.h IS NOT NULL THEN '{}'::jsonb
             END || CASE WHEN hol.h IS NOT NULL THEN jsonb_build_object('holiday', hol.h) ELSE '{}' END AS entry
           FROM unnest($1::uuid[]) e(id) CROSS JOIN generate_series($2::date, $3::date, interval '1 day') d
           LEFT JOIN app.roster_entries re ON re.employee_id = e.id AND re.work_date = d::date AND re.superseded_at IS NULL
           LEFT JOIN app.shifts s ON s.id = re.shift_id
           LEFT JOIN app.employee_job_versions jv ON jv.employee_id = e.id AND jv.state = 'active' AND jv.valid @> d::date
           LEFT JOIN app.shifts s2 ON s2.id = jv.default_shift_id
           -- The master branch's calendar for the year; an optional holiday only with an approved pick.
           LEFT JOIN LATERAL (SELECT jsonb_build_object('id', h.id, 'name', h.name, 'kind', h.kind) AS h
             FROM app.branch_calendars bc JOIN app.holidays h ON h.calendar_id = bc.calendar_id AND h.holiday_date = d::date AND h.removed_at IS NULL
             WHERE bc.branch_id = jv.master_branch_id AND bc.year = extract(year FROM d)::smallint
               AND (h.kind <> 'optional' OR EXISTS (SELECT 1 FROM app.optional_holiday_picks p
                    WHERE p.holiday_id = h.id AND p.employee_id = e.id AND p.state = 'approved'))) hol ON true
           WHERE jv.id IS NOT NULL OR re.id IS NOT NULL) x
         WHERE x.entry IS NOT NULL
         GROUP BY x.employee_id",
    )
    .bind(&ids)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let by_id: HashMap<Uuid, Value> = entries.into_iter().collect();
    let rows: Vec<Value> = people
        .iter()
        .map(|r| {
            let id: Uuid = r.get("id");
            json!({
                "employee": { "id": id, "employee_code": r.get::<String, _>("employee_code"), "full_name": r.get::<String, _>("full_name") },
                "entries": by_id.get(&id).cloned().unwrap_or(json!({})),
            })
        })
        .collect();
    let next = more.then(|| people.last().map(|r| r.get::<String, _>("employee_code"))).flatten();
    ctx.commit().await?;
    Ok(Json(json!({ "from": from, "to": to, "rows": rows, "next_cursor": next })))
}

// ---------- applying entries (R2, R4, R5) ----------

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub employee_id: Uuid,
    pub work_date: NaiveDate,
    pub shift_id: Option<Uuid>,
    #[serde(default)]
    pub is_weekly_off: bool,
}

#[derive(Default)]
pub(crate) struct Outcome {
    pub applied: usize,
    pub unchanged: usize,
    pub failed: Vec<Value>,
}

/// Applies entries one by one; a bad entry is reported and the rest still apply (AC-AT-02.2).
// shortcut: a few queries per entry (fine for a few thousand); move to set-based SQL if large rosters are slow.
pub(crate) async fn apply_entries(ctx: &mut Ctx, em: &EmSettings, entries: Vec<Entry>, source: &str) -> ApiResult<Outcome> {
    let mut out = Outcome::default();
    let mut editable: HashMap<Uuid, bool> = HashMap::new();
    let mut shifts: HashMap<Uuid, Option<(String, Option<Uuid>)>> = HashMap::new();
    let mut changed: BTreeMap<Uuid, Vec<Value>> = BTreeMap::new();
    for en in entries {
        let fail = |code: &str| json!({ "employee_id": en.employee_id, "work_date": en.work_date, "code": code });
        if en.shift_id.is_some() == en.is_weekly_off {
            out.failed.push(fail("invalid_value"));
            continue;
        }
        let ok = match editable.get(&en.employee_id) {
            Some(ok) => *ok,
            None => {
                let ok = allowed(ctx, en.employee_id, MANAGE, Access::Edit, em).await?;
                editable.insert(en.employee_id, ok);
                ok
            }
        };
        if !ok {
            out.failed.push(fail("not_found"));
            continue;
        }
        let master: Option<Uuid> = sqlx::query_scalar(
            "SELECT master_branch_id FROM app.employee_job_versions WHERE employee_id = $1 AND state = 'active' AND valid @> $2::date",
        )
        .bind(en.employee_id)
        .bind(en.work_date)
        .fetch_optional(&mut *ctx.tx)
        .await?;
        let Some(master) = master else {
            out.failed.push(fail("outside_employment"));
            continue;
        };
        if lock_on(&mut ctx.tx, en.employee_id, en.work_date).await?.is_some() {
            out.failed.push(fail("date_locked"));
            continue;
        }
        if let Some(s) = en.shift_id {
            if let std::collections::hash_map::Entry::Vacant(slot) = shifts.entry(s) {
                let row: Option<(String, Option<Uuid>)> = sqlx::query_as("SELECT state::text, branch_id FROM app.shifts WHERE id = $1")
                    .bind(s)
                    .fetch_optional(&mut *ctx.tx)
                    .await?;
                slot.insert(row);
            }
            match &shifts[&s] {
                None => {
                    out.failed.push(fail("not_found"));
                    continue;
                }
                Some((state, _)) if state != "active" => {
                    out.failed.push(fail("shift_inactive"));
                    continue;
                }
                Some((_, Some(b))) if *b != master => {
                    out.failed.push(fail("shift_not_allowed_at_branch"));
                    continue;
                }
                _ => {}
            }
        }
        if !replace_entry(ctx, &en, source).await? {
            out.unchanged += 1;
            continue;
        }
        out.applied += 1;
        changed.entry(en.employee_id).or_default().push(json!({ "work_date": en.work_date, "shift_id": en.shift_id, "is_weekly_off": en.is_weekly_off }));
    }
    for (e, list) in changed {
        let dates: Vec<&Value> = list.iter().map(|x| &x["work_date"]).collect();
        ctx.audit(MANAGE, "employee", e, None, Some(json!({ "roster": list, "source": source })), None).await?;
        ctx.event("ScheduleChanged", "employee", e, json!({ "employee_ids": [e], "dates": dates })).await?;
    }
    Ok(out)
}

/// Supersedes the current entry for the employee-date with `en`. False when it already says the same.
/// No checks here: callers check access, employment, locks and the shift first.
pub(crate) async fn replace_entry(ctx: &mut Ctx, en: &Entry, source: &str) -> ApiResult<bool> {
    let current: Option<(Uuid, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT id, shift_id, is_weekly_off FROM app.roster_entries WHERE employee_id = $1 AND work_date = $2 AND superseded_at IS NULL FOR UPDATE",
    )
    .bind(en.employee_id)
    .bind(en.work_date)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    if current.is_some_and(|(_, s, w)| s == en.shift_id && w == en.is_weekly_off) {
        return Ok(false);
    }
    if let Some((old, ..)) = current {
        sqlx::query("UPDATE app.roster_entries SET superseded_at = now() WHERE id = $1").bind(old).execute(&mut *ctx.tx).await?;
    }
    let new: Uuid = sqlx::query_scalar(
        "INSERT INTO app.roster_entries (org_id, employee_id, work_date, shift_id, is_weekly_off, source)
         VALUES ($1, $2, $3, $4, $5, $6::app.roster_source) RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(en.employee_id)
    .bind(en.work_date)
    .bind(en.shift_id)
    .bind(en.is_weekly_off)
    .bind(source)
    .fetch_one(&mut *ctx.tx)
    .await?;
    if let Some((old, ..)) = current {
        sqlx::query("UPDATE app.roster_entries SET superseded_by_id = $2 WHERE id = $1").bind(old).bind(new).execute(&mut *ctx.tx).await?;
    }
    Ok(true)
}

/// Shared shape of R2, R4, R5: permission, idempotency, apply, reply.
async fn run(mut ctx: Ctx, headers: HeaderMap, raw: Value, route: &str, source: &str, build: impl FnOnce(Value) -> ApiResult<Vec<Entry>>) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, route, &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let entries = build(raw)?;
    if entries.len() > MAX_ENTRIES {
        return Err(AppError::unprocessable("range_too_long", "Too many entries", format!("At most {MAX_ENTRIES} entries per request")).into());
    }
    let em = em_settings(&mut ctx.tx, today()).await?;
    let o = apply_entries(&mut ctx, &em, entries, source).await?;
    let body = json!({ "applied": o.applied, "unchanged": o.unchanged, "failed": o.failed });
    ctx.idempotency_finish(&headers, 200, &body).await?;
    ctx.commit().await?;
    Ok(Json(body).into_response())
}

// ---------- R2 PUT /attendance/roster ----------

pub async fn put(ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Req {
        entries: Vec<Entry>,
    }
    run(ctx, headers, raw, "PUT /v1/attendance/roster", "screen", |raw| Ok(parse::<Req>(raw)?.entries)).await
}

// ---------- R3 DELETE /attendance/roster/entries ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clear {
    employee_id: Uuid,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    reason: Option<String>,
}

/// Back to the default shift for the range. Refused as a whole if a locked date has an entry.
pub async fn clear(mut ctx: Ctx, Body(raw): Body) -> ApiResult<Json<Value>> {
    ctx.scope(MANAGE)?;
    let req: Clear = parse(raw)?;
    let (from, to) = range(req.from, req.to)?;
    let reason = req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).ok_or_else(|| {
        AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")])
    })?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    crate::employees::require_edit(&mut ctx, req.employee_id, MANAGE, &em).await?;
    let dates: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT work_date FROM app.roster_entries WHERE employee_id = $1 AND work_date BETWEEN $2 AND $3 AND superseded_at IS NULL ORDER BY 1 FOR UPDATE",
    )
    .bind(req.employee_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *ctx.tx)
    .await?;
    for d in &dates {
        if let Some(lock) = lock_on(&mut ctx.tx, req.employee_id, *d).await? {
            return Err(date_locked(lock, *d));
        }
    }
    if !dates.is_empty() {
        sqlx::query("UPDATE app.roster_entries SET superseded_at = now() WHERE employee_id = $1 AND work_date = ANY($2) AND superseded_at IS NULL")
            .bind(req.employee_id)
            .bind(&dates)
            .execute(&mut *ctx.tx)
            .await?;
        ctx.audit(MANAGE, "employee", req.employee_id, Some(json!({ "roster_dates": dates })), None, Some(reason)).await?;
        ctx.event("ScheduleChanged", "employee", req.employee_id, json!({ "employee_ids": [req.employee_id], "dates": dates })).await?;
    }
    ctx.commit().await?;
    Ok(Json(json!({ "removed": dates.len() })))
}

// ---------- R4 POST /attendance/roster/copy ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Copy {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    to_start: NaiveDate,
    employee_ids: Option<Vec<Uuid>>,
    branch_id: Option<Uuid>,
    department_id: Option<Uuid>,
}

/// Copies the current entries of a range forward. Employees who have left or not yet joined on a target
/// date are reported as `outside_employment` (AC-AT-02.4).
pub async fn copy(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope(MANAGE)?;
    let req: Copy = parse(raw.clone())?;
    let (from, to) = range(req.from, req.to)?;
    if req.to_start <= to {
        return Err(bad_param("to_start", "Must be after the range being copied").into());
    }
    let rows: Vec<(Uuid, NaiveDate, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT r.employee_id, r.work_date, r.shift_id, r.is_weekly_off FROM app.roster_entries r
         JOIN app.employee_job_versions v ON v.employee_id = r.employee_id AND v.state = 'active' AND v.valid @> r.work_date
         WHERE r.superseded_at IS NULL AND r.work_date BETWEEN $1 AND $2
           AND ($3::uuid[] IS NULL OR r.employee_id = ANY($3))
           AND ($4::uuid IS NULL OR v.master_branch_id = $4) AND ($5::uuid IS NULL OR v.department_id = $5)
         ORDER BY r.employee_id, r.work_date",
    )
    .bind(from)
    .bind(to)
    .bind(&req.employee_ids)
    .bind(req.branch_id)
    .bind(req.department_id)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let shift = Days::new((req.to_start - from).num_days() as u64);
    let entries: Vec<Entry> = rows
        .into_iter()
        .filter_map(|(e, d, s, w)| Some(Entry { employee_id: e, work_date: d.checked_add_days(shift)?, shift_id: s, is_weekly_off: w }))
        .collect();
    run(ctx, headers, raw, "POST /v1/attendance/roster/copy", "copy", |_| Ok(entries)).await
}

// ---------- R5 POST /attendance/roster/rotation ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rotation {
    employee_ids: Vec<Uuid>,
    start: NaiveDate,
    weeks: u32,
    pattern: Vec<String>,
    #[serde(default)]
    weekly_off_days: Vec<u32>,
}

pub async fn rotate(ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    run(ctx, headers, raw, "POST /v1/attendance/roster/rotation", "rotation", |raw| {
        let r: Rotation = parse(raw)?;
        let mut errs = Vec::new();
        if !(1..=26).contains(&r.weeks) {
            errs.push(FieldError::new("weeks", "invalid_value", "1 to 26 weeks"));
        }
        if r.employee_ids.is_empty() {
            errs.push(FieldError::new("employee_ids", "required", "Choose at least one employee"));
        }
        if r.weekly_off_days.iter().any(|d| !(1..=7).contains(d)) {
            errs.push(FieldError::new("weekly_off_days", "invalid_value", "Days are 1 (Monday) to 7 (Sunday)"));
        }
        let slots: Option<Vec<Slot>> =
            r.pattern.iter().map(|p| if p == "weekly_off" { Some(Slot::WeeklyOff) } else { p.parse().ok().map(Slot::Shift) }).collect();
        let slots = match slots {
            Some(s) if !s.is_empty() => s,
            _ => {
                errs.push(FieldError::new("pattern", "invalid_value", "A list of shift ids or \"weekly_off\""));
                Vec::new()
            }
        };
        if !errs.is_empty() {
            return Err(AppError::validation(errs).into());
        }
        let days = rotation(r.start, r.weeks, &slots, &r.weekly_off_days);
        Ok(r.employee_ids
            .iter()
            .flat_map(|e| {
                days.iter().map(move |(d, s)| match s {
                    Slot::Shift(id) => Entry { employee_id: *e, work_date: *d, shift_id: Some(*id), is_weekly_off: false },
                    Slot::WeeklyOff => Entry { employee_id: *e, work_date: *d, shift_id: None, is_weekly_off: true },
                })
            })
            .collect())
    })
    .await
}
