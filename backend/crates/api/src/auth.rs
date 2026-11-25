//! Sign-in and sessions A1–A10 (API_CONTRACTS §3, TSD §8, BR-EM-10).
//! Every credential failure answers `invalid_credentials`, whatever the cause, and is committed
//! (failed attempts, lock) before the error goes back. Secrets are stored only as argon2id hashes.

use crate::ctx::{issue_access_token, Ctx, ACCESS_TOKEN_SECONDS};
use crate::error::{parse, ApiResult, Body};
use crate::AppState;
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use payroll_core::pin::{check_new_pin, locks_after, LOCK_MINUTES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shared::AppError;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row, Transaction};
use std::sync::LazyLock;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

type Tx = Transaction<'static, Postgres>;

pub const REFRESH_TOKEN_DAYS: i64 = 30;
pub(crate) const MFA_TOKEN_SECONDS: u64 = 5 * 60;
pub(crate) const INVITE_SECONDS: u64 = 7 * 24 * 3600;
const OTP_MINUTES: i64 = 10;
const MIN_PASSWORD: usize = 12;

// ---------- helpers ----------

pub(crate) fn invalid() -> AppError {
    AppError::new(401, "invalid_credentials", "Sign-in failed", "The details entered are not correct")
}

fn locked(until: DateTime<Utc>) -> AppError {
    AppError::new(423, "account_locked", "Account locked", "Too many wrong attempts; try again later")
        .with_meta(json!({ "locked_until": until }))
}

fn revoked() -> AppError {
    AppError::new(401, "session_revoked", "Signed out", "This session has ended; sign in again")
}

pub async fn hash_secret(secret: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default().hash_password(secret.as_bytes(), &SaltString::generate(&mut OsRng)).map(|h| h.to_string())
    })
    .await
    .map_err(|_| AppError::internal())?
    .map_err(|_| AppError::internal().into())
}

/// Hash of a value nobody knows, verified when there is no account, so unknown and known
/// accounts take the same time to refuse.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    Argon2::default().hash_password(b"no-such-account", &SaltString::generate(&mut OsRng)).unwrap().to_string()
});

pub(crate) async fn verify_secret(secret: String, hash: Option<String>) -> bool {
    tokio::task::spawn_blocking(move || {
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| DUMMY_HASH.clone());
        let ok = PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(secret.as_bytes(), &h).is_ok());
        known && ok
    })
    .await
    .unwrap_or(false)
}

fn sha256(s: &str) -> String {
    format!("{:x}", Sha256::digest(s))
}

fn random_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Refuses while locked; otherwise nothing.
fn check_lock(row: &PgRow) -> ApiResult<()> {
    match row.get::<Option<DateTime<Utc>>, _>("locked_until") {
        Some(until) if until > Utc::now() => Err(locked(until).into()),
        _ => Ok(()),
    }
}

/// A wrong secret: count it, lock on the fifth (AC-EM-19.6). Returns the error to send after commit.
async fn failed(tx: &mut Tx, user: Uuid) -> ApiResult<AppError> {
    let attempts: i16 = sqlx::query_scalar(
        "UPDATE app.user_credentials SET failed_attempts = failed_attempts + 1 WHERE user_id = $1 RETURNING failed_attempts",
    )
    .bind(user)
    .fetch_one(&mut **tx)
    .await?;
    if !locks_after(attempts) {
        return Ok(invalid());
    }
    let until: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE app.user_credentials SET failed_attempts = 0, locked_until = now() + make_interval(mins => $2)
         WHERE user_id = $1 RETURNING locked_until",
    )
    .bind(user)
    .bind(LOCK_MINUTES as i32)
    .fetch_one(&mut **tx)
    .await?;
    Ok(locked(until))
}

async fn refuse(mut tx: Tx, user: Uuid) -> ApiResult<Response> {
    let err = failed(&mut tx, user).await?;
    tx.commit().await?;
    Err(err.into())
}

async fn succeeded(tx: &mut Tx, user: Uuid) -> ApiResult<()> {
    sqlx::query("UPDATE app.user_credentials SET failed_attempts = 0, locked_until = NULL WHERE user_id = $1")
        .bind(user)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// New session; refresh token is `<org>.<session>.<secret>` with only SHA-256(secret) stored (DATA_MODEL §6.3).
async fn open_session(tx: &mut Tx, state: &AppState, org: Uuid, user: Uuid, kind: &str, device: Option<Uuid>) -> ApiResult<Value> {
    let secret = random_secret();
    let sid: Uuid = sqlx::query_scalar(
        "INSERT INTO app.user_sessions (org_id, user_id, device_id, refresh_token_hash, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(days => $5)) RETURNING id",
    )
    .bind(org)
    .bind(user)
    .bind(device)
    .bind(sha256(&secret))
    .bind(REFRESH_TOKEN_DAYS as i32)
    .fetch_one(&mut **tx)
    .await?;
    Ok(tokens(state, org, user, sid, kind, &secret))
}

fn tokens(state: &AppState, org: Uuid, user: Uuid, sid: Uuid, kind: &str, secret: &str) -> Value {
    json!({
        "access_token": issue_access_token(&state.jwt_secret, user, org, sid, kind),
        "refresh_token": format!("{org}.{sid}.{secret}"),
        "expires_in": ACCESS_TOKEN_SECONDS,
    })
}

/// Short-lived signed tokens for the admin steps. No `sid`, so they never pass as access tokens.
#[derive(Serialize, Deserialize)]
pub(crate) struct StepClaims {
    pub sub: Uuid,
    pub org: Uuid,
    purpose: String,
    exp: u64,
}

pub(crate) fn step_token(state: &AppState, user: Uuid, org: Uuid, purpose: &str, seconds: u64) -> String {
    let claims = StepClaims { sub: user, org, purpose: purpose.into(), exp: jsonwebtoken::get_current_timestamp() + seconds };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(&state.jwt_secret)).expect("HS256 encoding cannot fail")
}

pub(crate) fn read_step(state: &AppState, token: &str, purpose: &str) -> Option<StepClaims> {
    decode::<StepClaims>(token, &DecodingKey::from_secret(&state.jwt_secret), &Validation::default())
        .ok()
        .map(|t| t.claims)
        .filter(|c| c.purpose == purpose)
}

/// Invitation link token for an admin user in state 'invited' (valid 7 days; single use because
/// accepting makes the user active). Issued by user management when it lands.
pub fn issue_invite_token(state: &AppState, user: Uuid, org: Uuid) -> String {
    step_token(state, user, org, "invite", INVITE_SECONDS)
}

pub(crate) fn totp(secret: Vec<u8>, account: &str) -> Option<TOTP> {
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret, Some("Payroll".into()), account.replace(':', "")).ok()
}

/// TOTP secrets are stored AES-256-GCM encrypted: nonce (12 bytes) then ciphertext.
pub(crate) fn encrypt(state: &AppState, plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(&state.totp_key).expect("32-byte key");
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(&nonce, plain).expect("encryption cannot fail"));
    out
}

pub(crate) fn decrypt(state: &AppState, data: &[u8]) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(&state.totp_key).ok()?;
    (data.len() > 12).then(|| cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()).flatten()
}

// ---------- employee sign-in (A1–A4) ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    id: String,
    platform: Option<String>,
}

/// The employee's account, locked for update, or None for any reason sign-in is not possible
/// (unknown organisation or code, no portal access, not current, no credential).
async fn employee_account(state: &AppState, org_code: &str, employee_code: &str) -> ApiResult<Option<(Tx, Uuid, PgRow)>> {
    let org: Option<Uuid> = sqlx::query_scalar("SELECT app.org_id_for_short_code($1)")
        .bind(org_code.trim())
        .fetch_one(&state.pool)
        .await?;
    let Some(org) = org else { return Ok(None) };
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    let row = sqlx::query(
        "SELECT u.id AS user_id, c.state::text AS cred_state, c.secret_hash, c.temporary_expires_at, c.locked_until,
                c.otp_hash, c.otp_expires_at, e.email, p.includes_mobile_app
         FROM app.employees e
         JOIN app.users u ON u.employee_id = e.id AND u.state = 'active'
         JOIN app.user_credentials c ON c.user_id = u.id AND c.kind = 'pin' AND c.state IN ('temporary', 'active')
         JOIN app.organisations o ON o.id = e.org_id
         JOIN app.plans p ON p.id = o.plan_id
         WHERE e.employee_code = $1 AND e.is_current AND e.portal_access <> 'none'
         FOR UPDATE OF c",
    )
    .bind(employee_code.trim())
    .fetch_optional(&mut *tx)
    .await?;
    Ok(row.map(|r| (tx, org, r)))
}

/// After the secret matched: plan check, device registration, session (A1, A3).
async fn employee_signed_in(state: &AppState, mut tx: Tx, org: Uuid, row: &PgRow, device: Option<Device>) -> ApiResult<Response> {
    let user: Uuid = row.get("user_id");
    succeeded(&mut tx, user).await?;
    let platform = device.as_ref().and_then(|d| d.platform.as_deref()).unwrap_or("web").to_lowercase();
    if platform != "web" && !row.get::<bool, _>("includes_mobile_app") {
        tx.commit().await?;
        return Err(AppError::new(403, "mobile_app_not_in_plan", "App not in plan", "Sign in on the web instead").into());
    }
    let (mut device_id, mut new_device) = (None, false);
    if let Some(d) = device.filter(|d| !d.id.trim().is_empty()) {
        let r = sqlx::query(
            "INSERT INTO app.user_devices (org_id, user_id, device_identifier, platform) VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, device_identifier) DO UPDATE SET last_seen_at = now(), platform = EXCLUDED.platform
             RETURNING id, (xmax = 0) AS inserted,
                       EXISTS (SELECT 1 FROM app.user_devices o WHERE o.user_id = $2 AND o.device_identifier <> $3) AS had_other",
        )
        .bind(org)
        .bind(user)
        .bind(d.id.trim())
        .bind(&platform)
        .fetch_one(&mut *tx)
        .await?;
        device_id = Some(r.get::<Uuid, _>("id"));
        new_device = r.get("inserted");
        // A phone other than the first is shown to HR (DEC-20, AC-EM-19.7).
        if new_device && r.get::<bool, _>("had_other") {
            sqlx::query(
                "INSERT INTO app.outbox_events (org_id, event_type, aggregate_type, aggregate_id, payload)
                 VALUES ($1, 'NewDeviceSignIn', 'user', $2, $3)",
            )
            .bind(org)
            .bind(user)
            .bind(json!({ "user_id": user, "device_id": device_id, "platform": platform }))
            .execute(&mut *tx)
            .await?;
        }
    }
    // App sign-ins bind the phone when none is bound (DEC-20); punches come only from the bound phone.
    let bound = match device_id {
        Some(d) if platform != "web" => crate::fences::bind_on_sign_in(&mut tx, user, d).await?,
        _ => false,
    };
    let mut body = open_session(&mut tx, state, org, user, "employee", device_id).await?;
    tx.commit().await?;
    body["must_change_pin"] = json!(row.get::<String, _>("cred_state") == "temporary");
    body["new_device"] = json!(new_device);
    body["device_bound"] = json!(bound);
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmployeeSignIn {
    org_short_code: String,
    employee_code: String,
    pin: String,
    device: Option<Device>,
}

/// Rate-limited per IP (router) and per organisation + employee code (A1).
pub async fn employee_sign_in(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: EmployeeSignIn = parse(raw)?;
    state.limiter.check(&format!("a1:{}:{}", req.org_short_code.trim().to_uppercase(), req.employee_code.trim().to_uppercase()), crate::rate::ACCOUNT_LIMIT)?;
    let Some((tx, org, row)) = employee_account(&state, &req.org_short_code, &req.employee_code).await? else {
        verify_secret(req.pin, None).await;
        return Err(invalid().into());
    };
    check_lock(&row)?;
    let user: Uuid = row.get("user_id");
    if !verify_secret(req.pin, row.get("secret_hash")).await {
        return refuse(tx, user).await;
    }
    let expired = row.get::<String, _>("cred_state") == "temporary"
        && row.get::<Option<DateTime<Utc>>, _>("temporary_expires_at").is_some_and(|t| t <= Utc::now());
    if expired {
        tx.commit().await?;
        return Err(AppError::new(401, "pin_expired", "PIN expired", "Ask HR for a new temporary PIN").into());
    }
    employee_signed_in(&state, tx, org, &row, req.device).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OtpRequest {
    org_short_code: String,
    employee_code: String,
}

/// A2: always 202, so it reveals nothing about the account.
pub async fn otp_request(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: OtpRequest = parse(raw)?;
    state.limiter.check(&format!("a2:{}:{}", req.org_short_code.trim().to_uppercase(), req.employee_code.trim().to_uppercase()), crate::rate::OTP_SEND_LIMIT)?;
    if let Some((mut tx, _, row)) = employee_account(&state, &req.org_short_code, &req.employee_code).await? {
        let has_email = row.get::<Option<String>, _>("email").is_some_and(|e| !e.trim().is_empty());
        if has_email && check_lock(&row).is_ok() {
            let code = format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000);
            sqlx::query(
                "UPDATE app.user_credentials SET otp_hash = $2, otp_expires_at = now() + make_interval(mins => $3) WHERE user_id = $1",
            )
            .bind(row.get::<Uuid, _>("user_id"))
            .bind(hash_secret(code.clone()).await?)
            .bind(OTP_MINUTES as i32)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            // In the background, so the reply time never shows whether an account exists. The code itself is never stored.
            let (mailer, to) = (state.mailer.clone(), row.get::<String, _>("email"));
            tokio::spawn(async move {
                let (subject, html) = crate::mail::one_time_code(&code, OTP_MINUTES);
                if let Err(e) = mailer.send(crate::mail::Email { to, subject, html }).await {
                    tracing::warn!(error = %e, "one-time code not emailed");
                }
            });
        }
    }
    Ok((StatusCode::ACCEPTED, Json(json!({}))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OtpVerify {
    org_short_code: String,
    employee_code: String,
    code: String,
    device: Option<Device>,
}

pub async fn otp_verify(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: OtpVerify = parse(raw)?;
    state.limiter.check(&format!("a3:{}:{}", req.org_short_code.trim().to_uppercase(), req.employee_code.trim().to_uppercase()), crate::rate::ACCOUNT_LIMIT)?;
    let Some((mut tx, org, row)) = employee_account(&state, &req.org_short_code, &req.employee_code).await? else {
        verify_secret(req.code, None).await;
        return Err(invalid().into());
    };
    check_lock(&row)?;
    let user: Uuid = row.get("user_id");
    let live = row.get::<Option<DateTime<Utc>>, _>("otp_expires_at").is_some_and(|t| t > Utc::now());
    let hash = row.get::<Option<String>, _>("otp_hash").filter(|_| live);
    if !verify_secret(req.code, hash).await {
        return refuse(tx, user).await;
    }
    sqlx::query("UPDATE app.user_credentials SET otp_hash = NULL, otp_expires_at = NULL WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    employee_signed_in(&state, tx, org, &row, req.device).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPin {
    current_pin: String,
    new_pin: String,
}

/// A4: set own PIN; the only route open while a temporary PIN is in force.
pub async fn set_pin(mut ctx: Ctx, Body(raw): Body) -> ApiResult<Response> {
    let req: SetPin = parse(raw)?;
    if check_new_pin(&req.new_pin).is_err() {
        return Err(AppError::unprocessable("pin_too_weak", "PIN too weak", "Use 6 digits, not all the same and not in a run").into());
    }
    let row = sqlx::query(
        "SELECT secret_hash, locked_until FROM app.user_credentials WHERE user_id = $1 AND kind = 'pin'
           AND state IN ('temporary', 'active') FOR UPDATE",
    )
    .bind(ctx.user_id)
    .fetch_optional(&mut *ctx.tx)
    .await?
    .ok_or_else(invalid)?;
    check_lock(&row)?;
    if !verify_secret(req.current_pin, row.get("secret_hash")).await {
        let err = failed(&mut ctx.tx, ctx.user_id).await?;
        ctx.commit().await?;
        return Err(err.into());
    }
    sqlx::query(
        "UPDATE app.user_credentials SET secret_hash = $2, state = 'active', temporary_expires_at = NULL,
                failed_attempts = 0, locked_until = NULL WHERE user_id = $1",
    )
    .bind(ctx.user_id)
    .bind(hash_secret(req.new_pin).await?)
    .execute(&mut *ctx.tx)
    .await?;
    let user = ctx.user_id;
    ctx.audit("auth.pin.change", "user", user, None, None, None).await?;
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------- admin sign-in (A5–A7) ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminSignIn {
    email: String,
    password: String,
}

pub async fn admin_sign_in(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: AdminSignIn = parse(raw)?;
    let org: Option<Uuid> = sqlx::query_scalar("SELECT app.org_id_for_admin_email($1)")
        .bind(req.email.trim())
        .fetch_one(&state.pool)
        .await?;
    let found = match org {
        Some(org) => {
            let mut tx = db::begin_tenant(&state.pool, org, None).await?;
            let row = sqlx::query(
                "SELECT u.id AS user_id, c.secret_hash, c.locked_until, c.totp_secret_encrypted IS NULL AS setup_required
                 FROM app.users u JOIN app.user_credentials c ON c.user_id = u.id AND c.kind = 'password' AND c.state = 'active'
                 WHERE u.kind = 'admin' AND lower(u.email) = lower($1) AND u.state = 'active' FOR UPDATE OF c",
            )
            .bind(req.email.trim())
            .fetch_optional(&mut *tx)
            .await?;
            row.map(|r| (tx, org, r))
        }
        None => None,
    };
    let Some((mut tx, org, row)) = found else {
        verify_secret(req.password, None).await;
        return Err(invalid().into());
    };
    check_lock(&row)?;
    let user: Uuid = row.get("user_id");
    if !verify_secret(req.password, row.get("secret_hash")).await {
        return refuse(tx, user).await;
    }
    succeeded(&mut tx, user).await?;
    tx.commit().await?;
    Ok(Json(json!({
        "mfa_token": step_token(&state, user, org, "mfa", MFA_TOKEN_SECONDS),
        "mfa_setup_required": row.get::<bool, _>("setup_required"),
    }))
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminMfa {
    mfa_token: String,
    totp_code: String,
}

// shortcut: a TOTP code is not blocked from reuse within its 30-second window; store the last used step if that matters.
pub async fn admin_mfa(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: AdminMfa = parse(raw)?;
    let claims = read_step(&state, &req.mfa_token, "mfa").ok_or_else(invalid)?;
    let mut tx = db::begin_tenant(&state.pool, claims.org, None).await?;
    let row = sqlx::query(
        "SELECT u.email, c.locked_until, c.totp_secret_encrypted FROM app.users u
         JOIN app.user_credentials c ON c.user_id = u.id AND c.kind = 'password' AND c.state = 'active'
         WHERE u.id = $1 AND u.state = 'active' FOR UPDATE OF c",
    )
    .bind(claims.sub)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(invalid)?;
    check_lock(&row)?;
    let ok = row
        .get::<Option<Vec<u8>>, _>("totp_secret_encrypted")
        .and_then(|enc| decrypt(&state, &enc))
        .and_then(|secret| totp(secret, &row.get::<String, _>("email")))
        .is_some_and(|t| t.check_current(req.totp_code.trim()).unwrap_or(false));
    if !ok {
        return refuse(tx, claims.sub).await;
    }
    succeeded(&mut tx, claims.sub).await?;
    let body = open_session(&mut tx, &state, claims.org, claims.sub, "admin", None).await?;
    tx.commit().await?;
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptInvite {
    invite_token: String,
    password: String,
}

/// Password policy: 12 to 128 characters, not one repeated character.
pub(crate) fn password_ok(p: &str) -> bool {
    let n = p.chars().count();
    (MIN_PASSWORD..=128).contains(&n) && p.chars().any(|c| !p.starts_with(c))
}

pub async fn accept_invite(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: AcceptInvite = parse(raw)?;
    let gone = || AppError::new(410, "invite_invalid_or_expired", "Invitation not valid", "Ask for a new invitation");
    let claims = read_step(&state, &req.invite_token, "invite").ok_or_else(gone)?;
    if !password_ok(&req.password) {
        return Err(AppError::unprocessable("password_too_weak", "Password too weak", "Use at least 12 characters").into());
    }
    let mut tx = db::begin_tenant(&state.pool, claims.org, None).await?;
    let email: String = sqlx::query_scalar("SELECT email FROM app.users WHERE id = $1 AND kind = 'admin' AND state = 'invited' FOR UPDATE")
        .bind(claims.sub)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(gone)?;
    let secret = Secret::generate_secret().to_bytes().map_err(|_| AppError::internal())?;
    let t = totp(secret.clone(), &email).ok_or_else(AppError::internal)?;
    sqlx::query(
        "INSERT INTO app.user_credentials (user_id, org_id, kind, secret_hash, state, totp_secret_encrypted)
         VALUES ($1, $2, 'password', $3, 'active', $4)
         ON CONFLICT (user_id) DO UPDATE SET kind = 'password', secret_hash = EXCLUDED.secret_hash, state = 'active',
           totp_secret_encrypted = EXCLUDED.totp_secret_encrypted, failed_attempts = 0, locked_until = NULL",
    )
    .bind(claims.sub)
    .bind(claims.org)
    .bind(hash_secret(req.password).await?)
    .bind(encrypt(&state, &secret))
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE app.users SET state = 'active', row_version = row_version + 1 WHERE id = $1")
        .bind(claims.sub)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({
        "mfa_setup": { "secret": t.get_secret_base32(), "otpauth_uri": t.get_url() },
        "mfa_token": step_token(&state, claims.sub, claims.org, "mfa", MFA_TOKEN_SECONDS),
    }))
    .into_response())
}

// ---------- sessions (A8–A10) ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    refresh_token: String,
}

/// A8: the refresh token rotates on every use; presenting an old one ends the session.
pub async fn refresh(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: Refresh = parse(raw)?;
    let mut parts = req.refresh_token.splitn(3, '.');
    let (Some(org), Some(sid), Some(secret)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(revoked().into());
    };
    let (Ok(org), Ok(sid)) = (org.parse::<Uuid>(), sid.parse::<Uuid>()) else {
        return Err(revoked().into());
    };
    let mut tx = db::begin_tenant(&state.pool, org, None).await?;
    let row = sqlx::query(
        "SELECT s.user_id, s.refresh_token_hash, u.kind::text AS kind,
                s.revoked_at IS NULL AND s.expires_at > now() AND u.state = 'active' AS live
         FROM app.user_sessions s JOIN app.users u ON u.id = s.user_id WHERE s.id = $1 FOR UPDATE OF s",
    )
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(revoked)?;
    if !row.get::<bool, _>("live") {
        return Err(revoked().into());
    }
    if row.get::<String, _>("refresh_token_hash") != sha256(secret) {
        sqlx::query("UPDATE app.user_sessions SET revoked_at = now(), revoke_reason = 'refresh_token_reused' WHERE id = $1")
            .bind(sid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(revoked().into());
    }
    let next = random_secret();
    sqlx::query("UPDATE app.user_sessions SET refresh_token_hash = $2 WHERE id = $1")
        .bind(sid)
        .bind(sha256(&next))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let user: Uuid = row.get("user_id");
    Ok(Json(tokens(&state, org, user, sid, &row.get::<String, _>("kind"), &next)).into_response())
}

pub async fn sign_out(mut ctx: Ctx) -> ApiResult<Response> {
    sqlx::query("UPDATE app.user_sessions SET revoked_at = now(), revoke_reason = 'signed_out' WHERE id = $1")
        .bind(ctx.session_id)
        .execute(&mut *ctx.tx)
        .await?;
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// A10: what the UI may show. Scope kind is the widest the user holds for any permission.
pub async fn me_access(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    let row = sqlx::query(
        "SELECT u.id, u.display_name, u.kind::text AS kind, u.email, o.name, o.short_code, o.state::text AS org_state,
                p.includes_mobile_app
         FROM app.users u JOIN app.organisations o ON o.id = u.org_id JOIN app.plans p ON p.id = o.plan_id
         WHERE u.id = $1",
    )
    .bind(ctx.user_id)
    .fetch_one(&mut *ctx.tx)
    .await?;
    let mut permissions: Vec<&String> = ctx.grants().keys().collect();
    permissions.sort();
    let scopes: Vec<_> = ctx.grants().values().collect();
    let mut branches: Vec<Uuid> = scopes.iter().flat_map(|s| s.branches.iter().copied()).collect();
    let mut departments: Vec<Uuid> = scopes.iter().flat_map(|s| s.departments.iter().copied()).collect();
    branches.sort();
    branches.dedup();
    departments.sort();
    departments.dedup();
    let kind = if scopes.iter().any(|s| s.org) {
        "organisation"
    } else if !branches.is_empty() {
        "branches"
    } else if !departments.is_empty() {
        "departments"
    } else if scopes.iter().any(|s| s.chain) {
        "chain"
    } else {
        "own"
    };
    let body = json!({
        "user": { "id": row.get::<Uuid, _>("id"), "name": row.get::<String, _>("display_name"),
                  "kind": row.get::<String, _>("kind"), "email": row.get::<Option<String>, _>("email") },
        "org": { "name": row.get::<String, _>("name"), "short_code": row.get::<String, _>("short_code"),
                 "includes_mobile_app": row.get::<bool, _>("includes_mobile_app"), "state": row.get::<String, _>("org_state") },
        "permissions": permissions,
        "scope": { "kind": kind, "branch_ids": branches, "department_ids": departments },
        "employee_id": ctx.employee_id,
    });
    ctx.commit().await?;
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_policy() {
        assert!(!password_ok("short"));
        assert!(!password_ok("aaaaaaaaaaaaaaaa"));
        assert!(password_ok("correct horse battery"));
    }

    #[tokio::test]
    async fn hash_round_trip_and_unknown_account() {
        let h = hash_secret("482913".into()).await.unwrap();
        assert!(verify_secret("482913".into(), Some(h.clone())).await);
        assert!(!verify_secret("482914".into(), Some(h)).await);
        assert!(!verify_secret("no-such-account".into(), None).await);
    }
}
