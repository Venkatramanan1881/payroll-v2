//! Vendor console V1–V4 and vendor sign-in (API_CONTRACTS §9, TSD §4.3, SM-9).
//! Vendor staff are not client users: their queries run as `app_vendor`, which sees platform tables
//! only. Writes that belong to a client organisation (audit, outbox) switch to `app_api` for that
//! organisation inside the same transaction, so the change and its audit commit together.

use crate::auth::{decrypt, encrypt, invalid, password_ok, read_step, step_token, totp, verify_secret, MFA_TOKEN_SECONDS};
use crate::ctx::{if_match, stale, Claims};
use crate::error::{parse, ApiError, ApiResult, Body, CORRELATION_ID};
use crate::AppState;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::{request::Parts, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use sqlx::{PgPool, Postgres, Row, Transaction};
use totp_rs::Secret;
use uuid::Uuid;

type Tx = Transaction<'static, Postgres>;

// shortcut: no refresh token or session table for vendor staff; they sign in again after an hour,
// and disabling a vendor user takes effect on their next request. Add vendor sessions if that's too coarse.
pub const VENDOR_TOKEN_SECONDS: u64 = 3600;

async fn vendor_tx(pool: &PgPool) -> ApiResult<Tx> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL ROLE app_vendor").execute(&mut *tx).await?;
    Ok(tx)
}

/// From here on the transaction acts for one organisation as `app_api` (row-level security applies).
async fn as_tenant(tx: &mut Tx, org: Uuid) -> ApiResult<()> {
    sqlx::query("SELECT set_config('app.org_id', $1, true), set_config('app.user_id', '', true)")
        .bind(org.to_string())
        .execute(&mut **tx)
        .await?;
    sqlx::query("SET LOCAL ROLE app_api").execute(&mut **tx).await?;
    Ok(())
}

/// A signed-in, active vendor user with an `app_vendor` transaction.
pub struct Vendor {
    pub tx: Tx,
    pub id: Uuid,
}

impl FromRequestParts<AppState> for Vendor {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let unauthenticated = || AppError::new(401, "unauthenticated", "Sign in required", "The access token is missing, invalid or expired");
        let claims = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|t| decode::<Claims>(t, &DecodingKey::from_secret(&state.jwt_secret), &Validation::default()).ok())
            .map(|t| t.claims)
            .filter(|c| c.kind == "vendor")
            .ok_or_else(unauthenticated)?;
        let mut tx = vendor_tx(&state.pool).await?;
        let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.vendor_users WHERE id = $1 AND state = 'active')")
            .bind(claims.sub)
            .fetch_one(&mut *tx)
            .await?;
        if !active {
            return Err(unauthenticated().into());
        }
        Ok(Vendor { tx, id: claims.sub })
    }
}

impl Vendor {
    /// Audit row in the organisation's log, actor `vendor`. Leaves the transaction as `app_api`.
    async fn audit(&mut self, org: Uuid, action: &str, old: Option<Value>, new: Option<Value>, reason: Option<&str>) -> ApiResult<()> {
        as_tenant(&mut self.tx, org).await?;
        sqlx::query(
            "INSERT INTO app.audit_log (org_id, actor_kind, actor_id, action, target_type, target_id,
                                        old_values, new_values, reason, correlation_id)
             VALUES ($1, 'vendor', $2, $3, 'organisation', $1, $4, $5, $6, $7)",
        )
        .bind(org)
        .bind(self.id)
        .bind(action)
        .bind(old)
        .bind(new)
        .bind(reason)
        .bind(CORRELATION_ID.try_with(|c| c.parse::<Uuid>().ok()).ok().flatten())
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }
}

// ---------- vendor users (bootstrap) and sign-in ----------

/// Creates a vendor user with a password and an authenticator secret. Used by the `vendor_user`
/// command (there is no route: vendor staff are added by the vendor's own operators).
/// Returns (id, base32 secret, otpauth URI).
pub async fn create_vendor_user(state: &AppState, email: &str, name: &str, password: String) -> ApiResult<(Uuid, String, String)> {
    let email = email.trim();
    if !password_ok(&password) {
        return Err(AppError::unprocessable("password_too_weak", "Password too weak", "Use at least 12 characters").into());
    }
    let secret = Secret::generate_secret().to_bytes().map_err(|_| AppError::internal())?;
    let t = totp(secret.clone(), email).ok_or_else(AppError::internal)?;
    let mut tx = vendor_tx(&state.pool).await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.vendor_users (email, display_name, password_hash, totp_secret_encrypted) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(email)
    .bind(name.trim())
    .bind(crate::auth::hash_secret(password).await?)
    .bind(encrypt(state, &secret))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((id, t.get_secret_base32(), t.get_url()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignIn {
    email: String,
    password: String,
}

/// vendor_users has no failed-attempt lock, so the per-email rate limit is the brake on guessing.
pub async fn sign_in(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: SignIn = parse(raw)?;
    state.limiter.check(&format!("vendor:{}", req.email.trim().to_lowercase()), crate::rate::ACCOUNT_LIMIT)?;
    let mut tx = vendor_tx(&state.pool).await?;
    let row = sqlx::query(
        "SELECT id, password_hash FROM app.vendor_users
         WHERE lower(email) = lower($1) AND state = 'active' AND totp_secret_encrypted IS NOT NULL",
    )
    .bind(req.email.trim())
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let hash = row.as_ref().and_then(|r| r.get::<Option<String>, _>("password_hash"));
    let ok = verify_secret(req.password, hash).await; // runs even with no account, so timing doesn't tell
    let id: Uuid = row.filter(|_| ok).map(|r| r.get("id")).ok_or_else(invalid)?;
    if crate::auth::mfa_disabled() {
        return vendor_token(&state, id);
    }
    Ok(Json(json!({ "mfa_token": step_token(&state, id, Uuid::nil(), "vendor_mfa", MFA_TOKEN_SECONDS) })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mfa {
    mfa_token: String,
    totp_code: String,
}

pub async fn mfa(State(state): State<AppState>, Body(raw): Body) -> ApiResult<Response> {
    let req: Mfa = parse(raw)?;
    let claims = read_step(&state, &req.mfa_token, "vendor_mfa").ok_or_else(invalid)?;
    state.limiter.check(&format!("vendor-mfa:{}", claims.sub), crate::rate::ACCOUNT_LIMIT)?;
    let mut tx = vendor_tx(&state.pool).await?;
    let row = sqlx::query("SELECT email, totp_secret_encrypted FROM app.vendor_users WHERE id = $1 AND state = 'active'")
        .bind(claims.sub)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(invalid)?;
    tx.commit().await?;
    let ok = row
        .get::<Option<Vec<u8>>, _>("totp_secret_encrypted")
        .and_then(|enc| decrypt(&state, &enc))
        .and_then(|secret| totp(secret, &row.get::<String, _>("email")))
        .is_some_and(|t| t.check_current(req.totp_code.trim()).unwrap_or(false));
    if !ok {
        return Err(invalid().into());
    }
    vendor_token(&state, claims.sub)
}

fn vendor_token(state: &AppState, id: Uuid) -> ApiResult<Response> {
    let exp = jsonwebtoken::get_current_timestamp() + VENDOR_TOKEN_SECONDS;
    let claims = Claims { sub: id, org: Uuid::nil(), sid: Uuid::nil(), kind: "vendor".into(), exp };
    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(&state.jwt_secret)).map_err(|_| AppError::internal())?;
    Ok(Json(json!({ "access_token": token, "expires_in": VENDOR_TOKEN_SECONDS })).into_response())
}

// ---------- organisations (V1–V4) ----------

/// One organisation as the console shows it: platform fields and a headcount, no client data.
const ORG: &str = "SELECT jsonb_build_object(
        'id', o.id, 'short_code', o.short_code, 'name', o.name, 'state', o.state,
        'plan', jsonb_build_object('id', p.id, 'code', p.code, 'name', p.name),
        'employee_limit', o.employee_limit, 'limit_behaviour', o.limit_behaviour,
        'employee_count', coalesce(c.employees, 0), 'created_at', o.created_at, 'version', o.row_version)
    FROM app.organisations o
    JOIN app.plans p ON p.id = o.plan_id
    LEFT JOIN app.org_employee_counts() c ON c.org_id = o.id";

async fn org_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar(&format!("{ORG} WHERE o.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

fn response(status: StatusCode, doc: Value) -> Response {
    crate::employees::employee_response(status, doc)
}

async fn check_plan(tx: &mut Tx, plan: Uuid, errs: &mut Vec<FieldError>) -> ApiResult<()> {
    let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app.plans WHERE id = $1 AND state = 'active')")
        .bind(plan)
        .fetch_one(&mut **tx)
        .await?;
    if !ok {
        errs.push(FieldError::new("plan_id", "invalid_value", "No active plan with this id"));
    }
    Ok(())
}

fn check_limit_fields(limit: Option<i32>, behaviour: Option<&str>, errs: &mut Vec<FieldError>) {
    if limit.is_some_and(|l| l <= 0) {
        errs.push(FieldError::new("employee_limit", "invalid_value", "Must be at least 1"));
    }
    if behaviour.is_some_and(|b| !["block", "allow_and_alert"].contains(&b)) {
        errs.push(FieldError::new("limit_behaviour", "invalid_value", "Use block or allow_and_alert"));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOrg {
    short_code: String,
    name: String,
    plan_id: Uuid,
    employee_limit: i32,
    limit_behaviour: String,
    admin_email: String,
    admin_name: String,
}

/// V1. Provisions the organisation and queues the first Org Admin's invitation (sent by J4).
// shortcut: no Idempotency-Key storage here (that table is per organisation); a retried create
// gets 409 short_code_in_use instead of the first response. Store vendor keys if clients need replay.
pub async fn create(mut v: Vendor, Body(raw): Body) -> ApiResult<Response> {
    let req: NewOrg = parse(raw)?;
    let (code, name, email, admin) = (req.short_code.trim(), req.name.trim(), req.admin_email.trim(), req.admin_name.trim());
    let mut errs = Vec::new();
    if !(2..=6).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        errs.push(FieldError::new("short_code", "invalid_value", "2 to 6 capital letters or digits"));
    }
    if name.is_empty() {
        errs.push(FieldError::new("name", "required", "Give a name"));
    }
    let at = email.find('@');
    if !at.is_some_and(|i| i > 0 && i < email.len() - 1 && !email.contains(char::is_whitespace)) {
        errs.push(FieldError::new("admin_email", "invalid_value", "Give a valid email address"));
    }
    if admin.is_empty() {
        errs.push(FieldError::new("admin_name", "required", "Give the admin's name"));
    }
    check_limit_fields(Some(req.employee_limit), Some(&req.limit_behaviour), &mut errs);
    check_plan(&mut v.tx, req.plan_id, &mut errs).await?;
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let org: Uuid = sqlx::query_scalar("SELECT app.provision_organisation($1, $2, $3, $4, $5, $6::app.limit_behaviour, $7, $8)")
        .bind(v.id)
        .bind(code)
        .bind(name)
        .bind(req.plan_id)
        .bind(req.employee_limit)
        .bind(&req.limit_behaviour)
        .bind(email)
        .bind(admin)
        .fetch_one(&mut *v.tx)
        .await?;
    as_tenant(&mut v.tx, org).await?;
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM app.users WHERE kind = 'admin' AND state = 'invited'")
        .fetch_one(&mut *v.tx)
        .await?;
    sqlx::query(
        "INSERT INTO app.outbox_events (org_id, event_type, aggregate_type, aggregate_id, payload)
         VALUES ($1, 'AdminInvited', 'user', $2, jsonb_build_object('user_id', $2))",
    )
    .bind(org)
    .bind(admin_id)
    .execute(&mut *v.tx)
    .await?;
    v.tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "org_id": org, "first_admin_user_id": admin_id }))).into_response())
}

/// V2. Counts only, no employee data.
// shortcut: no paging; the vendor has tens of organisations, not thousands.
pub async fn list(mut v: Vendor) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = sqlx::query_scalar(&format!("{ORG} ORDER BY o.short_code")).fetch_all(&mut *v.tx).await?;
    v.tx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

/// Locks the organisation and checks If-Match.
async fn locked(v: &mut Vendor, id: Uuid, headers: &HeaderMap) -> ApiResult<Value> {
    let expected = if_match(headers)?;
    let version: i32 = sqlx::query_scalar("SELECT row_version FROM app.organisations WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *v.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if version != expected {
        return Err(stale(version));
    }
    org_doc(&mut v.tx, id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrgPatch {
    employee_limit: Option<i32>,
    limit_behaviour: Option<String>,
    plan_id: Option<Uuid>,
}

/// V3. Plan, limit and limit behaviour. Lowering the limit below the headcount is allowed: it only
/// stops further joiners (or alerts), per the limit behaviour.
pub async fn patch(mut v: Vendor, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let req: OrgPatch = parse(raw)?;
    let mut errs = Vec::new();
    check_limit_fields(req.employee_limit, req.limit_behaviour.as_deref(), &mut errs);
    if let Some(plan) = req.plan_id {
        check_plan(&mut v.tx, plan, &mut errs).await?;
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let old = locked(&mut v, id, &headers).await?;
    let changed = req.employee_limit.is_some_and(|l| old["employee_limit"] != l)
        || req.limit_behaviour.as_ref().is_some_and(|b| old["limit_behaviour"] != b.as_str())
        || req.plan_id.is_some_and(|p| old["plan"]["id"] != p.to_string());
    if !changed {
        v.tx.commit().await?;
        return Ok(response(StatusCode::OK, old));
    }
    sqlx::query(
        "UPDATE app.organisations SET employee_limit = coalesce($2, employee_limit),
           limit_behaviour = coalesce($3::app.limit_behaviour, limit_behaviour), plan_id = coalesce($4, plan_id),
           row_version = row_version + 1
         WHERE id = $1",
    )
    .bind(id)
    .bind(req.employee_limit)
    .bind(req.limit_behaviour)
    .bind(req.plan_id)
    .execute(&mut *v.tx)
    .await?;
    let doc = org_doc(&mut v.tx, id).await?;
    let pick = |d: &Value| json!({ "plan_id": d["plan"]["id"], "employee_limit": d["employee_limit"], "limit_behaviour": d["limit_behaviour"] });
    v.audit(id, "org.update", Some(pick(&old)), Some(pick(&doc)), None).await?;
    v.tx.commit().await?;
    Ok(response(StatusCode::OK, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

pub async fn suspend(v: Vendor, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(v, id, &headers, raw, "suspended").await
}

pub async fn reactivate(v: Vendor, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    set_state(v, id, &headers, raw, "active").await
}

/// V4, SM-9: Active ⇄ Suspended. While suspended its users can only view (VC-R4, enforced in Ctx).
async fn set_state(mut v: Vendor, id: Uuid, headers: &HeaderMap, raw: Value, to: &str) -> ApiResult<Response> {
    let reason = parse::<Reason>(raw)?.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let Some(reason) = reason else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    let old = locked(&mut v, id, headers).await?;
    if old["state"] == to {
        v.tx.commit().await?;
        return Ok(response(StatusCode::OK, old)); // already there
    }
    sqlx::query("UPDATE app.organisations SET state = $2::app.org_state, row_version = row_version + 1 WHERE id = $1")
        .bind(id)
        .bind(to)
        .execute(&mut *v.tx)
        .await?;
    let doc = org_doc(&mut v.tx, id).await?;
    let action = if to == "suspended" { "org.suspend" } else { "org.reactivate" };
    v.audit(id, action, Some(json!({ "state": old["state"] })), Some(json!({ "state": to })), Some(&reason)).await?;
    v.tx.commit().await?;
    Ok(response(StatusCode::OK, doc))
}
