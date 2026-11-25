//! Portal access and PINs: E4 devices, E13 grant, E14 remove, E15 PIN reset, E16 unlock (SM-6, BR-EM-32).
//! A temporary PIN is returned once and never stored in plain text: not in the idempotency row,
//! the audit log, the outbox or the logs (AC-EM-19.2, §1.6).

use crate::auth::hash_secret;
use crate::ctx::Ctx;
use crate::employees::{allowed, em_settings, require_edit, today, Access};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use payroll_core::pin::{check_new_pin, TEMPORARY_PIN_DAYS};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::Row;
use uuid::Uuid;

/// Random 6 digits that would also pass as a chosen PIN.
fn temporary_pin() -> String {
    loop {
        let pin = format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000);
        if check_new_pin(&pin).is_ok() {
            return pin;
        }
    }
}

fn no_access() -> AppError {
    AppError::conflict("no_access", "No portal access", "This employee has no portal access")
}

/// The employee's user id when they currently hold portal access.
async fn access_user(ctx: &mut Ctx, id: Uuid) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT u.id FROM app.employees e
         JOIN app.users u ON u.employee_id = e.id AND u.state = 'active'
         JOIN app.user_credentials c ON c.user_id = u.id AND c.state IN ('temporary', 'active')
         WHERE e.id = $1 AND e.portal_access = 'employee_portal'
         FOR UPDATE OF e",
    )
    .bind(id)
    .fetch_optional(&mut *ctx.tx)
    .await?)
}

async fn revoke_sessions(ctx: &mut Ctx, user: Uuid, reason: &str) -> ApiResult<()> {
    sqlx::query("UPDATE app.user_sessions SET revoked_at = now(), revoke_reason = $2 WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user)
        .bind(reason)
        .execute(&mut *ctx.tx)
        .await?;
    Ok(())
}

/// Sets a new temporary PIN (state Temporary, lock and one-time code cleared). Returns its expiry.
async fn set_temporary(ctx: &mut Ctx, user: Uuid, pin: String) -> ApiResult<DateTime<Utc>> {
    Ok(sqlx::query_scalar(
        "INSERT INTO app.user_credentials (user_id, org_id, kind, secret_hash, state, temporary_expires_at)
         VALUES ($1, $2, 'pin', $3, 'temporary', now() + make_interval(days => $4))
         ON CONFLICT (user_id) DO UPDATE SET secret_hash = EXCLUDED.secret_hash, state = 'temporary',
           temporary_expires_at = EXCLUDED.temporary_expires_at, failed_attempts = 0, locked_until = NULL,
           otp_hash = NULL, otp_expires_at = NULL
         RETURNING temporary_expires_at",
    )
    .bind(user)
    .bind(ctx.org_id)
    .bind(hash_secret(pin).await?)
    .bind(TEMPORARY_PIN_DAYS as i32)
    .fetch_one(&mut *ctx.tx)
    .await?)
}

/// Emails the PIN when asked and the employee has an email. Sent directly after commit, never
/// through the outbox, so the PIN is never stored (AC-EM-19.2). Returns whether it went.
async fn email_pin(state: &AppState, wanted: bool, to: Option<String>, name: &str, pin: &str) -> bool {
    let Some(to) = to.filter(|e| wanted && !e.trim().is_empty()) else { return false };
    let (subject, html) = crate::mail::temporary_pin(name, pin, TEMPORARY_PIN_DAYS);
    match state.mailer.send(crate::mail::Email { to, subject, html }).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(error = %e, "temporary PIN not emailed");
            false
        }
    }
}

/// The one response that carries a PIN: never cached.
fn pin_response(status: StatusCode, pin: String, expires_at: DateTime<Utc>, emailed: bool) -> Response {
    let body = json!({ "temporary_pin": pin, "expires_at": expires_at, "emailed": emailed });
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendEmail {
    #[serde(default)]
    send_email: bool,
}

// ---------- E4 GET /employees/{id}/devices ----------

pub async fn devices(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    ctx.scope("employee.pin.issue")?;
    let em = em_settings(&mut ctx.tx, today()).await?;
    if !allowed(&mut ctx, id, "employee.pin.issue", Access::View, &em).await? {
        return Err(AppError::not_found().into());
    }
    let items: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', d.id, 'platform', d.platform,
                  'first_seen_at', d.first_seen_at, 'last_seen_at', d.last_seen_at) ORDER BY d.last_seen_at DESC), '[]')
         FROM app.user_devices d JOIN app.users u ON u.id = d.user_id WHERE u.employee_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(items))
}

// ---------- E13 POST /employees/{id}/portal-access ----------

pub async fn grant(State(state): State<AppState>, mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.pin.issue", &em).await?;
    let req: SendEmail = parse(raw.clone())?;
    // Same key again: the PIN is not replayed (§1.6).
    if ctx.idempotency_begin(&headers, &format!("POST /v1/employees/{id}/portal-access"), &raw).await?.is_some() {
        return Err(AppError::conflict("pin_already_issued", "PIN already issued", "Use PIN reset for a new PIN").into());
    }
    if access_user(&mut ctx, id).await?.is_some() {
        return Err(AppError::conflict("already_has_access", "Already has access", "This employee can already sign in").into());
    }
    let row = sqlx::query("SELECT full_name, email, is_current, portal_access::text AS portal_access FROM app.employees WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *ctx.tx)
        .await?;
    if !row.get::<bool, _>("is_current") {
        return Err(AppError::conflict("employee_not_current", "Employee has left", "Portal access is for current employees").into());
    }
    // One user per employee: reactivate the old row if there is one (SM-6).
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO app.users (org_id, kind, employee_id, display_name) VALUES ($1, 'employee', $2, $3)
         ON CONFLICT (employee_id) WHERE employee_id IS NOT NULL
           DO UPDATE SET state = 'active', display_name = EXCLUDED.display_name, row_version = app.users.row_version + 1
         RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(row.get::<String, _>("full_name"))
    .fetch_one(&mut *ctx.tx)
    .await?;
    sqlx::query(
        "INSERT INTO app.user_roles (org_id, user_id, role_id)
         SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'employee'
         ON CONFLICT (user_id, role_id) DO NOTHING",
    )
    .bind(ctx.org_id)
    .bind(user)
    .execute(&mut *ctx.tx)
    .await?;
    let pin = temporary_pin();
    let expires_at = set_temporary(&mut ctx, user, pin.clone()).await?;
    sqlx::query("UPDATE app.employees SET portal_access = 'employee_portal', row_version = row_version + 1 WHERE id = $1")
        .bind(id)
        .execute(&mut *ctx.tx)
        .await?;
    let old = json!({ "portal_access": row.get::<String, _>("portal_access") });
    ctx.audit("employee.pin.issue", "employee", id, Some(old), Some(json!({ "portal_access": "employee_portal" })), None).await?;
    ctx.event("PortalAccessGranted", "employee", id, json!({ "employee_id": id, "user_id": user })).await?;
    ctx.idempotency_finish(&headers, 201, &json!({})).await?;
    ctx.commit().await?;
    let emailed = email_pin(&state, req.send_email, row.get("email"), &row.get::<String, _>("full_name"), &pin).await;
    Ok(pin_response(StatusCode::CREATED, pin, expires_at, emailed))
}

// ---------- E14 DELETE /employees/{id}/portal-access ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    reason: Option<String>,
}

pub async fn remove(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.pin.issue", &em).await?;
    let req: Remove = parse(raw)?;
    let reason = req.reason.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    let Some(reason) = reason else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    let user = access_user(&mut ctx, id).await?.ok_or_else(no_access)?;
    sqlx::query("UPDATE app.users SET state = 'revoked', row_version = row_version + 1 WHERE id = $1")
        .bind(user)
        .execute(&mut *ctx.tx)
        .await?;
    sqlx::query("UPDATE app.user_credentials SET state = 'revoked', otp_hash = NULL, otp_expires_at = NULL WHERE user_id = $1")
        .bind(user)
        .execute(&mut *ctx.tx)
        .await?;
    revoke_sessions(&mut ctx, user, "portal access removed").await?;
    sqlx::query("UPDATE app.employees SET portal_access = 'none', row_version = row_version + 1 WHERE id = $1")
        .bind(id)
        .execute(&mut *ctx.tx)
        .await?;
    ctx.audit(
        "employee.pin.issue",
        "employee",
        id,
        Some(json!({ "portal_access": "employee_portal" })),
        Some(json!({ "portal_access": "none" })),
        Some(&reason),
    )
    .await?;
    ctx.event("PortalAccessRemoved", "employee", id, json!({ "employee_id": id, "user_id": user })).await?;
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------- E15 POST /employees/{id}/pin-reset ----------

pub async fn pin_reset(State(state): State<AppState>, mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.pin.issue", &em).await?;
    let req: SendEmail = parse(raw)?;
    let user = access_user(&mut ctx, id).await?.ok_or_else(no_access)?;
    let pin = temporary_pin();
    let expires_at = set_temporary(&mut ctx, user, pin.clone()).await?;
    revoke_sessions(&mut ctx, user, "PIN reset").await?;
    ctx.audit("employee.pin.issue", "employee", id, None, Some(json!({ "pin": "reset" })), None).await?;
    let who = sqlx::query("SELECT full_name, email FROM app.employees WHERE id = $1").bind(id).fetch_one(&mut *ctx.tx).await?;
    ctx.commit().await?;
    let emailed = email_pin(&state, req.send_email, who.get("email"), &who.get::<String, _>("full_name"), &pin).await;
    Ok(pin_response(StatusCode::OK, pin, expires_at, emailed))
}

// ---------- E16 POST /employees/{id}/unlock ----------

pub async fn unlock(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "employee.pin.issue", &em).await?;
    let user = access_user(&mut ctx, id).await?.ok_or_else(no_access)?;
    let was: Option<DateTime<Utc>> = sqlx::query_scalar(
        "UPDATE app.user_credentials c SET failed_attempts = 0, locked_until = NULL
         FROM (SELECT locked_until FROM app.user_credentials WHERE user_id = $1) old
         WHERE c.user_id = $1 RETURNING old.locked_until",
    )
    .bind(user)
    .fetch_one(&mut *ctx.tx)
    .await?;
    if was.is_some_and(|t| t > Utc::now()) {
        ctx.audit("employee.pin.issue", "employee", id, Some(json!({ "locked_until": was })), Some(json!({ "locked_until": null })), None)
            .await?;
    }
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
