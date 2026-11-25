//! Per-request context (API_CONTRACTS §1.2, §1.3): verify the bearer token, check the session is
//! live, open the tenant transaction, and load permissions with their scope from the database.

use crate::error::{ApiError, ApiResult, CORRELATION_ID};
use crate::AppState;
use axum::extract::FromRequestParts;
use axum::http::{request::Parts, HeaderMap, Method};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::{AppError, FieldError};
use sqlx::{Postgres, Row, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub const ACCESS_TOKEN_SECONDS: u64 = 15 * 60;

/// Access token body. Carries identity only; permissions are never read from it (§1.2).
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,  // user id
    pub org: Uuid,  // organisation id
    pub sid: Uuid,  // session id
    pub kind: String,
    pub exp: u64,
}

pub fn issue_access_token(secret: &[u8], user: Uuid, org: Uuid, session: Uuid, kind: &str) -> String {
    let exp = jsonwebtoken::get_current_timestamp() + ACCESS_TOKEN_SECONDS;
    let claims = Claims { sub: user, org, sid: session, kind: kind.into(), exp };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret)).expect("HS256 encoding cannot fail")
}

/// Where a permission applies. A user's roles are merged per permission.
#[derive(Debug, Default, Clone)]
pub struct Scope {
    pub org: bool,
    pub branches: Vec<Uuid>,
    pub departments: Vec<Uuid>,
    pub chain: bool, // Manager: own reporting chain
    pub own: bool,   // Employee: own record
}

pub struct Ctx {
    pub tx: Transaction<'static, Postgres>,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub session_id: Uuid,
    pub employee_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    grants: HashMap<String, Scope>,
    /// A scheduled job, not a signed-in user: audited as actor `system`.
    system: bool,
}

fn unauthenticated() -> AppError {
    AppError::new(401, "unauthenticated", "Sign in required", "The access token is missing, invalid or expired")
}

impl FromRequestParts<AppState> for Ctx {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(unauthenticated)?;
        let claims = decode::<Claims>(token, &DecodingKey::from_secret(&state.jwt_secret), &Validation::default())
            .map_err(|_| unauthenticated())?
            .claims;
        // Vendor tokens are for /v1/vendor only (TSD §4.3).
        if claims.kind == "vendor" {
            return Err(unauthenticated().into());
        }

        let mut tx = db::begin_tenant(&state.pool, claims.org, Some(claims.sub)).await?;

        let row = sqlx::query(
            "SELECT u.state::text AS user_state, u.employee_id, o.state::text AS org_state,
                    (SELECT c.state::text FROM app.user_credentials c WHERE c.user_id = u.id) AS cred_state
             FROM app.user_sessions s
             JOIN app.users u ON u.id = s.user_id
             JOIN app.organisations o ON o.id = u.org_id
             WHERE s.id = $1 AND s.user_id = $2 AND s.revoked_at IS NULL AND s.expires_at > now()",
        )
        .bind(claims.sid)
        .bind(claims.sub)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::new(401, "session_revoked", "Signed out", "This session has ended; sign in again"))?;

        if row.get::<String, _>("user_state") != "active" {
            return Err(AppError::new(401, "session_revoked", "Signed out", "This account is not active").into());
        }
        // A temporary PIN must be changed before anything else (A4); only A4 and sign-out get through.
        let pin_routes = ["/v1/auth/employee/pin", "/v1/auth/sign-out"].contains(&parts.uri.path());
        if !pin_routes && claims.kind == "employee" && row.get::<Option<String>, _>("cred_state").as_deref() == Some("temporary") {
            return Err(AppError::new(403, "pin_change_required", "Change your PIN", "Set a new PIN to continue").into());
        }
        // Suspended organisation: reads only (VC-R4).
        if row.get::<String, _>("org_state") == "suspended" && parts.method != Method::GET {
            return Err(AppError::new(403, "organisation_suspended", "Organisation suspended", "Only viewing is allowed").into());
        }

        let grants = load_grants(&mut tx, claims.sub).await?;

        Ok(Ctx {
            tx,
            user_id: claims.sub,
            org_id: claims.org,
            session_id: claims.sid,
            employee_id: row.get("employee_id"),
            correlation_id: CORRELATION_ID.try_with(|c| c.parse().ok()).ok().flatten(),
            grants,
            system: false,
        })
    }
}

/// The user's permissions with their merged scope, read from the database (never from the token).
async fn load_grants(tx: &mut Transaction<'static, Postgres>, user: Uuid) -> ApiResult<HashMap<String, Scope>> {
    let rows = sqlx::query(
        "SELECT rp.permission_code, r.builtin_key, ur.scope_kind::text AS scope_kind,
                coalesce(array_agg(si.branch_id) FILTER (WHERE si.branch_id IS NOT NULL), '{}') AS branches,
                coalesce(array_agg(si.department_id) FILTER (WHERE si.department_id IS NOT NULL), '{}') AS departments
         FROM app.user_roles ur
         JOIN app.roles r ON r.id = ur.role_id AND r.state = 'active'
         JOIN app.role_permissions rp ON rp.role_id = r.id
         LEFT JOIN app.user_role_scope_items si ON si.user_role_id = ur.id
         WHERE ur.user_id = $1
         GROUP BY ur.id, rp.permission_code, r.builtin_key, ur.scope_kind",
    )
    .bind(user)
    .fetch_all(&mut **tx)
    .await?;
    let mut grants: HashMap<String, Scope> = HashMap::new();
    for r in rows {
        let s = grants.entry(r.get("permission_code")).or_default();
        // Built-in Manager and Employee roles are limited by who they are, not by a scope setting.
        match (r.get::<Option<String>, _>("builtin_key").as_deref(), r.get::<String, _>("scope_kind").as_str()) {
            (Some("manager"), _) => s.chain = true,
            (Some("employee"), _) => s.own = true,
            (_, "organisation") => s.org = true,
            (_, "branches") => s.branches.extend(r.get::<Vec<Uuid>, _>("branches")),
            _ => s.departments.extend(r.get::<Vec<Uuid>, _>("departments")),
        }
    }
    Ok(grants)
}

impl Ctx {
    /// For background work done on a user's behalf (bulk import): their permissions now, no session.
    /// Refused if the user is no longer active or the organisation is suspended.
    pub async fn for_user(pool: &sqlx::PgPool, org: Uuid, user: Uuid) -> ApiResult<Ctx> {
        let mut tx = db::begin_tenant(pool, org, Some(user)).await?;
        let row = sqlx::query(
            "SELECT u.employee_id FROM app.users u JOIN app.organisations o ON o.id = u.org_id
             WHERE u.id = $1 AND u.state = 'active' AND o.state = 'active'",
        )
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::forbidden)?;
        let grants = load_grants(&mut tx, user).await?;
        Ok(Ctx {
            tx,
            user_id: user,
            org_id: org,
            session_id: Uuid::nil(),
            employee_id: row.get("employee_id"),
            correlation_id: None,
            grants,
            system: false,
        })
    }

    /// Commits, then carries on in a fresh transaction as the same user (bulk import: one per row).
    pub async fn commit_and_continue(&mut self, pool: &sqlx::PgPool) -> ApiResult<()> {
        let next = db::begin_tenant(pool, self.org_id, (!self.system).then_some(self.user_id)).await?;
        std::mem::replace(&mut self.tx, next).commit().await?;
        Ok(())
    }

    /// Rolls back, then carries on in a fresh transaction as the same user.
    pub async fn rollback_and_continue(&mut self, pool: &sqlx::PgPool) -> ApiResult<()> {
        let next = db::begin_tenant(pool, self.org_id, (!self.system).then_some(self.user_id)).await?;
        std::mem::replace(&mut self.tx, next).rollback().await?;
        Ok(())
    }

    /// For internal jobs: the organisation's tenant transaction with no user and no permissions.
    pub async fn system(pool: &sqlx::PgPool, org: Uuid) -> ApiResult<Ctx> {
        Ok(Ctx {
            tx: db::begin_tenant(pool, org, None).await?,
            user_id: Uuid::nil(),
            org_id: org,
            session_id: Uuid::nil(),
            employee_id: None,
            correlation_id: CORRELATION_ID.try_with(|c| c.parse().ok()).ok().flatten(),
            grants: HashMap::new(),
            system: true,
        })
    }

    /// The caller's scope for a permission, or 403 if they don't hold it.
    pub fn scope(&self, permission: &str) -> ApiResult<Scope> {
        self.grants.get(permission).cloned().ok_or_else(|| AppError::forbidden().into())
    }

    pub fn has(&self, permission: &str) -> bool {
        self.grants.contains_key(permission)
    }

    /// Every permission held, with its merged scope (A10).
    pub fn grants(&self) -> &HashMap<String, Scope> {
        &self.grants
    }

    pub async fn commit(self) -> ApiResult<()> {
        self.tx.commit().await?;
        Ok(())
    }

    /// One audit entry (§1.8). `action` is the route's permission code.
    pub async fn audit(
        &mut self,
        action: &str,
        target_type: &str,
        target_id: Uuid,
        old_values: Option<serde_json::Value>,
        new_values: Option<serde_json::Value>,
        reason: Option<&str>,
    ) -> ApiResult<()> {
        sqlx::query(
            "INSERT INTO app.audit_log (org_id, actor_kind, actor_id, action, target_type, target_id,
                                        old_values, new_values, reason, correlation_id)
             VALUES ($1, $10::app.actor_kind, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(self.org_id)
        .bind((!self.system).then_some(self.user_id))
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(old_values)
        .bind(new_values)
        .bind(reason)
        .bind(self.correlation_id)
        .bind(if self.system { "system" } else { "user" })
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// An event for the outbox, committed with the change (DOMAIN_MODEL §7).
    pub async fn event(&mut self, event_type: &str, aggregate_type: &str, id: Uuid, payload: serde_json::Value) -> ApiResult<()> {
        sqlx::query(
            "INSERT INTO app.outbox_events (org_id, event_type, aggregate_type, aggregate_id, payload)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(self.org_id)
        .bind(event_type)
        .bind(aggregate_type)
        .bind(id)
        .bind(payload)
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// Idempotency (§1.6). Returns a stored response to replay, or None to go ahead.
    /// The key row is written in this transaction, so a failed request leaves no key behind and a
    /// concurrent duplicate waits for the first to finish, then replays its response.
    pub async fn idempotency_begin(
        &mut self,
        headers: &HeaderMap,
        route: &str,
        body: &serde_json::Value,
    ) -> ApiResult<Option<(u16, serde_json::Value)>> {
        let key = headers
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .filter(|k| (8..=100).contains(&k.len()))
            .ok_or_else(|| {
                AppError::validation(vec![FieldError::new(
                    "Idempotency-Key",
                    "required",
                    "Send an Idempotency-Key header of 8 to 100 characters",
                )])
            })?;
        // serde_json keeps object keys sorted, so the same body always hashes the same.
        let hash = format!("{:x}", Sha256::digest(format!("{route}\n{body}")));
        let inserted = sqlx::query(
            "INSERT INTO app.idempotency_keys (org_id, user_id, idempotency_key, request_hash)
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(self.org_id)
        .bind(self.user_id)
        .bind(key)
        .bind(&hash)
        .execute(&mut *self.tx)
        .await?
        .rows_affected();
        if inserted == 1 {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT request_hash, response_status, response_body FROM app.idempotency_keys
             WHERE org_id = $1 AND user_id = $2 AND idempotency_key = $3 AND created_at > now() - interval '24 hours'",
        )
        .bind(self.org_id)
        .bind(self.user_id)
        .bind(key)
        .fetch_optional(&mut *self.tx)
        .await?;
        let Some(row) = row else {
            // An expired key (older than 24 h) that J7 has not purged yet is refused rather than reused.
            return Err(AppError::unprocessable("idempotency_key_reused", "Key already used", "Use a new Idempotency-Key").into());
        };
        if row.get::<String, _>("request_hash") != hash {
            return Err(AppError::unprocessable(
                "idempotency_key_reused",
                "Key already used",
                "This Idempotency-Key was used for a different request",
            )
            .into());
        }
        match row.get::<Option<i16>, _>("response_status") {
            Some(status) => Ok(Some((status as u16, row.get::<Option<serde_json::Value>, _>("response_body").unwrap_or_default()))),
            None => Err(AppError::conflict("request_in_progress", "Still working", "The first request is still running").into()),
        }
    }

    pub async fn idempotency_finish(&mut self, headers: &HeaderMap, status: u16, body: &serde_json::Value) -> ApiResult<()> {
        let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).unwrap_or_default();
        sqlx::query(
            "UPDATE app.idempotency_keys SET response_status = $4, response_body = $5
             WHERE org_id = $1 AND user_id = $2 AND idempotency_key = $3",
        )
        .bind(self.org_id)
        .bind(self.user_id)
        .bind(key)
        .bind(status as i16)
        .bind(body)
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }
}

/// `If-Match: "<version>"` (§1.5). Missing → 428.
pub fn if_match(headers: &HeaderMap) -> ApiResult<i32> {
    headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok())
        .ok_or_else(|| {
            AppError::new(428, "precondition_required", "Version required", "Send If-Match with the record's version").into()
        })
}

pub fn stale(current: i32) -> ApiError {
    AppError::new(412, "stale_version", "Changed by someone else", "Reload the record and try again")
        .with_meta(serde_json::json!({ "current_version": current }))
        .into()
}
