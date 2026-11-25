//! Internal job routes J1–J7 (API_CONTRACTS §10). Called by the scheduler only, one organisation per call.

use crate::ctx::Ctx;
use crate::employees::{allowed, em_settings, today, Access};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use sha2::{Digest, Sha256};
use shared::AppError;
use uuid::Uuid;

/// `Authorization: Bearer <JOBS_TOKEN>`. Hashes are compared, so timing reveals nothing about the token.
/// shortcut: a shared secret, not the Google-signed OIDC token §10 asks for; verify Cloud Tasks' OIDC token once deployed on Google Cloud.
fn check_caller(s: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let sent = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let ok = match (&s.jobs_token_hash, sent) {
        (Some(want), Some(t)) => Sha256::digest(t.as_bytes()).as_slice() == &want[..],
        _ => false,
    };
    if ok { Ok(()) } else { Err(AppError::new(401, "unauthenticated", "Not allowed", "Internal job routes need the jobs token").into()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrgJob {
    org_id: Uuid,
}

/// J2: scheduled status changes due today take effect (daily 00:15).
pub async fn status_due(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: OrgJob = parse(raw)?;
    Ok(Json(crate::status::apply_due(&s.pool, job.org_id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecalcJob {
    org_id: Uuid,
    employee_id: Uuid,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    reason: String,
}

/// J-AT2: recalculate one employee's days (also run from outbox events). Safe to repeat: unchanged days are skipped.
pub async fn recalculate(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let j: RecalcJob = parse(raw)?;
    let to = j.to.min(today());
    let from = j.from.max(to - chrono::Duration::days(61));
    let changed = crate::days::recalc_range(&s, j.org_id, j.employee_id, from, to, &j.reason).await.map_err(|e| {
        tracing::warn!(error = %e, "recalculation failed");
        AppError::internal()
    })?;
    Ok(Json(json!({ "changed": changed })))
}

/// J-AT1 (hourly): finalise days whose window has passed.
pub async fn finalise(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: OrgJob = parse(raw)?;
    Ok(Json(json!({ "finalised": crate::days::finalise(&s, job.org_id).await? })))
}

/// J-AT3 (nightly): recalculate recent days (absent-pending turning absent, days without punches).
pub async fn attendance_sweep(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: OrgJob = parse(raw)?;
    let changed = crate::days::sweep(&s, job.org_id).await.map_err(|e| {
        tracing::warn!(error = %e, "attendance sweep failed");
        AppError::internal()
    })?;
    Ok(Json(json!({ "changed": changed })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportJob {
    org_id: Uuid,
    import_id: Uuid,
}

/// J5: validate and apply a bulk import's rows (SM-7). Safe to call again for the same import.
pub async fn bulk_import(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: ImportJob = parse(raw)?;
    Ok(Json(crate::imports::process(&s, job.org_id, job.import_id).await?))
}

/// J-AT6: process a punch or roster import (same body for both routes). Safe to call again.
pub async fn attendance_import(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: ImportJob = parse(raw)?;
    Ok(Json(crate::at_imports::process(&s, job.org_id, job.import_id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportJob {
    org_id: Uuid,
    export_id: Uuid,
}

/// J6: build a large export (P3 answered 202). Safe to call again for the same export.
pub async fn export(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: ExportJob = parse(raw)?;
    Ok(Json(crate::reports::process_export(&s, job.org_id, job.export_id).await?))
}

/// J3: open Probation periods whose end is EM-04 days away or less, not yet reminded for that end
/// date (AC-EM-12.1). Marks them and queues one reminder event, in one transaction, so a second run
/// the same day finds nothing; extending probation changes the date, so a new reminder follows (AC-EM-12.4).
pub async fn probation_reminders(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: OrgJob = parse(raw)?;
    Ok(Json(json!({ "reminded": remind(&s.pool, job.org_id).await? })))
}

/// J1 (every 15 minutes): J2 and J3 for every active organisation. Both are safe to repeat, so
/// running them on every tick means a missed tick never skips a day; each organisation is run on
/// its own, and one failing doesn't stop the rest.
// shortcut: runs here, FAN_OUT_PARALLEL organisations at a time, instead of one Cloud Task per organisation
// (APQ-05), and "today" is India time for all (see employees::today); move to Cloud Tasks at deployment.
pub async fn fan_out(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT app.active_org_ids()").fetch_all(&s.pool).await?;
    let (mut applied, mut reminded, mut failed) = (0, 0, Vec::new());
    let mut running = tokio::task::JoinSet::new();
    let mut pending = orgs.iter().copied();
    loop {
        while running.len() < FAN_OUT_PARALLEL {
            let Some(org) = pending.next() else { break };
            running.spawn(one_org(s.pool.clone(), org));
        }
        let Some(done) = running.join_next().await else { break };
        let (a, r, f) = done.map_err(|_| AppError::internal())?;
        applied += a;
        reminded += r;
        failed.extend(f);
    }
    if !failed.is_empty() {
        tracing::warn!(failed = ?failed, "fan-out: some organisations failed");
    }
    Ok(Json(json!({ "organisations": orgs.len(), "status_applied": applied, "reminded": reminded, "failed": failed })))
}

/// Organisations run at once by J1; leaves 4 of the API's 10 pool connections for live requests.
const FAN_OUT_PARALLEL: usize = 6;

/// J2 then J3 for one organisation: (status changes applied, employees reminded, failures).
async fn one_org(pool: sqlx::PgPool, org: Uuid) -> (usize, usize, Vec<Value>) {
    let (mut applied, mut reminded, mut failed) = (0, 0, Vec::new());
    match crate::status::apply_due(&pool, org).await {
        Ok(r) => {
            applied = r["applied"].as_array().map_or(0, Vec::len);
            if r["failed"].as_array().is_some_and(|f| !f.is_empty()) {
                failed.push(json!({ "org_id": org, "job": "status-due", "requests": r["failed"] }));
            }
        }
        Err(e) => failed.push(json!({ "org_id": org, "job": "status-due", "code": e.0.code })),
    }
    match remind(&pool, org).await {
        Ok(n) => reminded = n,
        Err(e) => failed.push(json!({ "org_id": org, "job": "probation-reminders", "code": e.0.code })),
    }
    (applied, reminded, failed)
}

/// J3 for one organisation; returns how many employees were reminded.
async fn remind(pool: &sqlx::PgPool, org: Uuid) -> ApiResult<usize> {
    let mut ctx = Ctx::system(pool, org).await?;
    let today = today();
    let em = em_settings(&mut ctx.tx, today).await?;
    let due: Vec<Value> = sqlx::query_scalar(
        "UPDATE app.employment_periods p SET probation_reminder_sent_for = p.probation_end_date
         WHERE p.state = 'open' AND p.probation_end_date IS NOT NULL
           AND p.probation_end_date - $1::int <= $2
           AND p.probation_reminder_sent_for IS DISTINCT FROM p.probation_end_date
           AND (SELECT st.status FROM app.employee_status_changes st
                WHERE st.employment_period_id = p.id AND st.superseded_at IS NULL AND st.effective_from <= $2
                ORDER BY st.effective_from DESC LIMIT 1) = 'probation'
         RETURNING jsonb_build_object('employee_id', p.employee_id, 'probation_end_date', p.probation_end_date)",
    )
    .bind(em.probation_reminder_days as i32)
    .bind(today)
    .fetch_all(&mut *ctx.tx)
    .await?;
    if !due.is_empty() {
        let payload = json!({ "employees": due });
        ctx.event("ProbationReminders", "organisation", org, payload.clone()).await?;
        ctx.audit("job.probation_reminders", "organisation", org, None, Some(payload), None).await?;
    }
    ctx.commit().await?;
    Ok(due.len())
}

/// Who hears about a probation end: active admins who may raise that employee's status change
/// (confirmation is Probation -> Active, AC-EM-12.2), each told only about employees in their scope.
async fn probation_mail(s: &AppState, org: Uuid, payload: &Value) -> Result<(), String> {
    let err = |e: crate::error::ApiError| e.0.to_string();
    let due: Vec<Uuid> = payload["employees"]
        .as_array()
        .ok_or("payload has no employees")?
        .iter()
        .filter_map(|e| e["employee_id"].as_str()?.parse().ok())
        .collect();
    let mut t = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
    let admins: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT DISTINCT u.id, u.email, u.display_name FROM app.users u
         JOIN app.user_roles ur ON ur.user_id = u.id
         JOIN app.roles r ON r.id = ur.role_id AND r.state = 'active'
         JOIN app.role_permissions rp ON rp.role_id = r.id AND rp.permission_code = 'employee.status.raise'
         WHERE u.kind = 'admin' AND u.state = 'active'",
    )
    .fetch_all(&mut *t)
    .await
    .map_err(|e| e.to_string())?;
    t.commit().await.map_err(|e| e.to_string())?;
    for (admin, to, name) in admins {
        // Suspended organisation or user since: no mail.
        let Ok(mut ctx) = Ctx::for_user(&s.pool, org, admin).await else { continue };
        let em = em_settings(&mut ctx.tx, today()).await.map_err(err)?;
        let mut lines = Vec::new();
        for id in &due {
            if !allowed(&mut ctx, *id, "employee.status.raise", Access::Edit, &em).await.map_err(err)? {
                continue;
            }
            // Still in probation with this end date? Confirmed or extended since: say nothing.
            let row: Option<(String, String, chrono::NaiveDate)> = sqlx::query_as(
                "SELECT e.full_name, e.employee_code, p.probation_end_date FROM app.employees e
                 JOIN app.employment_periods p ON p.employee_id = e.id AND p.state = 'open'
                 WHERE e.id = $1 AND p.probation_reminder_sent_for = p.probation_end_date",
            )
            .bind(id)
            .fetch_optional(&mut *ctx.tx)
            .await
            .map_err(|e| e.to_string())?;
            lines.extend(row);
        }
        ctx.commit().await.map_err(err)?;
        if !lines.is_empty() {
            let (subject, html) = crate::mail::probation_reminder(&name, &lines);
            s.mailer.send(crate::mail::Email { to, subject, html }).await?;
        }
    }
    Ok(())
}

/// J-AT8 (every 15 minutes): missed-punch reminders and the managers' digest.
pub async fn attendance_reminders(State(s): State<AppState>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let job: OrgJob = parse(raw)?;
    Ok(Json(crate::notify::reminders(&s, job.org_id).await?))
}

/// J-AT9 (daily): delete punch idempotency keys older than 90 days, selfie files older than 13 months (DEC-51;
/// the punch row stays) and attendance import files 30 days after the import finished.
// shortcut: selfies are looked for in the month before the 13-month line only (a daily job covers it); a gap of
// over a month in runs leaves older files, clear those by hand.
pub async fn attendance_cleanup(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT app.active_org_ids()").fetch_all(&s.pool).await?;
    let (mut keys, mut files) = (0u64, 0usize);
    for org in &orgs {
        let mut tx = db::begin_tenant(&s.pool, *org, None).await?;
        keys += sqlx::query("DELETE FROM app.punch_keys WHERE created_at < now() - interval '90 days'").execute(&mut *tx).await?.rows_affected();
        let old: Vec<String> = sqlx::query_scalar(
            "SELECT selfie_ref FROM app.raw_punches WHERE selfie_ref IS NOT NULL
               AND punched_at >= now() - interval '14 months' AND punched_at < now() - interval '13 months'
             UNION ALL
             SELECT file_ref FROM app.attendance_imports WHERE file_ref IS NOT NULL AND state IN ('completed', 'failed') AND completed_at < now() - interval '30 days'",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        for f in &old {
            if tokio::fs::remove_file(s.files_dir.join(f)).await.is_ok() {
                files += 1;
            }
        }
    }
    Ok(Json(json!({ "organisations": orgs.len(), "punch_keys_deleted": keys, "files_deleted": files })))
}

/// J7 (daily): purge idempotency keys older than 24 hours (section 1.6), expire exports and delete
/// uploaded import files after 30 days (TSD section 11).
// shortcut: walks active organisations only (app.active_org_ids); a suspended one is cleaned once reactivated.
pub async fn cleanup(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT app.active_org_ids()").fetch_all(&s.pool).await?;
    let (mut keys, mut exports, mut files) = (0u64, 0usize, 0usize);
    for org in &orgs {
        let mut tx = db::begin_tenant(&s.pool, *org, None).await?;
        keys += sqlx::query("DELETE FROM app.idempotency_keys WHERE created_at < now() - interval '24 hours'")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let expired: Vec<String> = sqlx::query_scalar(
            "UPDATE app.export_jobs SET state = 'expired' WHERE state = 'ready' AND expires_at < now() RETURNING file_ref",
        )
        .fetch_all(&mut *tx)
        .await?;
        exports += expired.len();
        let uploads: Vec<String> = sqlx::query_scalar(
            "SELECT file_ref FROM app.bulk_imports WHERE state IN ('completed', 'failed') AND completed_at < now() - interval '30 days'",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        for f in expired.iter().chain(&uploads) {
            // Already gone (an earlier run) is fine.
            if tokio::fs::remove_file(s.files_dir.join(f)).await.is_ok() {
                files += 1;
            }
        }
    }
    Ok(Json(json!({ "organisations": orgs.len(), "idempotency_keys_deleted": keys, "exports_expired": exports, "files_deleted": files })))
}

/// Events delivered by email. With no mailer configured they wait in the outbox instead of using up attempts.
const MAIL_EVENTS: [&str; 2] = ["AdminInvited", "ProbationReminders"];
/// After this many failures an event is left for a person to look at (`last_error`).
const MAX_ATTEMPTS: i32 = 10;

/// J4: delivers pending events, oldest first, as role app_outbox (the only role that sees every
/// organisation's events). Delivery is at least once: an event whose send succeeded but whose
/// update was lost is sent again on the next run.
/// shortcut: sends happen while the batch's rows are locked, one by one; fine for 100 per minute, move to per-event tasks if mail volume grows.
pub async fn outbox_dispatch(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    check_caller(&s, &headers)?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET LOCAL ROLE app_outbox").execute(&mut *tx).await?;
    // SKIP LOCKED: two dispatchers running at once never take the same event.
    let rows = sqlx::query(
        "SELECT id, org_id, event_type, payload FROM app.outbox_events
         WHERE dispatched_at IS NULL AND attempts < $1 AND ($2 OR event_type <> ALL($3))
         ORDER BY id LIMIT 100 FOR UPDATE SKIP LOCKED",
    )
    .bind(MAX_ATTEMPTS)
    .bind(s.mailer.is_on())
    .bind(&MAIL_EVENTS[..])
    .fetch_all(&mut *tx)
    .await?;
    let (mut done, mut failed) = (0, 0);
    for r in rows {
        let id: i64 = r.get("id");
        match deliver(&s, r.get("org_id"), r.get::<String, _>("event_type").as_str(), r.get("payload")).await {
            Ok(()) => {
                sqlx::query("UPDATE app.outbox_events SET dispatched_at = now(), last_error = NULL WHERE id = $1").bind(id).execute(&mut *tx).await?;
                done += 1;
            }
            Err(e) => {
                tracing::warn!(event_id = id, error = %e, "outbox event not delivered");
                sqlx::query("UPDATE app.outbox_events SET attempts = attempts + 1, last_error = $2 WHERE id = $1")
                    .bind(id)
                    .bind(e)
                    .execute(&mut *tx)
                    .await?;
                failed += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "dispatched": done, "failed": failed })))
}

async fn deliver(s: &AppState, org: Uuid, kind: &str, payload: Value) -> Result<(), String> {
    match kind {
        "AdminInvited" => invite(s, org, &payload).await,
        k if crate::days::RECALC_EVENTS.contains(&k) => crate::days::on_event(s, org, k, &payload).await,
        "ProbationReminders" => probation_mail(s, org, &payload).await,
        // No subscriber yet (HR notices, Attendance): delivered to nobody, marked done.
        _ => Ok(()),
    }
}

/// The invitation email. The link's token is made here and never stored (R6).
async fn invite(s: &AppState, org: Uuid, payload: &Value) -> Result<(), String> {
    let user: Uuid = payload["user_id"].as_str().and_then(|u| u.parse().ok()).ok_or("payload has no user_id")?;
    let mut t = db::begin_tenant(&s.pool, org, None).await.map_err(|e| e.to_string())?;
    let who: Option<(String, String)> = sqlx::query_as("SELECT email, display_name FROM app.users WHERE id = $1 AND kind = 'admin' AND state = 'invited'")
        .bind(user)
        .fetch_optional(&mut *t)
        .await
        .map_err(|e| e.to_string())?;
    t.commit().await.map_err(|e| e.to_string())?;
    // Accepted, suspended or gone since: nothing to send.
    let Some((to, name)) = who else { return Ok(()) };
    let link = format!("{}/accept-invite?token={}", s.app_url, crate::auth::issue_invite_token(s, user, org));
    let (subject, html) = crate::mail::invitation(&name, &link, (crate::auth::INVITE_SECONDS / 86_400) as i64);
    s.mailer.send(crate::mail::Email { to, subject, html }).await
}
