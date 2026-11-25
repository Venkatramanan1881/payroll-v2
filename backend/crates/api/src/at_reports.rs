//! Attendance reports RP1–RP7 and the dashboard DB1 (API_CONTRACTS Package 2 §11, US-AT-43–44).
//! Each report is a column list plus rows, so the screen (JSON) and the export (RP8, Package 1 P3) share one
//! query. Rows follow the caller's employee-view scope like every Package 1 report.

use crate::ctx::Ctx;
use crate::employees::{bad_param, em_settings, push_from, push_scope, query_pairs, today, Access};
use crate::error::ApiResult;
use axum::extract::RawQuery;
use axum::Json;
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};
use shared::AppError;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

pub(crate) const CODES: [&str; 7] = ["r1", "r2", "r3", "r4", "r5", "r23", "r26"];
// shortcut: no paging on screen reports; capped here, large ones go through exports.
const MAX_ROWS: i64 = 50_000;

const EMP: &str = "'employee_code', e.employee_code, 'full_name', e.full_name";
const BRANCH: &str = "(SELECT b.short_code FROM app.branches b WHERE b.id = d.branch_id)";

pub(crate) struct Report {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
}

struct Params {
    from: NaiveDate,
    to: NaiveDate,
    branch: Option<Uuid>,
}

/// `from`/`to` (default the last 7 days, at most 62) or `date` for one day, and `branch_id`.
fn params(pairs: &[(String, String)]) -> ApiResult<Params> {
    let (mut from, mut to, mut branch) = (None, None, None);
    let date = |v: &str, n: &str| v.parse::<NaiveDate>().map_err(|_| bad_param(n, "Use YYYY-MM-DD"));
    for (k, v) in pairs {
        match k.as_str() {
            "from" => from = Some(date(v, "from")?),
            "to" => to = Some(date(v, "to")?),
            "date" => (from, to) = (Some(date(v, "date")?), Some(date(v, "date")?)),
            "branch_id" => branch = Some(v.parse::<Uuid>().map_err(|_| bad_param("branch_id", "Not an id"))?),
            _ => return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into()),
        }
    }
    let to = to.unwrap_or_else(today);
    let from = from.unwrap_or(to - Duration::days(6));
    if to < from || (to - from).num_days() >= 62 {
        return Err(AppError::unprocessable("range_too_long", "Range not valid", "to must be on or after from, at most 62 days").into());
    }
    Ok(Params { from, to, branch })
}

/// `SELECT {select}` over employees `e` (job `jv`) in the caller's view scope, joined by `join`, then `tail`.
async fn scoped(ctx: &mut Ctx, select: &str, join: impl FnOnce(&mut QueryBuilder<'static, Postgres>), tail: &str) -> ApiResult<Vec<Value>> {
    let scope = ctx.scope("employee.view")?;
    let as_of = today();
    let em = em_settings(&mut ctx.tx, as_of).await?;
    let mut qb = QueryBuilder::new(format!("SELECT {select}"));
    push_from(&mut qb, as_of);
    join(&mut qb);
    qb.push(" WHERE ");
    push_scope(&mut qb, &scope, Access::View, ctx.employee_id, em.hr_allowed_branch_access, as_of);
    qb.push(format!(" {tail} LIMIT {MAX_ROWS}"));
    Ok(qb.build_query_scalar().fetch_all(&mut *ctx.tx).await?)
}

/// Current day results in the range (and branch) as `d`.
fn days_join(p: &Params) -> impl FnOnce(&mut QueryBuilder<'static, Postgres>) + '_ {
    move |qb| {
        qb.push(" JOIN app.day_results d ON d.employee_id = e.id AND d.is_current AND d.work_date BETWEEN ").push_bind(p.from).push(" AND ").push_bind(p.to);
        if let Some(b) = p.branch {
            qb.push(" AND d.branch_id = ").push_bind(b);
        }
    }
}

fn cols(c: &[&str]) -> Vec<String> {
    c.iter().map(|s| s.to_string()).collect()
}

/// The report's rows. Checks the report's own permission.
pub(crate) async fn build(ctx: &mut Ctx, code: &str, pairs: &[(String, String)]) -> ApiResult<Report> {
    ctx.scope(&format!("report.{code}"))?;
    let p = params(pairs)?;
    Ok(match code {
        // r1 daily attendance.
        "r1" => Report {
            columns: cols(&["work_date", "employee_code", "full_name", "branch", "shift", "status", "first_in", "last_out", "net_minutes", "late_minutes", "early_minutes", "flags"]),
            rows: scoped(
                ctx,
                &format!("jsonb_build_object('work_date', d.work_date, {EMP}, 'branch', {BRANCH}, 'shift', d.detail->'shift'->>'code', 'status', d.status,
                    'first_in', d.first_in, 'last_out', d.last_out, 'net_minutes', d.net_minutes, 'late_minutes', d.late_minutes,
                    'early_minutes', d.early_minutes, 'flags', array_to_string(d.flags, ', '))"),
                days_join(&p),
                "ORDER BY d.work_date, e.employee_code",
            )
            .await?,
        },
        // r2 register: one row per employee, one column per date holding the status code.
        "r2" => {
            let dates: Vec<String> = (0..=(p.to - p.from).num_days()).map(|i| (p.from + Duration::days(i)).to_string()).collect();
            let mut columns = cols(&["employee_code", "full_name"]);
            columns.extend(dates);
            let rows = scoped(
                ctx,
                &format!("jsonb_build_object({EMP}) || jsonb_object_agg(d.work_date::text, CASE d.status WHEN 'present' THEN 'P' WHEN 'half_day' THEN 'HD'
                    WHEN 'absent' THEN 'A' WHEN 'absent_pending' THEN 'A?' WHEN 'weekly_off' THEN 'WO' WHEN 'holiday' THEN 'H'
                    WHEN 'holiday_unpaid' THEN 'HU' ELSE 'LL' END)"),
                days_join(&p),
                "GROUP BY e.id, e.employee_code, e.full_name ORDER BY e.employee_code",
            )
            .await?;
            Report { columns, rows }
        }
        // r3 late and early.
        // shortcut: penalties are the day deductions; period-end penalties join when periods (S9) are built.
        "r3" => Report {
            columns: cols(&["employee_code", "full_name", "late_marks", "late_minutes", "early_marks", "early_minutes", "deduction_days"]),
            rows: scoped(
                ctx,
                &format!("jsonb_build_object({EMP}, 'late_marks', count(*) FILTER (WHERE d.late_mark), 'late_minutes', sum(d.late_minutes),
                    'early_marks', count(*) FILTER (WHERE d.early_mark), 'early_minutes', sum(d.early_minutes), 'deduction_days', sum(d.deduction_days))"),
                days_join(&p),
                "AND (d.late_minutes > 0 OR d.early_minutes > 0) GROUP BY e.id, e.employee_code, e.full_name ORDER BY e.employee_code",
            )
            .await?,
        },
        // r4 missed punches, with the regularisations raised for that date.
        "r4" => Report {
            columns: cols(&["work_date", "employee_code", "full_name", "branch", "status", "first_in", "last_out", "finalised", "regularisations"]),
            rows: scoped(
                ctx,
                &format!("jsonb_build_object('work_date', d.work_date, {EMP}, 'branch', {BRANCH}, 'status', d.status, 'first_in', d.first_in,
                    'last_out', d.last_out, 'finalised', d.finalised_at IS NOT NULL,
                    'regularisations', (SELECT string_agg(q.kind || ': ' || q.state, ', ' ORDER BY q.created_at) FROM app.regularisation_requests q
                                        WHERE q.employee_id = e.id AND q.work_date = d.work_date))"),
                days_join(&p),
                "AND 'missed_punch' = ANY (d.flags) ORDER BY d.work_date, e.employee_code",
            )
            .await?,
        },
        // r5 exceptions: flagged punches with their review.
        // shortcut: device status joins with devices (S4, on hold).
        "r5" => Report {
            columns: cols(&["punched_at", "employee_code", "full_name", "source", "direction", "flags", "fence_distance_m", "review"]),
            rows: scoped(
                ctx,
                &format!("jsonb_build_object('punched_at', p.punched_at, {EMP}, 'source', p.source, 'direction', p.direction,
                    'flags', array_to_string(p.flags, ', '), 'fence_distance_m', p.fence_distance_m,
                    'review', (SELECT r.state FROM app.punch_reviews r WHERE r.punch_id = p.id))"),
                |qb| {
                    qb.push(" JOIN app.raw_punches p ON p.employee_id = e.id AND p.flags <> '{}' AND p.punched_at >= ")
                        .push_bind(p.from)
                        .push("::date - interval '1 day' AND p.punched_at < ")
                        .push_bind(p.to)
                        .push("::date + interval '2 days'");
                    if let Some(b) = p.branch {
                        qb.push(" AND p.branch_id = ").push_bind(b);
                    }
                },
                "ORDER BY p.punched_at, e.employee_code",
            )
            .await?,
        },
        // r23 adjustments: HR corrections (day overrides) and automatic adjustments, one `type` column apart.
        "r23" => {
            let mut rows = scoped(
                ctx,
                &format!("jsonb_build_object('type', 'hr_correction', 'date', o.work_date, {EMP}, 'kind', coalesce(o.status::text, 'fields'),
                    'detail', jsonb_build_object('net_minutes', o.net_minutes) || o.fields, 'reason', o.reason, 'created_at', o.created_at, 'voided', o.voided_at IS NOT NULL)"),
                |qb| {
                    qb.push(" JOIN app.day_overrides o ON o.employee_id = e.id AND o.work_date BETWEEN ").push_bind(p.from).push(" AND ").push_bind(p.to);
                },
                "ORDER BY o.work_date, e.employee_code",
            )
            .await?;
            rows.extend(
                scoped(
                    ctx,
                    &format!("jsonb_build_object('type', 'automatic', 'date', a.original_date, {EMP}, 'kind', a.kind,
                        'detail', jsonb_build_object('days_delta', a.days_delta, 'minutes_delta', a.minutes_delta), 'reason', a.reason, 'created_at', a.created_at, 'voided', false)"),
                    |qb| {
                        qb.push(" JOIN app.adjustments a ON a.employee_id = e.id AND a.created_at::date BETWEEN ").push_bind(p.from).push(" AND ").push_bind(p.to);
                    },
                    "ORDER BY a.created_at, e.employee_code",
                )
                .await?,
            );
            Report { columns: cols(&["type", "date", "employee_code", "full_name", "kind", "detail", "reason", "created_at", "voided"]), rows }
        }
        // r26 settings history (policy versions) and assignments recorded in the range; organisation-wide.
        _ => {
            let rows: Vec<Value> = sqlx::query_scalar(
                "SELECT doc FROM (
                   SELECT v.recorded_at AS at, jsonb_build_object('record', 'policy_version', 'policy_set', s.name, 'valid_from', v.valid_from, 'valid_to', NULL,
                     'target', NULL, 'state', v.state, 'reason', v.change_reason, 'by', v.author_id, 'decided_by', v.decided_by, 'at', v.recorded_at) AS doc
                   FROM app.policy_set_versions v JOIN app.policy_sets s ON s.id = v.policy_set_id WHERE v.recorded_at::date BETWEEN $1 AND $2
                   UNION ALL
                   SELECT a.recorded_at, jsonb_build_object('record', 'assignment', 'policy_set', s.name, 'valid_from', lower(a.valid), 'valid_to', upper(a.valid) - 1,
                     'target', a.level || ':' || a.target_id, 'state', a.state, 'reason', NULL, 'by', a.recorded_by, 'decided_by', NULL, 'at', a.recorded_at)
                   FROM app.policy_assignments a JOIN app.policy_sets s ON s.id = a.policy_set_id WHERE a.recorded_at::date BETWEEN $1 AND $2
                 ) x ORDER BY at",
            )
            .bind(p.from)
            .bind(p.to)
            .fetch_all(&mut *ctx.tx)
            .await?;
            Report { columns: cols(&["record", "policy_set", "valid_from", "valid_to", "target", "state", "reason", "by", "decided_by", "at"]), rows }
        }
    })
}

/// The report as spreadsheet rows, header first (RP8).
pub(crate) async fn sheet(ctx: &mut Ctx, code: &str, pairs: &[(String, String)]) -> ApiResult<Vec<Vec<String>>> {
    let r = build(ctx, code, pairs).await?;
    let text = |v: &Value| match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        o => o.to_string(),
    };
    let mut out = vec![r.columns.clone()];
    out.extend(r.rows.iter().map(|row| r.columns.iter().map(|c| text(&row[c])).collect()));
    Ok(out)
}

async fn respond(mut ctx: Ctx, code: &str, raw: Option<String>) -> ApiResult<Json<Value>> {
    let pairs = query_pairs(raw)?;
    let r = build(&mut ctx, code, &pairs).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "report": code, "columns": r.columns, "rows": r.rows })))
}

pub async fn r1(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r1", raw).await
}
pub async fn r2(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r2", raw).await
}
pub async fn r3(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r3", raw).await
}
pub async fn r4(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r4", raw).await
}
pub async fn r5(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r5", raw).await
}
pub async fn r23(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r23", raw).await
}
pub async fn r26(ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    respond(ctx, "r26", raw).await
}

// ---------- DB1 /dashboard/attendance ----------

/// Today by branch for the caller's scope (a manager's scope is the team), pending punch reviews, devices offline.
// shortcut: not cached (TEC-04 asks 15 minutes per caller scope); `refresh` is accepted and ignored until it is.
// shortcut: on_leave is 0 until Package 3.
pub async fn dashboard(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    ctx.scope("dashboard.attendance")?;
    if let Some((k, _)) = query_pairs(raw)?.iter().find(|(k, _)| k != "refresh") {
        return Err(AppError::unprocessable("unknown_field", "Unknown query parameter", format!("`{k}` is not supported")).into());
    }
    let day = today();
    let branches = scoped(
        &mut ctx,
        "jsonb_build_object('branch', (SELECT jsonb_build_object('id', b.id, 'short_code', b.short_code, 'name', b.name) FROM app.branches b WHERE b.id = d.branch_id),
            'present', count(*) FILTER (WHERE d.status IN ('present', 'half_day')), 'absent', count(*) FILTER (WHERE d.status IN ('absent', 'absent_pending')),
            'late', count(*) FILTER (WHERE d.late_mark), 'on_leave', 0, 'missed_punches', count(*) FILTER (WHERE 'missed_punch' = ANY (d.flags)))",
        |qb| {
            qb.push(" JOIN app.day_results d ON d.employee_id = e.id AND d.is_current AND d.work_date = ").push_bind(day);
        },
        "GROUP BY d.branch_id ORDER BY d.branch_id",
    )
    .await?;
    let pending = scoped(
        &mut ctx,
        "to_jsonb(count(*))",
        |qb| {
            qb.push(" JOIN app.punch_reviews r ON r.employee_id = e.id AND r.state = 'pending'");
        },
        "",
    )
    .await?;
    let requests = scoped(
        &mut ctx,
        "to_jsonb(count(*))",
        |qb| {
            qb.push(" JOIN app.regularisation_requests q ON q.employee_id = e.id AND q.state IN ('pending', 'escalated')");
        },
        "",
    )
    .await?;
    let offline: i64 = sqlx::query_scalar("SELECT count(*) FROM app.devices WHERE state = 'active' AND NOT is_online").fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({
        "date": day,
        "branches": branches,
        "pending_approvals": { "punch_reviews": pending.first().cloned().unwrap_or(json!(0)), "regularisations": requests.first().cloned().unwrap_or(json!(0)) },
        "devices_offline": offline,
    })))
}
