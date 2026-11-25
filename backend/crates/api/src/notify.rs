//! Notifications N1–N2 and J-AT8 reminders (API_CONTRACTS Package 2 §10, §12, BRD §9.14, US-AT-42).
//! Only the in-app channel is delivered here; an org switches an event off per channel and recipient,
//! and no stored row means on.
// shortcut: the email channel is stored in settings but not sent yet; send through crate::mail when HR asks for it.

use crate::ctx::Ctx;
use crate::employees::{query_pairs, today, Tx};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::{Path, RawQuery};
use axum::Json;
use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::Row;
use uuid::Uuid;

const MANAGE: &str = "notification.settings.manage";
const CHANNELS: [&str; 2] = ["email", "in_app"];
const RECIPIENTS: [&str; 4] = ["employee", "manager", "hr_manager", "org_admin"];
/// Local hour after which the next-morning reminder and the manager digest go out.
// shortcut: a fixed 09:00 India time; make it a setting if organisations ask.
const MORNING_HOUR: u32 = 9;

/// Adds an in-app notification unless the event is switched off for this recipient, or the user already has one
/// whose payload contains `key` (so a job that runs every 15 minutes sends each reminder once).
// shortcut: two job runs at the same instant could both insert; fine for reminders, add a unique key if it matters.
pub(crate) async fn notify(tx: &mut Tx, org: Uuid, user: Uuid, event: &str, recipient: &str, key: &Value, payload: &Value) -> ApiResult<bool> {
    let added = sqlx::query(
        "INSERT INTO app.in_app_notifications (org_id, user_id, event, payload)
         SELECT $1, $2, $3, $5
         WHERE NOT EXISTS (SELECT 1 FROM app.notification_settings WHERE event = $3 AND channel = 'in_app' AND recipient = $4 AND NOT enabled)
           AND NOT EXISTS (SELECT 1 FROM app.in_app_notifications WHERE user_id = $2 AND event = $3 AND payload @> $6)",
    )
    .bind(org)
    .bind(user)
    .bind(event)
    .bind(recipient)
    .bind(payload)
    .bind(key)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(added > 0)
}

// ---------- N1 /notification-settings ----------

const SETTINGS: &str = "SELECT coalesce(jsonb_agg(jsonb_build_object('event', event, 'channel', channel, 'recipient', recipient,
        'enabled', enabled, 'updated_at', updated_at) ORDER BY event, channel, recipient), '[]') FROM app.notification_settings";

pub async fn settings(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    ctx.scope(MANAGE)?;
    let rows: Value = sqlx::query_scalar(SETTINGS).fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "rows": rows })))
}

pub async fn settings_put(mut ctx: Ctx, Body(raw): Body) -> ApiResult<Json<Value>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Row {
        event: String,
        channel: String,
        recipient: String,
        enabled: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Req {
        rows: Vec<Row>,
    }
    ctx.scope(MANAGE)?;
    let req: Req = parse(raw)?;
    let mut errs = Vec::new();
    for (i, r) in req.rows.iter().enumerate() {
        if r.event.is_empty() || r.event.len() > 60 || !r.event.chars().all(|c| c.is_ascii_alphabetic()) {
            errs.push(FieldError::new(format!("rows[{i}].event"), "invalid_value", "An event name in letters only, e.g. MissedPunch"));
        }
        if !CHANNELS.contains(&r.channel.as_str()) {
            errs.push(FieldError::new(format!("rows[{i}].channel"), "invalid_value", "email or in_app"));
        }
        if r.recipient != "hr" && !RECIPIENTS.contains(&r.recipient.as_str()) {
            errs.push(FieldError::new(format!("rows[{i}].recipient"), "invalid_value", "employee, manager, hr or org_admin"));
        }
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let old: Value = sqlx::query_scalar(SETTINGS).fetch_one(&mut *ctx.tx).await?;
    for r in &req.rows {
        // The contract says "hr"; the table stores the role name.
        let recipient = if r.recipient == "hr" { "hr_manager" } else { &r.recipient };
        sqlx::query(
            "INSERT INTO app.notification_settings (org_id, event, channel, recipient, enabled) VALUES ($1, $2, $3::app.notification_channel, $4, $5)
             ON CONFLICT (org_id, event, channel, recipient) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now(), updated_by = app.current_user_id()",
        )
        .bind(ctx.org_id)
        .bind(&r.event)
        .bind(&r.channel)
        .bind(recipient)
        .bind(r.enabled)
        .execute(&mut *ctx.tx)
        .await?;
    }
    let rows: Value = sqlx::query_scalar(SETTINGS).fetch_one(&mut *ctx.tx).await?;
    let org = ctx.org_id;
    ctx.audit(MANAGE, "organisation", org, Some(old), Some(rows.clone()), None).await?;
    ctx.commit().await?;
    Ok(Json(json!({ "rows": rows })))
}

// ---------- N2 /me/notifications ----------

// shortcut: the latest 100, no paging yet.
pub async fn mine(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    let unread = query_pairs(raw)?.iter().any(|(k, v)| k == "unread" && v == "true");
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id', id, 'event', event, 'payload', payload, 'created_at', created_at, 'read_at', read_at)
         FROM app.in_app_notifications WHERE user_id = $1 AND (NOT $2 OR read_at IS NULL) ORDER BY created_at DESC LIMIT 100",
    )
    .bind(ctx.user_id)
    .bind(unread)
    .fetch_all(&mut *ctx.tx)
    .await?;
    let unread_count: i64 = sqlx::query_scalar("SELECT count(*) FROM app.in_app_notifications WHERE user_id = $1 AND read_at IS NULL")
        .bind(ctx.user_id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items, "unread_count": unread_count })))
}

pub async fn read(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let found: Option<Option<DateTime<Utc>>> = sqlx::query_scalar(
        "UPDATE app.in_app_notifications SET read_at = coalesce(read_at, now()) WHERE id = $1 AND user_id = $2 RETURNING read_at",
    )
    .bind(id)
    .bind(ctx.user_id)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let read_at = found.ok_or_else(AppError::not_found)?;
    ctx.commit().await?;
    Ok(Json(json!({ "id": id, "read_at": read_at })))
}

pub async fn read_all(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    let n = sqlx::query("UPDATE app.in_app_notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL")
        .bind(ctx.user_id)
        .execute(&mut *ctx.tx)
        .await?
        .rows_affected();
    ctx.commit().await?;
    Ok(Json(json!({ "marked": n })))
}

// ---------- J-AT8 reminders ----------

/// MP-04 missed-punch reminders (shift end + X minutes, and next morning) and the managers' daily digest of
/// pending punch reviews (NT-06). Each goes out once; the job is safe to run every 15 minutes.
// shortcut: the digest counts punch reviews only; regularisations join it with S8.
pub async fn reminders(s: &AppState, org: Uuid) -> ApiResult<Value> {
    let mut tx = db::begin_tenant(&s.pool, org, None).await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()").fetch_one(&mut *tx).await?;
    let rows = sqlx::query(
        "SELECT d.employee_id, d.work_date, u.id AS user_id,
                ((d.work_date + (d.detail->'shift'->>'end')::time) AT TIME ZONE (d.detail->>'local_zone'))
                  + CASE WHEN (d.detail->'shift'->>'end')::time <= (d.detail->'shift'->>'start')::time THEN interval '1 day' ELSE interval '0' END AS shift_end,
                ((d.work_date + 1 + make_time($2, 0, 0)) AT TIME ZONE (d.detail->>'local_zone')) AS morning
         FROM app.day_results d JOIN app.users u ON u.employee_id = d.employee_id AND u.state = 'active'
         WHERE d.is_current AND d.finalised_at IS NULL AND 'missed_punch' = ANY (d.flags)
           AND d.work_date >= $1 AND d.detail->'shift'->>'end' IS NOT NULL",
    )
    .bind(today() - Duration::days(2))
    .bind(MORNING_HOUR as i32)
    .fetch_all(&mut *tx)
    .await?;
    let mut sent = 0;
    for r in rows {
        let (emp, date, user): (Uuid, NaiveDate, Uuid) = (r.get("employee_id"), r.get("work_date"), r.get("user_id"));
        let st = crate::policies::settings_for(&mut tx, emp, date).await?;
        if !st.missed_punch_reminders {
            continue;
        }
        let shift_end: DateTime<Utc> = r.get("shift_end");
        let due = [("shift_end", shift_end + Duration::minutes(st.reminder_after_minutes as i64)), ("next_morning", r.get("morning"))];
        for (kind, at) in due {
            if now >= at {
                let key = json!({ "work_date": date, "kind": kind });
                sent += notify(&mut tx, org, user, "MissedPunch", "employee", &key, &key).await? as i32;
            }
        }
    }
    let mut digests = 0;
    if (now + Duration::minutes(330)).hour() >= MORNING_HOUR {
        let pending = sqlx::query(
            "SELECT u.id, count(*) AS n FROM app.punch_reviews r
             JOIN app.employee_job_versions jv ON jv.employee_id = r.employee_id AND jv.state = 'active' AND jv.valid @> $1::date
             JOIN app.users u ON u.employee_id = jv.reporting_manager_id AND u.state = 'active'
             WHERE r.state = 'pending' GROUP BY u.id",
        )
        .bind(today())
        .fetch_all(&mut *tx)
        .await?;
        for p in pending {
            let key = json!({ "date": today() });
            let payload = json!({ "date": today(), "pending_punch_reviews": p.get::<i64, _>("n") });
            digests += notify(&mut tx, org, p.get("id"), "ApprovalsPending", "manager", &key, &payload).await? as i32;
        }
    }
    tx.commit().await?;
    Ok(json!({ "missed_punch_reminders": sent, "manager_digests": digests }))
}
