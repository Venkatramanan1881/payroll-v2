//! Day results (API_CONTRACTS Package 2 §7, §10, §12): the recalculation that runs the S6 engine and stores a
//! version (J-AT2), finalising (J-AT1), the nightly sweep (J-AT3), DY1–DY3 and self-service M1–M3.
//! Inputs are read and the result stored under a per employee-date lock; a result is stored only when the
//! input fingerprint changed (INV-AT-09, DM-04).
// shortcut: a change touching a locked date is skipped, not shadow-recalculated into an adjustment (S9, on hold).

use crate::ctx::Ctx;
use crate::employees::{allowed, bad_param, em_settings, push_from, push_scope, query_pairs, require_edit, today, Access, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::{Path, RawQuery};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use payroll_core::day::{self, DayInput, Dir, Overlays, Punch};
use payroll_core::policy::{AtSettings, WeeklyOffMode};
use payroll_core::shifts::ShiftTimes;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shared::{AppError, FieldError};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

const VIEW: &str = "attendance.day.view";
const CORRECT: &str = "attendance.day.correct";
/// How far back an event recalculates (the open period and a little more).
// shortcut: changes dated earlier than this are not recalculated; with S9 they become shadow adjustments.
const LOOKBACK_DAYS: i64 = 62;
/// Days the nightly sweep looks at (UA-01 pending → absent, days without punches).
const SWEEP_DAYS: i64 = 8;

/// The expected schedule on a date (ScheduleResolver, DOMAIN_MODEL §5).
pub(crate) struct Sched {
    pub master: Uuid,
    pub job: Uuid,
    /// A roster entry exists for the date (then SH-08 never overrides it).
    pub rostered: bool,
    pub zone: String,
    pub shift_version: Option<Uuid>,
    pub shift: Option<ShiftTimes>,
    pub shift_doc: Value,
    pub weekly_off: bool,
    pub holiday: bool,
}

/// A shift version `v` of shift `s` as the engine's ShiftTimes, and as shown in a day's detail ($3 = detected).
const TIMES: &str = "CASE WHEN v.id IS NULL THEN NULL ELSE jsonb_build_object('kind', v.kind, 'start_time', v.start_time, 'end_time', v.end_time,
    'is_night', v.is_night, 'night_date_rule', v.night_date_rule, 'required_minutes', v.required_minutes,
    'window_before_min', v.window_before_min, 'window_after_min', v.window_after_min, 'flex_earliest', v.flex_earliest,
    'flex_latest', v.flex_latest, 'core_start', v.core_start, 'core_end', v.core_end, 'break_kind', v.break_kind,
    'break_start', v.break_start, 'break_end', v.break_end, 'half_day_point', v.half_day_point, 'half_day_time', v.half_day_time) END";
const DOC: &str = "CASE WHEN v.id IS NULL THEN 'null'::jsonb ELSE jsonb_build_object('id', s.id, 'code', s.code, 'name', s.name,
    'start', left(v.start_time::text, 5), 'end', left(v.end_time::text, 5), 'detected', $3::bool) END";
/// The latest approved version of shift `s` on date $2.
const VERSION: &str = "LEFT JOIN LATERAL (SELECT * FROM app.shift_versions w WHERE w.shift_id = s.id AND w.state = 'approved' AND w.valid_from <= $2
    ORDER BY w.valid_from DESC LIMIT 1) v ON true";

/// SH-08 candidates: the employee's allowed shifts, or with none set every active shift usable at the master branch
/// (ATQ-07), each as (version, times, detail doc marked detected).
async fn detect_candidates(tx: &mut Tx, sched: &Sched, date: NaiveDate) -> ApiResult<Vec<(Uuid, ShiftTimes, Value)>> {
    let rows: Vec<(Uuid, Value, Value)> = sqlx::query_as(&format!(
        "SELECT v.id, {TIMES}, {DOC} FROM app.shifts s {VERSION}
         WHERE v.id IS NOT NULL AND s.state = 'active' AND (s.branch_id IS NULL OR s.branch_id = $4)
           AND (NOT EXISTS (SELECT 1 FROM app.job_version_allowed_shifts a WHERE a.job_version_id = $1)
                OR EXISTS (SELECT 1 FROM app.job_version_allowed_shifts a WHERE a.job_version_id = $1 AND a.shift_id = s.id))
         ORDER BY s.code"
    ))
    .bind(sched.job)
    .bind(date)
    .bind(true)
    .bind(sched.master)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|(id, times, doc)| Ok((id, serde_json::from_value::<ShiftTimes>(times).map_err(|_| AppError::internal())?, doc)))
        .collect()
}

/// SH-09 weekly off from the policy (fixed days or "2nd and 4th Saturday").
fn policy_off(date: NaiveDate, s: &AtSettings) -> bool {
    let weekday = date.weekday().number_from_monday() as u8;
    let week = ((date.day() - 1) / 7 + 1) as u8;
    match s.weekly_off_mode {
        WeeklyOffMode::Fixed => s.weekly_off_days.contains(&weekday),
        WeeklyOffMode::Alternating => s.weekly_off_alternating.iter().any(|a| a.day == weekday && a.weeks.contains(&week)),
        WeeklyOffMode::FromRoster => false,
    }
}

/// Roster entry → default shift; weekly off from the roster or the policy; holiday from the master branch's
/// calendar (an optional one only when picked and approved, DEC-26). None = not employed that day (INV-AT-10).
pub(crate) async fn schedule(tx: &mut Tx, employee: Uuid, date: NaiveDate, s: &AtSettings) -> ApiResult<Option<Sched>> {
    let row = sqlx::query(
        &format!("SELECT jv.master_branch_id, jv.id AS job_id, b.time_zone, v.id AS version_id, {TIMES} AS times, {DOC} AS doc, re.id IS NOT NULL AS rostered,
                re.is_weekly_off,
                EXISTS (SELECT 1 FROM app.branch_calendars bc JOIN app.holidays h ON h.calendar_id = bc.calendar_id AND h.holiday_date = $2 AND h.removed_at IS NULL
                        WHERE bc.branch_id = jv.master_branch_id AND bc.year = extract(year FROM $2::date)::smallint
                          AND (h.kind <> 'optional' OR EXISTS (SELECT 1 FROM app.optional_holiday_picks p WHERE p.holiday_id = h.id AND p.employee_id = $1 AND p.state = 'approved'))) AS holiday
         FROM app.employee_job_versions jv JOIN app.branches b ON b.id = jv.master_branch_id
         LEFT JOIN app.roster_entries re ON re.employee_id = $1 AND re.work_date = $2 AND re.superseded_at IS NULL
         LEFT JOIN app.shifts s ON s.id = CASE WHEN re.id IS NOT NULL THEN re.shift_id ELSE jv.default_shift_id END
         {VERSION}
         WHERE jv.employee_id = $1 AND jv.state = 'active' AND jv.valid @> $2::date"
    ))
    .bind(employee)
    .bind(date)
    .bind(false)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let shift = r.get::<Option<Value>, _>("times").map(serde_json::from_value::<ShiftTimes>).transpose().map_err(|_| AppError::internal())?;
    let roster_off: Option<bool> = r.get("is_weekly_off");
    Ok(Some(Sched {
        master: r.get("master_branch_id"),
        job: r.get("job_id"),
        rostered: r.get("rostered"),
        zone: r.get("time_zone"),
        shift_version: r.get("version_id"),
        shift,
        shift_doc: r.get("doc"),
        weekly_off: roster_off.unwrap_or_else(|| policy_off(date, s)),
        holiday: r.get("holiday"),
    }))
}

/// Recalculates one employee-date. Returns the new version, or None when nothing changed, the date is in
/// the future or locked, or the employee wasn't employed (an old result is then withdrawn).
pub(crate) async fn recalc_day(tx: &mut Tx, org: Uuid, employee: Uuid, d: NaiveDate, reason: &str) -> ApiResult<Option<i32>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text || $2::text))").bind(employee).bind(d).execute(&mut **tx).await?;
    let current: Option<(i32, String)> =
        sqlx::query_as("SELECT version, input_fingerprint FROM app.day_results WHERE employee_id = $1 AND work_date = $2 AND is_current")
            .bind(employee)
            .bind(d)
            .fetch_optional(&mut **tx)
            .await?;
    if crate::periods::lock_on(tx, employee, d).await?.is_some() {
        return Ok(None);
    }
    let resolved = crate::policies::resolve(tx, employee, d).await?;
    let s = AtSettings::from_json(resolved.as_ref().map(|r| r.settings.clone()).unwrap_or(json!({})));
    let (Some(mut sched), Some(policy)) = (schedule(tx, employee, d, &s).await?, resolved) else {
        // Not employed on that date (exit, joining moved): no result may stand (INV-AT-10).
        if current.is_some() {
            sqlx::query("UPDATE app.day_results SET is_current = false WHERE employee_id = $1 AND work_date = $2 AND is_current")
                .bind(employee)
                .bind(d)
                .execute(&mut **tx)
                .await?;
        }
        return Ok(None);
    };
    let today: NaiveDate = sqlx::query_scalar("SELECT (now() AT TIME ZONE $1)::date").bind(&sched.zone).fetch_one(&mut **tx).await?;
    if d > today {
        return Ok(None);
    }

    // Punches near the date, after adjudication (CON-06); the window decides which day each belongs to.
    let around = d.and_hms_opt(0, 0, 0).expect("midnight").and_utc();
    let rows = sqlx::query(
        "SELECT p.id, p.punched_at AT TIME ZONE $3 AS local, p.branch_id,
                coalesce((SELECT a.new_direction::text FROM app.punch_adjudications a WHERE a.punch_id = p.id AND a.kind = 'change_direction' ORDER BY a.created_at DESC LIMIT 1), p.direction::text) AS dir,
                (SELECT a.new_work_date FROM app.punch_adjudications a WHERE a.punch_id = p.id AND a.kind = 'move_date' ORDER BY a.created_at DESC LIMIT 1) AS moved,
                coalesce((SELECT a.kind = 'exclude' FROM app.punch_adjudications a WHERE a.punch_id = p.id AND a.kind IN ('exclude', 'include') ORDER BY a.created_at DESC LIMIT 1), false) AS excluded,
                EXISTS (SELECT 1 FROM app.punch_reviews r WHERE r.punch_id = p.id AND r.state = 'pending') AS flagged, false AS manual
         FROM app.raw_punches p
         WHERE p.employee_id = $1 AND ((p.punched_at >= $2 - interval '2 days' AND p.punched_at < $2 + interval '3 days')
               OR p.id IN (SELECT a.punch_id FROM app.punch_adjudications a WHERE a.employee_id = $1 AND a.kind = 'move_date' AND a.new_work_date = $4))
         UNION ALL
         SELECT m.id, m.punched_at AT TIME ZONE $3, m.branch_id, m.direction::text, NULL, false, false, true
         FROM app.manual_punches m WHERE m.employee_id = $1 AND m.voided_at IS NULL AND m.punched_at >= $2 - interval '2 days' AND m.punched_at < $2 + interval '3 days'",
    )
    .bind(employee)
    .bind(around)
    .bind(&sched.zone)
    .bind(d)
    .fetch_all(&mut **tx)
    .await?;
    // SH-08: no roster entry, so the first punch may pick the shift (AC-AT-03.1); none fits = default + flag (03.2).
    // shortcut: neighbouring days keep their roster/default shift for the window overlap below; detect them too
    // if rotating staff without rosters show wrong overlaps.
    let mut not_detected = false;
    if s.auto_detect_shift && !sched.rostered && !rows.is_empty() {
        let mut opts = detect_candidates(tx, &sched, d).await?;
        let times: Vec<NaiveDateTime> = rows
            .iter()
            .filter(|r| !r.get::<bool, _>("excluded") && r.get::<Option<NaiveDate>, _>("moved").is_none_or(|m| m == d))
            .map(|r| r.get("local"))
            .collect();
        let refs: Vec<&ShiftTimes> = opts.iter().map(|o| &o.1).collect();
        match day::detect(d, &times, &refs) {
            Some(i) => {
                let (version, times, doc) = opts.swap_remove(i);
                (sched.shift_version, sched.shift, sched.shift_doc) = (Some(version), Some(times), doc);
            }
            None => not_detected = true,
        }
    }
    // Neighbouring days' shifts, for overlapping windows (W2).
    let mut cands: Vec<(NaiveDate, ShiftTimes)> = Vec::new();
    for x in [d - Duration::days(1), d, d + Duration::days(1)] {
        let sh = if x == d {
            sched.shift.clone()
        } else if rows.is_empty() {
            None
        } else {
            schedule(tx, employee, x, &s).await?.and_then(|n| n.shift)
        };
        if let Some(sh) = sh {
            cands.push((x, sh));
        }
    }
    let refs: Vec<(NaiveDate, &ShiftTimes)> = cands.iter().map(|(x, sh)| (*x, sh)).collect();
    let (mut punches, mut excluded, mut overlap) = (Vec::new(), Vec::new(), false);
    for r in &rows {
        let at: NaiveDateTime = r.get("local");
        let moved: Option<NaiveDate> = r.get("moved");
        let owner = match moved {
            Some(m) => Some(m),
            None => match day::assign(at, &refs) {
                Some((x, o)) => {
                    overlap |= o && x == d;
                    Some(x)
                }
                None => Some(at.date()).filter(|x| !refs.iter().any(|(c, _)| c == x)),
            },
        };
        if owner != Some(d) {
            continue;
        }
        let id: Uuid = r.get("id");
        if r.get::<bool, _>("excluded") {
            excluded.push(id);
            continue;
        }
        let dir = match r.get::<String, _>("dir").as_str() {
            "in" => Dir::In,
            "out" => Dir::Out,
            _ => Dir::Untagged,
        };
        punches.push(Punch { id, at, dir, branch: r.get("branch_id"), flagged: r.get("flagged") });
    }
    punches.sort_by_key(|p| (p.at, p.id));

    let waivers: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, kind::text FROM app.waivers WHERE work_date = $1 AND state = 'active' AND (branch_id = $2 OR shift_id = (SELECT shift_id FROM app.shift_versions WHERE id = $3))",
    )
    .bind(d)
    .bind(sched.master)
    .bind(sched.shift_version)
    .fetch_all(&mut **tx)
    .await?;
    let overlays = Overlays {
        late_waived: s.late_bulk_waiver && waivers.iter().any(|w| w.1 == "late"),
        early_waived: s.early_bulk_waiver && waivers.iter().any(|w| w.1 == "early"),
        ..Overlays::default()
    };
    // Grace uses this period (LA-04, EC-03).
    // shortcut: "this period" is the calendar month, whatever LA-18 says; payroll-period counters come with S9.
    let (late_used, early_used): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE detail->>'late_grace_used' = 'true'), count(*) FILTER (WHERE detail->>'early_grace_used' = 'true')
         FROM app.day_results WHERE employee_id = $1 AND is_current AND work_date >= date_trunc('month', $2::date)::date AND work_date < $2",
    )
    .bind(employee)
    .bind(d)
    .fetch_one(&mut **tx)
    .await?;
    let left = |uses: Option<u32>, used: i64| uses.map(|n| (n as i64 - used).max(0) as u32);
    // HD-03: the nearest working day on each side (§3.12); unknown until the day after is calculated.
    // shortcut: approved leave doesn't count as presence until Package 3 brings leave.
    let presence_around = if sched.holiday && s.holiday_needs_presence_around {
        let (before, after): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT (SELECT status::text FROM app.day_results WHERE employee_id = $1 AND is_current AND work_date < $2
                       AND status NOT IN ('weekly_off', 'holiday', 'holiday_unpaid') ORDER BY work_date DESC LIMIT 1),
                    (SELECT status::text FROM app.day_results WHERE employee_id = $1 AND is_current AND work_date > $2
                       AND status NOT IN ('weekly_off', 'holiday', 'holiday_unpaid') ORDER BY work_date LIMIT 1)",
        )
        .bind(employee)
        .bind(d)
        .fetch_one(&mut **tx)
        .await?;
        let ok = |x: &Option<String>| x.as_deref().map(|v| v == "present" || v == "half_day");
        match (ok(&before), ok(&after)) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        }
    } else {
        None
    };
    let override_row: Option<(Uuid, Option<String>, Option<i32>, Value)> = sqlx::query_as(
        "SELECT id, status::text, net_minutes, fields FROM app.day_overrides WHERE employee_id = $1 AND work_date = $2 AND voided_at IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .bind(employee)
    .bind(d)
    .fetch_optional(&mut **tx)
    .await?;
    let input = DayInput {
        date: d,
        shift: sched.shift.as_ref(),
        weekly_off: sched.weekly_off,
        holiday: sched.holiday,
        presence_around,
        punches,
        overlays,
        settings: &s,
        late_grace_left: left(s.late_grace_uses, late_used),
        early_grace_left: left(s.early_grace_uses, early_used),
        past_cutoff: today > d + Duration::days(s.rg_window_days as i64),
        today,
    };

    // The fingerprint holds every input the engine reads, including what "today" decides for it.
    let fp_doc = json!({
        "punches": input.punches.iter().map(|p| json!([p.id, p.at, format!("{:?}", p.dir), p.flagged, p.branch])).collect::<Vec<_>>(),
        "excluded": excluded, "shift_version": sched.shift_version, "policy_version": policy.version_id, "master": sched.master,
        "weekly_off": sched.weekly_off, "holiday": sched.holiday, "presence": presence_around, "waivers": waivers.iter().map(|w| w.0).collect::<Vec<_>>(),
        "grace": [input.late_grace_left, input.early_grace_left], "cutoff": input.past_cutoff, "overlap": overlap, "not_detected": not_detected,
        "pending": today < d + Duration::days(s.absent_apply_days as i64 + 1), "override": override_row.as_ref().map(|o| o.0),
    });
    let fp: String = Sha256::digest(fp_doc.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    if current.as_ref().is_some_and(|c| c.1 == fp) {
        return Ok(None);
    }

    let mut out = day::calculate(&input);
    if overlap {
        out.flags.push(day::OVERLAP);
    }
    if not_detected {
        out.flags.push(day::SHIFT_NOT_DETECTED);
    }
    let mut status = serde_json::to_value(out.status).expect("status serialises");
    let mut net = out.net_minutes;
    let mut overrides = Vec::new();
    // A day override from an HR correction wins over the calculation (C1); it is shown in the explanation.
    if let Some((oid, ost, onet, fields)) = &override_row {
        if let Some(x) = ost {
            status = json!(x);
        }
        if let Some(n) = onet {
            net = *n as i64;
        }
        for (k, v) in fields.as_object().into_iter().flatten() {
            match (k.as_str(), v) {
                ("late_mark", Value::Bool(b)) => out.late_mark = *b,
                ("early_mark", Value::Bool(b)) => out.early_mark = *b,
                ("late_minutes", Value::Number(n)) => out.late_minutes = n.as_i64().unwrap_or(0),
                ("early_minutes", Value::Number(n)) => out.early_minutes = n.as_i64().unwrap_or(0),
                ("deduction_days", Value::Number(n)) => out.deduction_days = n.as_f64().unwrap_or(0.0),
                _ => {}
            }
        }
        overrides.push(json!({ "id": oid, "status": ost, "net_minutes": onet, "fields": fields }));
    }
    let rounded = if net == out.net_minutes { out.net_minutes_rounded } else { net };
    let window_end = sched.shift.as_ref().map(|sh| day::window(d, sh).1).unwrap_or((d + Duration::days(1)).and_hms_opt(0, 0, 0).expect("midnight"));
    let detail = json!({
        "shift": sched.shift_doc,
        "sessions": out.sessions.iter().map(|x| json!({ "start": x.start, "end": x.end, "minutes": x.minutes, "branch_id": x.branch })).collect::<Vec<_>>(),
        "gross_minutes": out.gross_minutes, "break_minutes": out.break_minutes, "deduct_minutes": out.deduct_minutes,
        "late_grace_used": out.late_grace_used, "early_grace_used": out.early_grace_used,
        "permission_used": out.permission_used.map(|p| json!({ "from": p.0, "to": p.1 })),
        "costed_branches": out.costed.iter().map(|c| json!({ "branch_id": c.0.unwrap_or(sched.master), "minutes": c.1 })).collect::<Vec<_>>(),
        "local_zone": sched.zone,
        "explanation": {
            "punches_used": input.punches.iter().map(|p| p.id).collect::<Vec<_>>(), "punches_excluded": excluded,
            "rules": out.rules.iter().map(|(k, v)| json!({ "setting": k, "value": v, "policy_version_id": policy.version_id })).collect::<Vec<_>>(),
            "policy_set": { "id": policy.set_id, "name": policy.set_name, "source": policy.source },
            "approvals": [], "waivers": waivers.iter().map(|w| w.0).collect::<Vec<_>>(), "overrides": overrides,
        },
    });
    let version = current.as_ref().map_or(1, |c| c.0 + 1);
    sqlx::query("UPDATE app.day_results SET is_current = false WHERE employee_id = $1 AND work_date = $2 AND is_current")
        .bind(employee)
        .bind(d)
        .execute(&mut **tx)
        .await?;
    let flags: Vec<String> = out.flags.iter().map(|f| f.to_string()).collect();
    sqlx::query(
        "INSERT INTO app.day_results (org_id, employee_id, work_date, version, reason, input_fingerprint, status, branch_id, shift_version_id,
                                      policy_version_id, first_in, last_out, net_minutes, net_minutes_rounded, late_minutes, early_minutes,
                                      late_mark, early_mark, deduction_days, flags, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7::app.day_status, $8, $9, $10, $11::timestamp AT TIME ZONE $22, $12::timestamp AT TIME ZONE $22,
                 $13, $14, $15, $16, $17, $18, $19::float8::numeric, $20,
                 jsonb_set($21, '{finalise_after}', to_jsonb(($23::timestamp AT TIME ZONE $22) + interval '1 hour')))",
    )
    .bind(org)
    .bind(employee)
    .bind(d)
    .bind(version)
    .bind(reason)
    .bind(&fp)
    .bind(status.as_str().unwrap_or("absent"))
    .bind(sched.master)
    .bind(sched.shift_version)
    .bind(policy.version_id)
    .bind(out.first_in)
    .bind(out.last_out)
    .bind(net as i32)
    .bind(rounded as i32)
    .bind(out.late_minutes as i32)
    .bind(out.early_minutes as i32)
    .bind(out.late_mark)
    .bind(out.early_mark)
    .bind(out.deduction_days)
    .bind(&flags)
    .bind(detail)
    .bind(&sched.zone)
    .bind(window_end)
    .execute(&mut **tx)
    .await?;
    sqlx::query("INSERT INTO app.outbox_events (org_id, event_type, aggregate_type, aggregate_id, payload) VALUES ($1, 'DayResultChanged', 'employee', $2, $3)")
        .bind(org)
        .bind(employee)
        .bind(json!({ "employee_id": employee, "work_date": d, "version": version, "status": status }))
        .execute(&mut **tx)
        .await?;
    Ok(Some(version))
}

/// Recalculates a range for one employee, one transaction per day so one bad day never blocks the rest.
pub(crate) async fn recalc_range(s: &AppState, org: Uuid, employee: Uuid, from: NaiveDate, to: NaiveDate, reason: &str) -> Result<usize, String> {
    recalc_dates(s, org, employee, from.iter_days().take_while(|d| *d <= to).collect(), reason).await
}

async fn recalc_dates(s: &AppState, org: Uuid, employee: Uuid, dates: Vec<NaiveDate>, reason: &str) -> Result<usize, String> {
    let mut changed = 0;
    for d in dates {
        let mut tx = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
        if recalc_day(&mut tx, org, employee, d, reason).await.map_err(|e| format!("{d}: {e:?}"))?.is_some() {
            changed += 1;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

/// Events that change a day's inputs (DOMAIN_MODEL §7, consumed by RecalculationQueue). EmployeeCreated needs
/// nothing: a new employee has no results, and the sweep creates the recent ones.
pub(crate) const RECALC_EVENTS: [&str; 7] =
    ["PunchRecorded", "ScheduleChanged", "PolicyChanged", "JobChanged", "StatusChanged", "EmployeeExited", "RecalculationRequested"];

fn date_of(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(|x| x.parse().ok())
}

/// Turns one event into (employees, from, to) and recalculates them (J-AT2).
// shortcut: runs inside the outbox dispatch, not as one Cloud Task per employee with TEC-03 rate limits; an
// organisation-wide policy change recalculates everyone here. Move to Cloud Tasks at deployment.
pub(crate) async fn on_event(s: &AppState, org: Uuid, kind: &str, p: &Value) -> Result<(), String> {
    let today = today();
    let dates: Vec<NaiveDate> = p["dates"].as_array().map(|a| a.iter().filter_map(date_of).collect()).unwrap_or_default();
    let mut from = ["from", "effective_from", "work_date", "joining_date"].iter().find_map(|k| date_of(&p[*k])).or(dates.iter().min().copied()).unwrap_or(today);
    let mut to = ["to", "work_date"].iter().find_map(|k| date_of(&p[*k])).or(dates.iter().max().copied()).unwrap_or(today);
    if kind == "PunchRecorded" {
        from -= Duration::days(1); // the punch may belong to the night shift before (W2)
    }
    if kind == "EmployeeExited" {
        from = date_of(&p["last_working_day"]).map_or(from, |l| l + Duration::days(1));
    }
    from = from.max(today - Duration::days(LOOKBACK_DAYS));
    to = to.min(today);
    if from > to {
        return Ok(());
    }
    let mut ids: Vec<Uuid> = p["employee_ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()?.parse().ok()).collect()).unwrap_or_default();
    ids.extend(p["employee_id"].as_str().and_then(|x| x.parse::<Uuid>().ok()));
    if ids.is_empty() {
        let branches: Vec<Uuid> = p["branch_ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()?.parse().ok()).collect()).unwrap_or_default();
        let shift: Option<Uuid> = p["shift_id"].as_str().and_then(|x| x.parse().ok());
        let mut t = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
        // Branch calendar → its employees; shift → its users; policy set or assignment → everyone placed then.
        ids = sqlx::query_scalar(
            "SELECT DISTINCT jv.employee_id FROM app.employee_job_versions jv
             WHERE jv.state = 'active' AND jv.valid && daterange($1, $2, '[]')
               AND (cardinality($3::uuid[]) = 0 OR jv.master_branch_id = ANY($3))
               AND ($4::uuid IS NULL OR jv.default_shift_id = $4
                    OR EXISTS (SELECT 1 FROM app.roster_entries re WHERE re.employee_id = jv.employee_id AND re.shift_id = $4 AND re.superseded_at IS NULL AND re.work_date BETWEEN $1 AND $2))",
        )
        .bind(from)
        .bind(to)
        .bind(&branches)
        .bind(shift)
        .fetch_all(&mut *t)
        .await
        .map_err(|e| e.to_string())?;
        t.commit().await.map_err(|e| e.to_string())?;
    }
    let reason = p["reason"].as_str().map(str::to_string).unwrap_or_else(|| kind.to_string());
    // A punch or a manual retry calculates every date in range; a schedule, policy or job change touches only
    // days already calculated (the nightly sweep creates the rest), so a far-back change stays cheap.
    let every_day = kind == "PunchRecorded" || kind == "RecalculationRequested";
    for e in ids {
        let dates = if every_day {
            from.iter_days().take_while(|d| *d <= to).collect()
        } else {
            let mut t = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
            let ds: Vec<NaiveDate> = sqlx::query_scalar("SELECT work_date FROM app.day_results WHERE employee_id = $1 AND is_current AND work_date BETWEEN $2 AND $3 ORDER BY 1")
                .bind(e)
                .bind(from)
                .bind(to)
                .fetch_all(&mut *t)
                .await
                .map_err(|e| e.to_string())?;
            t.commit().await.map_err(|e| e.to_string())?;
            ds
        };
        recalc_dates(s, org, e, dates, &reason).await?;
    }
    Ok(())
}

// ---------- J-AT1 finalise, J-AT3 sweep ----------

/// Finalises days whose window ended more than an hour ago with nothing open on them (§3.26, SM-AT-8):
/// no pending flagged-punch review, no open regularisation request and no missed-punch flag.
pub(crate) async fn finalise(s: &AppState, org: Uuid) -> ApiResult<u64> {
    let mut tx = db::begin_tenant(&s.pool, org, None).await?;
    let n = sqlx::query(
        "UPDATE app.day_results d SET finalised_at = now()
         WHERE d.is_current AND d.finalised_at IS NULL AND (d.detail->>'finalise_after')::timestamptz < now()
           AND NOT ('missed_punch' = ANY(d.flags))
           AND NOT EXISTS (SELECT 1 FROM app.regularisation_requests q WHERE q.employee_id = d.employee_id AND q.work_date = d.work_date
                           AND q.state IN ('pending', 'escalated'))
           AND NOT EXISTS (SELECT 1 FROM app.punch_reviews r WHERE r.employee_id = d.employee_id AND r.state = 'pending'
                           AND r.punch_id::text IN (SELECT jsonb_array_elements_text(d.detail->'explanation'->'punches_used')))",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// Nightly: every employee placed in the last SWEEP_DAYS days is recalculated; unchanged days are skipped
/// by their fingerprint, so this only catches days without punches and "Absent – pending" turning absent.
pub(crate) async fn sweep(s: &AppState, org: Uuid) -> Result<usize, String> {
    let to = today();
    let from = to - Duration::days(SWEEP_DAYS);
    let mut t = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT employee_id FROM app.employee_job_versions WHERE state = 'active' AND valid && daterange($1, $2, '[]')")
        .bind(from)
        .bind(to)
        .fetch_all(&mut *t)
        .await
        .map_err(|e| e.to_string())?;
    t.commit().await.map_err(|e| e.to_string())?;
    let mut changed = 0;
    for e in ids {
        changed += recalc_range(s, org, e, from, to, "nightly sweep").await?;
    }
    Ok(changed)
}

// ---------- DY1 / M3 list ----------

const DAY: &str = "jsonb_build_object('employee', jsonb_build_object('id', e.id, 'employee_code', e.employee_code, 'full_name', e.full_name),
    'employee_id', d.employee_id, 'work_date', d.work_date, 'version', d.version,
    'state', CASE WHEN d.finalised_at IS NULL THEN 'provisional' ELSE 'finalised' END, 'status', d.status,
    'shift', d.detail->'shift', 'first_in', d.first_in, 'last_out', d.last_out, 'net_minutes', d.net_minutes, 'net_minutes_rounded', d.net_minutes_rounded,
    'late_minutes', d.late_minutes, 'early_minutes', d.early_minutes, 'late_mark', d.late_mark, 'early_mark', d.early_mark,
    'deduction_days', d.deduction_days, 'flags', to_jsonb(d.flags), 'branch', jsonb_build_object('id', b.id, 'short_code', b.short_code),
    'costed_branches', d.detail->'costed_branches', 'calculated_at', d.calculated_at, 'reason', d.reason)";

fn date_param(k: &str, v: &str) -> ApiResult<NaiveDate> {
    v.parse().map_err(|_| bad_param(k, "Use YYYY-MM-DD").into())
}

async fn list_days(mut ctx: Ctx, raw: Option<String>, only: Option<Uuid>) -> ApiResult<Json<Value>> {
    let scope = ctx.scope(VIEW)?;
    let (mut from, mut to, mut employee, mut cursor) = (None, None, only, None);
    let mut filters: Vec<(&str, String)> = Vec::new();
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "from" => from = Some(date_param("from", &v)?),
            "to" => to = Some(date_param("to", &v)?),
            "employee_id" if only.is_none() => employee = Some(v.parse::<Uuid>().map_err(|_| bad_param("employee_id", "Not an id"))?),
            "branch_id" | "department_id" | "status" | "flag" | "late" | "state" if only.is_none() || matches!(k.as_str(), "status" | "flag") => {
                filters.push((match k.as_str() { "branch_id" => "branch_id", "department_id" => "department_id", "status" => "status", "flag" => "flag", "late" => "late", _ => "state" }, v))
            }
            "cursor" => cursor = Some(v),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let to = to.unwrap_or_else(today);
    let from = from.unwrap_or(to - Duration::days(6));
    if to < from || (to - from).num_days() >= 62 {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", "to must be on or after from, at most 62 days").into());
    }
    let as_of = today();
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(format!("SELECT {DAY} AS doc, d.work_date, e.employee_code"));
    push_from(&mut qb, as_of);
    qb.push(" JOIN app.day_results d ON d.employee_id = e.id AND d.is_current AND d.work_date BETWEEN ")
        .push_bind(from)
        .push(" AND ")
        .push_bind(to)
        .push(" JOIN app.branches b ON b.id = d.branch_id WHERE ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    if let Some(e) = employee {
        qb.push(" AND e.id = ").push_bind(e);
    }
    for (k, v) in filters {
        match k {
            "branch_id" => qb.push(" AND d.branch_id = ").push_bind(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            "department_id" => qb.push(" AND jv.department_id = ").push_bind(v.parse::<Uuid>().map_err(|_| bad_param("department_id", "Not an id"))?),
            "status" => qb.push(" AND d.status::text = ").push_bind(v),
            "flag" if v == "any" => qb.push(" AND cardinality(d.flags) > 0"),
            "flag" => qb.push(" AND ").push_bind(v).push(" = ANY(d.flags)"),
            "late" => qb.push(if v == "true" { " AND d.late_minutes > 0" } else { " AND d.late_minutes = 0" }),
            _ => qb.push(if v == "finalised" { " AND d.finalised_at IS NOT NULL" } else { " AND d.finalised_at IS NULL" }),
        };
    }
    if let Some(c) = cursor {
        let (dt, code) = c.split_once('|').ok_or_else(|| bad_param("cursor", "Not a valid cursor"))?;
        qb.push(" AND (d.work_date, e.employee_code) > (").push_bind(date_param("cursor", dt)?).push(", ").push_bind(code.to_string()).push(")");
    }
    qb.push(" ORDER BY d.work_date, e.employee_code LIMIT 201");
    let rows = qb.build().fetch_all(&mut *ctx.tx).await?;
    let more = rows.len() > 200;
    let rows = &rows[..rows.len().min(200)];
    let next = more.then(|| rows.last().map(|r| format!("{}|{}", r.get::<NaiveDate, _>("work_date"), r.get::<String, _>("employee_code")))).flatten();
    let items: Vec<Value> = rows.iter().map(|r| r.get("doc")).collect();
    ctx.commit().await?;
    Ok(Json(json!({ "items": items, "next_cursor": next })))
}

pub async fn list(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    list_days(ctx, raw, None).await
}

pub async fn my_list(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    list_days(ctx, raw, Some(me)).await
}

// ---------- DY2 / M3 one day ----------

async fn one_day(mut ctx: Ctx, id: Uuid, date: NaiveDate, raw: Option<String>) -> ApiResult<Json<Value>> {
    ctx.scope(VIEW)?;
    let mut known_on: Option<chrono::DateTime<chrono::Utc>> = None;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "known_on" => known_on = Some(v.parse().map_err(|_| bad_param("known_on", "Use an RFC 3339 time"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, VIEW, Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    // The version current at `known_on` (the latest calculated by then), else the current one (§3.25).
    let doc: Option<Value> = sqlx::query_scalar(&format!(
        "SELECT {DAY} || jsonb_build_object('explanation', d.detail->'explanation', 'sessions', d.detail->'sessions',
                'gross_minutes', d.detail->'gross_minutes', 'break_minutes', d.detail->'break_minutes')
         FROM app.day_results d JOIN app.employees e ON e.id = d.employee_id JOIN app.branches b ON b.id = d.branch_id
         WHERE d.employee_id = $1 AND d.work_date = $2 AND (CASE WHEN $3::timestamptz IS NULL THEN d.is_current ELSE d.calculated_at <= $3 END)
         ORDER BY d.version DESC LIMIT 1"
    ))
    .bind(id)
    .bind(date)
    .bind(known_on)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some(mut doc) = doc else { return Err(AppError::not_found().into()) };
    let versions: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('version', version, 'reason', reason, 'calculated_at', calculated_at) ORDER BY version), '[]')
         FROM app.day_results WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(id)
    .bind(date)
    .fetch_one(&mut *ctx.tx)
    .await?;
    // A punch, adjudication or manual punch newer than the result means a recalculation is waiting (§1.4).
    // shortcut: reads the input tables, not the outbox (app_api can't read it); schedule and policy changes don't show here.
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM app.raw_punches p WHERE p.employee_id = $1 AND p.received_at > $3 AND p.punched_at BETWEEN $2::date - 1 AND $2::date + 2)
             OR EXISTS (SELECT 1 FROM app.punch_adjudications a WHERE a.employee_id = $1 AND a.created_at > $3)
             OR EXISTS (SELECT 1 FROM app.manual_punches m WHERE m.employee_id = $1 AND (m.created_at > $3 OR m.voided_at > $3))",
    )
    .bind(id)
    .bind(date)
    .bind(doc["calculated_at"].as_str().and_then(|x| x.parse::<chrono::DateTime<chrono::Utc>>().ok()))
    .fetch_one(&mut *ctx.tx)
    .await?;
    let lock = crate::periods::lock_on(&mut ctx.tx, id, date).await?;
    if lock.is_some() {
        doc["state"] = json!("locked");
    }
    let can_correct = lock.is_none() && allowed(&mut ctx, id, CORRECT, Access::Edit, &em).await?;
    doc["versions"] = versions;
    doc["pending_recalculation"] = json!(pending);
    doc["requests"] = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', id, 'kind', kind, 'state', state, 'created_at', created_at) ORDER BY created_at), '[]')
         FROM app.regularisation_requests WHERE employee_id = $1 AND work_date = $2",
    )
    .bind(id)
    .bind(date)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let can_regularise = ctx.has("attendance.regularisation.request")
        && lock.is_none()
        && (ctx.employee_id == Some(id) || allowed(&mut ctx, id, "attendance.regularisation.request", Access::Edit, &em).await?);
    doc["can"] = json!({ "regularise": can_regularise, "correct": can_correct });
    ctx.commit().await?;
    Ok(Json(doc))
}

pub async fn get(ctx: Ctx, Path((id, date)): Path<(Uuid, NaiveDate)>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    one_day(ctx, id, date, raw).await
}

pub async fn my_get(ctx: Ctx, Path(date): Path<NaiveDate>, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    one_day(ctx, me, date, raw).await
}

// ---------- DY3 manual recalculation ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recalc {
    from: NaiveDate,
    to: NaiveDate,
    reason: Option<String>,
}

pub async fn recalculate(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let req: Recalc = parse(raw)?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, CORRECT, &em).await?;
    let Some(reason) = req.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    if req.to < req.from || (req.to - req.from).num_days() >= 62 {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", "to must be on or after from, at most 62 days").into());
    }
    let days = (req.to.min(today()) - req.from).num_days() + 1;
    ctx.audit(CORRECT, "employee", id, None, Some(json!({ "recalculate": { "from": req.from, "to": req.to } })), Some(&reason)).await?;
    ctx.event("RecalculationRequested", "employee", id, json!({ "employee_ids": [id], "from": req.from, "to": req.to, "reason": reason })).await?;
    ctx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "queued": days.max(0) }))).into_response())
}

// ---------- M1 features, M2 roster ----------

pub async fn features(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    let on = today();
    let s = crate::policies::settings_for(&mut ctx.tx, me, on).await?;
    let used: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM app.optional_holiday_picks WHERE employee_id = $1 AND state IN ('pending', 'approved')
         AND (SELECT extract(year FROM h.holiday_date) FROM app.holidays h WHERE h.id = holiday_id) = $2",
    )
    .bind(me)
    .bind(on.year() as f64)
    .fetch_one(&mut *ctx.tx)
    .await?;
    // Counted as RG1 counts against RG-03.
    let rg_used: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM app.regularisation_requests WHERE employee_id = $1 AND date_trunc('month', work_date) = date_trunc('month', $2::date)
           AND state NOT IN ('withdrawn', 'rejected', 'auto_rejected', 'cancelled')",
    )
    .bind(me)
    .bind(on)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({
        "punch": { "app": s.punch_app, "web": s.punch_web, "location_required": s.app_requires_location, "selfie_required": s.app_requires_selfie },
        "regularisation": { "enabled": s.regularisation, "kinds": s.rg_types, "window_days": s.rg_window_days, "limit": s.rg_limit_overall, "used": rg_used },
        "optional_holidays": { "allowed": s.optional_holidays_per_year, "used": used },
        "shift_requests": s.shift_requests_enabled,
    })))
}

/// M2: R1 for the caller, this and next week by default (AC-AT-04.1).
pub async fn my_roster(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let me = ctx.employee_id.ok_or_else(AppError::forbidden)?;
    let (mut from, mut to) = (None, None);
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "from" => from = Some(date_param("from", &v)?),
            "to" => to = Some(date_param("to", &v)?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let t = today();
    let from = from.unwrap_or(t - Duration::days(t.weekday().num_days_from_monday() as i64));
    let to = to.unwrap_or(from + Duration::days(13));
    let Json(mut v) = crate::roster::view(ctx, RawQuery(Some(format!("from={from}&to={to}&employee_id={me}")))).await?;
    Ok(Json(v["rows"].get_mut(0).map(Value::take).unwrap_or(json!({ "entries": {} }))))
}
