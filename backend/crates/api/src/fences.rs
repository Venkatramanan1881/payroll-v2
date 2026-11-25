//! Geo-fences F1–F5 (API_CONTRACTS Package 2 §5.2, US-AT-13) and phone binding P3 (US-AT-14, DEC-20).
//! A fence's position lives in insert-only versions from a moment in time; a punch is checked against the
//! version in force at its punch time (BR-AT-09). `in_use` false in a version switches the fence off.

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{check_item, em_settings, require_edit, today, Tx};
use crate::error::{parse, ApiResult, Body};
use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use payroll_core::punches::distance_m;
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use uuid::Uuid;

const MANAGE: &str = "attendance.fence.manage";
const MAX_PER_BRANCH: i64 = 10;

const FENCE: &str = "jsonb_build_object('id', f.id, 'branch_id', f.branch_id, 'name', f.name, 'state', f.state, 'version', f.row_version,
    'latitude', v.latitude, 'longitude', v.longitude, 'radius_m', v.radius_m, 'valid_from', v.valid_from)";
/// The latest version of fence `f` (the one a change or test works from).
const LATEST: &str = "JOIN LATERAL (SELECT * FROM app.geo_fence_versions w WHERE w.fence_id = f.id ORDER BY w.valid_from DESC LIMIT 1) v ON true";

fn in_branch(ctx: &Ctx, branch: Uuid) -> ApiResult<()> {
    let s = ctx.scope(MANAGE)?;
    if s.org || s.branches.contains(&branch) { Ok(()) } else { Err(AppError::forbidden().into()) }
}

async fn fence_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("SELECT {FENCE} FROM app.geo_fences f {LATEST} WHERE f.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

fn reply(status: StatusCode, doc: Value) -> Response {
    (status, [(header::ETAG, format!("\"{}\"", doc["version"]))], Json(doc)).into_response()
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Place {
    name: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    radius_m: Option<i32>,
}

fn place_errors(p: &Place, errs: &mut Vec<FieldError>) {
    if p.latitude.is_some_and(|l| !(-90.0..=90.0).contains(&l)) {
        errs.push(FieldError::new("latitude", "invalid_value", "-90 to 90"));
    }
    if p.longitude.is_some_and(|l| !(-180.0..=180.0).contains(&l)) {
        errs.push(FieldError::new("longitude", "invalid_value", "-180 to 180"));
    }
    if p.radius_m.is_some_and(|r| !(20..=5000).contains(&r)) {
        errs.push(FieldError::new("radius_m", "invalid_value", "20 to 5,000 metres"));
    }
    if p.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        errs.push(FieldError::new("name", "required", "This can't be blank"));
    }
}

/// Locks the fence, checks scope and If-Match. Returns (branch, state).
async fn lock_fence(ctx: &mut Ctx, id: Uuid, expected: i32) -> ApiResult<(Uuid, String)> {
    let row: Option<(Uuid, String, i32)> = sqlx::query_as("SELECT branch_id, state::text, row_version FROM app.geo_fences WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *ctx.tx)
        .await?;
    let Some((branch, state, v)) = row else { return Err(AppError::not_found().into()) };
    in_branch(ctx, branch)?;
    if v != expected {
        return Err(stale(v));
    }
    Ok((branch, state))
}

/// A new version copying the latest one, with the given changes.
async fn new_version(tx: &mut Tx, id: Uuid, p: &Place, in_use: bool) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO app.geo_fence_versions (org_id, fence_id, latitude, longitude, radius_m, in_use)
         SELECT org_id, fence_id, coalesce($2::float8, latitude), coalesce($3::float8, longitude), coalesce($4, radius_m), $5
         FROM app.geo_fence_versions WHERE fence_id = $1 ORDER BY valid_from DESC LIMIT 1",
    )
    .bind(id)
    .bind(p.latitude)
    .bind(p.longitude)
    .bind(p.radius_m)
    .bind(in_use)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE app.geo_fences SET row_version = row_version + 1 WHERE id = $1").bind(id).execute(&mut **tx).await?;
    Ok(())
}

// ---------- F1 / F2 ----------

pub async fn list(mut ctx: Ctx, Path(branch): Path<Uuid>) -> ApiResult<Json<Value>> {
    in_branch(&ctx, branch)?;
    let items: Value = sqlx::query_scalar(&format!(
        "SELECT coalesce(jsonb_agg({FENCE} ORDER BY lower(f.name)), '[]') FROM app.geo_fences f {LATEST} WHERE f.branch_id = $1"
    ))
    .bind(branch)
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn create(mut ctx: Ctx, Path(branch): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    in_branch(&ctx, branch)?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, &format!("POST /v1/attendance/branches/{branch}/fences"), &raw).await? {
        return Ok((StatusCode::from_u16(s).unwrap_or(StatusCode::OK), Json(b)).into_response());
    }
    let p: Place = parse(raw)?;
    let mut errs = Vec::new();
    for (k, missing) in [("name", p.name.is_none()), ("latitude", p.latitude.is_none()), ("longitude", p.longitude.is_none())] {
        if missing {
            errs.push(FieldError::new(k, "required", "This is required"));
        }
    }
    place_errors(&p, &mut errs);
    check_item(&mut ctx.tx, "branches", Some(branch), "branch_id", &mut errs).await?;
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    // Serialise fence counting per branch.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))").bind(branch).execute(&mut *ctx.tx).await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app.geo_fences WHERE branch_id = $1 AND state <> 'inactive'").bind(branch).fetch_one(&mut *ctx.tx).await?;
    if n >= MAX_PER_BRANCH {
        return Err(AppError::conflict("fence_limit_reached", "Fence limit reached", format!("A branch can have {MAX_PER_BRANCH} fences")).into());
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.geo_fences (org_id, branch_id, name) VALUES ($1, $2, $3) RETURNING id")
        .bind(ctx.org_id)
        .bind(branch)
        .bind(p.name.as_deref().map(str::trim))
        .fetch_one(&mut *ctx.tx)
        .await?;
    sqlx::query("INSERT INTO app.geo_fence_versions (org_id, fence_id, latitude, longitude, radius_m, in_use) VALUES ($1, $2, $3::float8, $4::float8, $5, false)")
        .bind(ctx.org_id)
        .bind(id)
        .bind(p.latitude)
        .bind(p.longitude)
        .bind(p.radius_m.unwrap_or(200))
        .execute(&mut *ctx.tx)
        .await?;
    let doc = fence_doc(&mut ctx.tx, id).await?;
    ctx.audit(MANAGE, "geo_fence", id, None, Some(doc.clone()), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(reply(StatusCode::CREATED, doc))
}

// ---------- F3 PATCH: a moved or resized fence is a new version from now (AC-AT-13.3) ----------

pub async fn patch(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let p: Place = parse(raw)?;
    let mut errs = Vec::new();
    place_errors(&p, &mut errs);
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let (_, state) = lock_fence(&mut ctx, id, expected).await?;
    let old = fence_doc(&mut ctx.tx, id).await?;
    if let Some(n) = p.name.as_deref() {
        sqlx::query("UPDATE app.geo_fences SET name = $2 WHERE id = $1").bind(id).bind(n.trim()).execute(&mut *ctx.tx).await?;
    }
    let moved = [p.latitude.map(|x| (x, &old["latitude"])), p.longitude.map(|x| (x, &old["longitude"]))]
        .into_iter()
        .flatten()
        .any(|(x, o)| o.as_str().and_then(|s| s.parse::<f64>().ok()).or(o.as_f64()) != Some(x))
        || p.radius_m.is_some_and(|r| old["radius_m"].as_i64() != Some(r as i64));
    if moved {
        new_version(&mut ctx.tx, id, &p, state == "active").await?;
    } else if p.name.is_some() {
        sqlx::query("UPDATE app.geo_fences SET row_version = row_version + 1 WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
    }
    let doc = fence_doc(&mut ctx.tx, id).await?;
    ctx.audit(MANAGE, "geo_fence", id, Some(old), Some(doc.clone()), None).await?;
    ctx.commit().await?;
    Ok(reply(StatusCode::OK, doc))
}

// ---------- F4 test (read-only, AC-AT-13.2) ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    latitude: f64,
    longitude: f64,
    accuracy_m: Option<f64>,
}

pub async fn test(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Json<Value>> {
    let probe: Probe = parse(raw)?;
    let doc = fence_doc(&mut ctx.tx, id).await?;
    in_branch(&ctx, doc["branch_id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_default())?;
    let num = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or_default();
    let d = distance_m(probe.latitude, probe.longitude, num(&doc["latitude"]), num(&doc["longitude"]));
    ctx.commit().await?;
    Ok(Json(json!({ "distance_m": (d * 10.0).round() / 10.0, "inside": d <= num(&doc["radius_m"]), "accuracy_m": probe.accuracy_m })))
}

// ---------- F5 activate / deactivate ----------

async fn set_state(mut ctx: Ctx, id: Uuid, headers: HeaderMap, to: &str) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let (_, state) = lock_fence(&mut ctx, id, expected).await?;
    if state != to {
        sqlx::query("UPDATE app.geo_fences SET state = $2::app.fence_state WHERE id = $1").bind(id).bind(to).execute(&mut *ctx.tx).await?;
        new_version(&mut ctx.tx, id, &Place::default(), to == "active").await?;
        ctx.audit(MANAGE, "geo_fence", id, Some(json!({ "state": state })), Some(json!({ "state": to })), None).await?;
    }
    let doc = fence_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(reply(StatusCode::OK, doc))
}

pub async fn activate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap) -> ApiResult<Response> {
    set_state(ctx, id, headers, "active").await
}
pub async fn deactivate(ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap) -> ApiResult<Response> {
    set_state(ctx, id, headers, "inactive").await
}

// ---------- P3 phone binding reset ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

/// Unbinds the employee's phone; the next app sign-in binds the phone used (AC-AT-14.2).
pub async fn binding_reset(mut ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    let em = em_settings(&mut ctx.tx, today()).await?;
    require_edit(&mut ctx, id, "attendance.device_binding.reset", &em).await?;
    let Some(reason) = parse::<Reason>(raw)?.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) else {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    };
    let device: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE app.user_devices d SET unbound_at = now(), unbound_by = $2, unbind_reason = $3
         FROM app.users u WHERE u.id = d.user_id AND u.employee_id = $1 AND d.bound_at IS NOT NULL AND d.unbound_at IS NULL
         RETURNING d.id, d.device_identifier",
    )
    .bind(id)
    .bind(ctx.user_id)
    .bind(&reason)
    .fetch_optional(&mut *ctx.tx)
    .await?;
    let Some((did, ident)) = device else {
        return Err(AppError::conflict("no_binding", "No bound phone", "This employee has no phone bound").into());
    };
    ctx.audit("attendance.device_binding.reset", "employee", id, Some(json!({ "bound_device": { "id": did, "identifier": ident } })), Some(json!({ "bound_device": null })), Some(&reason)).await?;
    ctx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// At an app sign-in: binds this phone when the user has no bound phone (DEC-20). A phone unbound
/// earlier can be bound again. Returns whether this phone is the bound one.
pub(crate) async fn bind_on_sign_in(tx: &mut Tx, user: Uuid, device: Uuid) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(
        "WITH b AS (UPDATE app.user_devices SET bound_at = now(), unbound_at = NULL, unbound_by = NULL, unbind_reason = NULL
                    WHERE id = $2 AND (bound_at IS NULL OR unbound_at IS NOT NULL)
                      AND NOT EXISTS (SELECT 1 FROM app.user_devices o WHERE o.user_id = $1 AND o.bound_at IS NOT NULL AND o.unbound_at IS NULL)
                    RETURNING id)
         SELECT EXISTS (SELECT 1 FROM b) OR EXISTS (SELECT 1 FROM app.user_devices WHERE id = $2 AND bound_at IS NOT NULL AND unbound_at IS NULL)",
    )
    .bind(user)
    .bind(device)
    .fetch_one(&mut **tx)
    .await?)
}
