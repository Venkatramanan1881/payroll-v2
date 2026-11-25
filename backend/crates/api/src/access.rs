//! Roles and users R1–R8 (API_CONTRACTS §7, BR-EM-29, PERMISSION_MATRIX fixed rules 3–4).
//! Permissions are read from the database on every request, so a change applies from each
//! holder's next request (AC-AC-01.1).
//! Rights rule: a caller may only grant, change or remove a role that fits within their own
//! permissions and their own `access.role.assign` scope. A user they manage must have every role,
//! before and after the change, within those rights.

use crate::ctx::{if_match, stale, Ctx};
use crate::employees::{check_item, query_pairs, Tx};
use crate::error::{parse, ApiResult, Body};
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use shared::{AppError, FieldError};
use std::collections::BTreeSet;
use uuid::Uuid;

fn exceeds() -> AppError {
    AppError::new(403, "exceeds_own_rights", "Beyond your rights", "You can't grant rights or scope wider than your own")
}

fn replay(s: u16, b: Value) -> Response {
    crate::employees::employee_response(StatusCode::from_u16(s).unwrap_or(StatusCode::OK), b)
}

// ---------- roles ----------

const ROLE: &str = "SELECT jsonb_build_object('id', r.id, 'name', r.name, 'builtin_key', r.builtin_key, 'state', r.state,
      'permissions', coalesce((SELECT jsonb_agg(rp.permission_code ORDER BY rp.permission_code) FROM app.role_permissions rp WHERE rp.role_id = r.id), '[]'),
      'user_count', (SELECT count(*) FROM app.user_roles ur JOIN app.users u ON u.id = ur.user_id
                     WHERE ur.role_id = r.id AND u.state <> 'revoked'),
      'version', r.row_version)
    FROM app.roles r WHERE r.org_id = app.current_org_id()";

async fn role_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("{ROLE} AND r.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

fn codes(doc: &Value) -> BTreeSet<String> {
    serde_json::from_value(doc["permissions"].clone()).unwrap_or_default()
}

/// R1
pub async fn permissions(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    ctx.scope("access.role.edit")?;
    let items: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('code', code, 'description', description, 'is_fixed', is_fixed) ORDER BY code), '[]')
         FROM app.permissions",
    )
    .fetch_one(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

fn can_read_roles(ctx: &Ctx) -> ApiResult<()> {
    if ctx.has("access.role.edit") || ctx.has("access.role.assign") { Ok(()) } else { Err(AppError::forbidden().into()) }
}

/// R2 list
pub async fn roles(mut ctx: Ctx) -> ApiResult<Json<Value>> {
    can_read_roles(&ctx)?;
    let items: Vec<Value> = sqlx::query_scalar(&format!("{ROLE} ORDER BY r.builtin_key IS NULL, lower(r.name)"))
        .fetch_all(&mut *ctx.tx)
        .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

/// R2 one
pub async fn role(mut ctx: Ctx, Path(id): Path<Uuid>) -> ApiResult<Response> {
    can_read_roles(&ctx)?;
    let doc = role_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::OK, doc))
}

/// Codes must exist, none fixed, all held by the caller.
async fn check_codes(ctx: &mut Ctx, wanted: &BTreeSet<String>) -> ApiResult<()> {
    let known: Vec<(String, bool)> = sqlx::query_as("SELECT code, is_fixed FROM app.permissions").fetch_all(&mut *ctx.tx).await?;
    if let Some(c) = wanted.iter().find(|c| !known.iter().any(|(k, _)| k == *c)) {
        return Err(AppError::validation(vec![FieldError::new("permissions", "invalid_value", format!("Unknown permission {c}"))]).into());
    }
    if let Some(c) = wanted.iter().find(|c| known.iter().any(|(k, fixed)| k == *c && *fixed)) {
        return Err(AppError::unprocessable("fixed_permission", "Fixed permission", format!("{c} can't be granted or removed")).into());
    }
    if wanted.iter().any(|c| !ctx.has(c)) {
        return Err(exceeds().into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewRole {
    name: Option<String>,
    copy_from_role_id: Option<Uuid>,
    permissions: Option<BTreeSet<String>>,
}

/// R3: a custom role, optionally copied from another of the organisation's roles (AC-AC-01.3).
pub async fn role_create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("access.role.edit")?;
    // shortcut: AC-06 (custom roles on/off) has no settings group yet, so custom roles are always on; add custom_roles_disabled with it.
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/access/roles", &raw).await? {
        return Ok(replay(s, b));
    }
    let req: NewRole = parse(raw)?;
    let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string)
        .ok_or_else(|| AppError::validation(vec![FieldError::new("name", "required", "Give the role a name")]))?;
    let wanted = match (req.permissions, req.copy_from_role_id) {
        (Some(p), _) => p,
        (None, Some(src)) => {
            let fixed: Vec<String> = sqlx::query_scalar("SELECT code FROM app.permissions WHERE is_fixed").fetch_all(&mut *ctx.tx).await?;
            let mut p = codes(&role_doc(&mut ctx.tx, src).await.map_err(|_| {
                AppError::validation(vec![FieldError::new("copy_from_role_id", "invalid_value", "Not found")])
            })?);
            p.retain(|c| !fixed.contains(c)); // a copy never carries a fixed permission
            p
        }
        (None, None) => BTreeSet::new(),
    };
    check_codes(&mut ctx, &wanted).await?;
    let id: Uuid = sqlx::query_scalar("INSERT INTO app.roles (org_id, name) VALUES ($1, $2) RETURNING id")
        .bind(ctx.org_id)
        .bind(&name)
        .fetch_one(&mut *ctx.tx)
        .await?;
    sqlx::query("INSERT INTO app.role_permissions (org_id, role_id, permission_code) SELECT $1, $2, unnest($3::text[])")
        .bind(ctx.org_id)
        .bind(id)
        .bind(wanted.iter().cloned().collect::<Vec<_>>())
        .execute(&mut *ctx.tx)
        .await?;
    let doc = role_doc(&mut ctx.tx, id).await?;
    ctx.audit("access.role.edit", "role", id, None, Some(json!({ "name": name, "permissions": wanted })), None).await?;
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::CREATED, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RolePermissions {
    permissions: BTreeSet<String>,
}

/// R4: the role's full permission set. Only codes the caller holds may be added or removed.
pub async fn role_permissions(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let mine = ctx.scope("access.role.edit")?;
    let expected = if_match(&headers)?;
    let req: RolePermissions = parse(raw)?;
    let version: i32 = sqlx::query_scalar("SELECT row_version FROM app.roles WHERE id = $1 AND org_id = $2 FOR UPDATE")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&mut *ctx.tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if version != expected {
        return Err(stale(version));
    }
    let old = role_doc(&mut ctx.tx, id).await?;
    match old["builtin_key"].as_str() {
        // Org Admin always holds everything, so the organisation can't lock itself out.
        Some("org_admin") => return Err(AppError::unprocessable("fixed_permission", "Fixed role", "The Organisation Admin role always has every permission").into()),
        // shortcut: a built-in role reaches users across the organisation, so only organisation-wide editors change it; per-holder scope checks if branch editors need it.
        Some(_) if !mine.org => return Err(exceeds().into()),
        _ => {}
    }
    let before = codes(&old);
    let added: BTreeSet<String> = req.permissions.difference(&before).cloned().collect();
    let removed: BTreeSet<String> = before.difference(&req.permissions).cloned().collect();
    check_codes(&mut ctx, &added).await?;
    check_codes(&mut ctx, &removed).await?;
    sqlx::query("DELETE FROM app.role_permissions WHERE role_id = $1 AND permission_code = ANY($2)")
        .bind(id)
        .bind(removed.iter().cloned().collect::<Vec<_>>())
        .execute(&mut *ctx.tx)
        .await?;
    sqlx::query("INSERT INTO app.role_permissions (org_id, role_id, permission_code) SELECT $1, $2, unnest($3::text[])")
        .bind(ctx.org_id)
        .bind(id)
        .bind(added.iter().cloned().collect::<Vec<_>>())
        .execute(&mut *ctx.tx)
        .await?;
    sqlx::query("UPDATE app.roles SET row_version = row_version + 1 WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
    ctx.audit("access.role.edit", "role", id, Some(json!({ "removed": removed })), Some(json!({ "added": added })), None).await?;
    let doc = role_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::OK, doc))
}

// ---------- users ----------

const USER: &str = "SELECT jsonb_build_object('id', u.id, 'kind', u.kind, 'email', u.email, 'display_name', u.display_name,
      'state', u.state, 'employee_id', u.employee_id, 'version', u.row_version,
      'roles', coalesce((SELECT jsonb_agg(jsonb_build_object('role_id', r.id, 'name', r.name, 'builtin_key', r.builtin_key,
            'scope_kind', ur.scope_kind,
            'branch_ids', coalesce((SELECT jsonb_agg(si.branch_id ORDER BY si.branch_id) FROM app.user_role_scope_items si
                                    WHERE si.user_role_id = ur.id AND si.branch_id IS NOT NULL), '[]'),
            'department_ids', coalesce((SELECT jsonb_agg(si.department_id ORDER BY si.department_id) FROM app.user_role_scope_items si
                                    WHERE si.user_role_id = ur.id AND si.department_id IS NOT NULL), '[]')) ORDER BY r.name)
          FROM app.user_roles ur JOIN app.roles r ON r.id = ur.role_id WHERE ur.user_id = u.id), '[]'))
    FROM app.users u";

async fn user_doc(tx: &mut Tx, id: Uuid) -> ApiResult<Value> {
    sqlx::query_scalar::<_, Value>(&format!("{USER} WHERE u.id = $1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::not_found().into())
}

/// One role held with its scope: request body item and stored shape alike.
#[derive(Deserialize, Clone)]
struct Grant {
    role_id: Uuid,
    #[serde(default = "organisation")]
    scope_kind: String,
    #[serde(default)]
    branch_ids: Vec<Uuid>,
    #[serde(default)]
    department_ids: Vec<Uuid>,
    #[serde(default, skip)]
    builtin_key: Option<String>,
}

fn organisation() -> String {
    "organisation".into()
}

/// Request items allow only the four documented keys.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantIn {
    role_id: Uuid,
    #[serde(default = "organisation")]
    scope_kind: String,
    #[serde(default)]
    branch_ids: Vec<Uuid>,
    #[serde(default)]
    department_ids: Vec<Uuid>,
}

/// Roles as stored, without the Employee role (that one follows portal access, E13/E14).
fn held(doc: &Value) -> Vec<Grant> {
    doc["roles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["builtin_key"] != "employee")
        .filter_map(|r| {
            let mut g: Grant = serde_json::from_value(r.clone()).ok()?;
            g.builtin_key = r["builtin_key"].as_str().map(str::to_string);
            Some(g)
        })
        .collect()
}

/// Does this role and scope fit within the caller's own rights (fixed rule 4)?
async fn within(ctx: &mut Ctx, g: &Grant) -> ApiResult<bool> {
    let perms: Vec<String> = sqlx::query_scalar("SELECT permission_code FROM app.role_permissions WHERE role_id = $1")
        .bind(g.role_id)
        .fetch_all(&mut *ctx.tx)
        .await?;
    if perms.iter().any(|p| !ctx.has(p)) {
        return Ok(false);
    }
    let mine = ctx.scope("access.role.assign")?;
    if mine.org || g.builtin_key.as_deref() == Some("manager") {
        // shortcut: a Manager's reach is their reporting chain, not a scope; not limited for branch assigners yet.
        return Ok(true);
    }
    Ok(match g.scope_kind.as_str() {
        "branches" => !g.branch_ids.is_empty() && g.branch_ids.iter().all(|b| mine.branches.contains(b)),
        "departments" => !g.department_ids.is_empty() && g.department_ids.iter().all(|d| mine.departments.contains(d)),
        _ => false,
    })
}

/// Shape checks on requested grants; fills in each role's builtin key. `employee_user`: Manager needs an employee.
async fn check_grants(ctx: &mut Ctx, items: Vec<GrantIn>, employee_user: bool) -> ApiResult<Vec<Grant>> {
    let mut errs = Vec::new();
    let mut out: Vec<Grant> = Vec::new();
    for (i, g) in items.into_iter().enumerate() {
        let f = |k: &str| format!("roles[{i}].{k}");
        let builtin: Option<Option<String>> = sqlx::query_scalar("SELECT builtin_key FROM app.roles WHERE id = $1 AND org_id = $2 AND state = 'active'")
            .bind(g.role_id)
            .bind(ctx.org_id)
            .fetch_optional(&mut *ctx.tx)
            .await?;
        let Some(builtin) = builtin else {
            errs.push(FieldError::new(f("role_id"), "invalid_value", "Not found or inactive"));
            continue;
        };
        match builtin.as_deref() {
            Some("employee") => errs.push(FieldError::new(f("role_id"), "invalid_value", "The Employee role comes with portal access")),
            Some("manager") if !employee_user => errs.push(FieldError::new(f("role_id"), "invalid_value", "The Manager role is for employees")),
            _ => {}
        }
        if out.iter().any(|o| o.role_id == g.role_id) {
            errs.push(FieldError::new(f("role_id"), "invalid_value", "Role listed twice"));
        }
        let ok = match (builtin.as_deref(), g.scope_kind.as_str()) {
            (Some("manager"), _) => g.branch_ids.is_empty() && g.department_ids.is_empty(),
            (_, "organisation") => g.branch_ids.is_empty() && g.department_ids.is_empty(),
            (_, "branches") => !g.branch_ids.is_empty() && g.department_ids.is_empty(),
            (_, "departments") => g.branch_ids.is_empty() && !g.department_ids.is_empty(),
            _ => false,
        };
        if !ok {
            errs.push(FieldError::new(f("scope_kind"), "invalid_value", "organisation, or branches / departments with their ids"));
        }
        for b in &g.branch_ids {
            check_item(&mut ctx.tx, "branches", Some(*b), &f("branch_ids"), &mut errs).await?;
        }
        for d in &g.department_ids {
            check_item(&mut ctx.tx, "departments", Some(*d), &f("department_ids"), &mut errs).await?;
        }
        out.push(Grant {
            role_id: g.role_id,
            scope_kind: if builtin.as_deref() == Some("manager") { organisation() } else { g.scope_kind },
            branch_ids: g.branch_ids,
            department_ids: g.department_ids,
            builtin_key: builtin,
        });
    }
    if errs.is_empty() { Ok(out) } else { Err(AppError::validation(errs).into()) }
}

async fn insert_grants(ctx: &mut Ctx, user: Uuid, grants: &[Grant]) -> ApiResult<()> {
    for g in grants {
        let ur: Uuid = sqlx::query_scalar(
            "INSERT INTO app.user_roles (org_id, user_id, role_id, scope_kind) VALUES ($1, $2, $3, $4::app.scope_kind) RETURNING id",
        )
        .bind(ctx.org_id)
        .bind(user)
        .bind(g.role_id)
        .bind(&g.scope_kind)
        .fetch_one(&mut *ctx.tx)
        .await?;
        sqlx::query(
            "INSERT INTO app.user_role_scope_items (org_id, user_role_id, branch_id, department_id)
             SELECT $1, $2, b, NULL FROM unnest($3::uuid[]) b UNION ALL SELECT $1, $2, NULL, d FROM unnest($4::uuid[]) d",
        )
        .bind(ctx.org_id)
        .bind(ur)
        .bind(&g.branch_ids)
        .bind(&g.department_ids)
        .execute(&mut *ctx.tx)
        .await?;
    }
    Ok(())
}

/// Serialises Org Admin changes, so two at once can't both remove "the other" admin.
async fn lock_org_admins(tx: &mut Tx) -> ApiResult<()> {
    sqlx::query("SELECT id FROM app.roles WHERE org_id = app.current_org_id() AND builtin_key = 'org_admin' FOR UPDATE")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Fixed rule 3: at least one active user keeps the Org Admin role. Checked after the change, before commit.
async fn keep_an_org_admin(tx: &mut Tx) -> ApiResult<()> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM app.users u JOIN app.user_roles ur ON ur.user_id = u.id JOIN app.roles r ON r.id = ur.role_id
         WHERE r.builtin_key = 'org_admin' AND u.state = 'active'",
    )
    .fetch_one(&mut **tx)
    .await?;
    if n == 0 {
        return Err(AppError::conflict("last_org_admin", "Last Org Admin", "The organisation must keep at least one Org Admin").into());
    }
    Ok(())
}

/// The target and every role they hold must be within the caller's rights. 404 if unknown.
async fn manageable(ctx: &mut Ctx, id: Uuid) -> ApiResult<Value> {
    ctx.scope("access.role.assign")?;
    let doc = user_doc(&mut ctx.tx, id).await?;
    for g in held(&doc) {
        if !within(ctx, &g).await? {
            return Err(exceeds().into());
        }
    }
    Ok(doc)
}

/// R5
pub async fn users(mut ctx: Ctx, RawQuery(raw): RawQuery) -> ApiResult<Json<Value>> {
    ctx.scope("access.role.assign")?;
    let mut kind: Option<String> = None;
    for (k, v) in query_pairs(raw)? {
        match k.as_str() {
            "kind" if v == "admin" || v == "employee" => kind = Some(v),
            _ => return Err(crate::employees::bad_param(&k, "Only kind=admin or kind=employee").into()),
        }
    }
    // shortcut: no paging; fine for admin lists, add a cursor when employee user lists grow large.
    let items: Vec<Value> = sqlx::query_scalar(&format!(
        "{USER} WHERE u.org_id = app.current_org_id() AND ($1::text IS NULL OR u.kind::text = $1) ORDER BY u.display_name"
    ))
    .bind(kind)
    .fetch_all(&mut *ctx.tx)
    .await?;
    ctx.commit().await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewUser {
    email: Option<String>,
    display_name: Option<String>,
    #[serde(default)]
    roles: Vec<GrantIn>,
}

/// R6: an invited admin user. They set a password and authenticator through A7.
pub async fn user_create(mut ctx: Ctx, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    ctx.scope("access.role.assign")?;
    if let Some((s, b)) = ctx.idempotency_begin(&headers, "POST /v1/access/users", &raw).await? {
        return Ok(replay(s, b));
    }
    let req: NewUser = parse(raw)?;
    let mut errs = Vec::new();
    let email = req.email.as_deref().map(str::trim).unwrap_or_default().to_string();
    let at = email.find('@');
    if !at.is_some_and(|i| i > 0 && i < email.len() - 1 && !email.contains(char::is_whitespace)) {
        errs.push(FieldError::new("email", "invalid_value", "Give a valid email address"));
    }
    let name = req.display_name.as_deref().map(str::trim).unwrap_or_default().to_string();
    if name.is_empty() {
        errs.push(FieldError::new("display_name", "required", "Give the user's name"));
    }
    if req.roles.is_empty() {
        errs.push(FieldError::new("roles", "required", "Give the user at least one role"));
    }
    if !errs.is_empty() {
        return Err(AppError::validation(errs).into());
    }
    let grants = check_grants(&mut ctx, req.roles, false).await?;
    for g in &grants {
        if !within(&mut ctx, g).await? {
            return Err(exceeds().into());
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO app.users (org_id, kind, email, display_name, state) VALUES ($1, 'admin', $2, $3, 'invited') RETURNING id",
    )
    .bind(ctx.org_id)
    .bind(&email)
    .bind(&name)
    .fetch_one(&mut *ctx.tx)
    .await?;
    sqlx::query("INSERT INTO app.user_credentials (user_id, org_id, kind) VALUES ($1, $2, 'password')")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *ctx.tx)
        .await?;
    insert_grants(&mut ctx, id, &grants).await?;
    let mut doc = user_doc(&mut ctx.tx, id).await?;
    ctx.audit("access.role.assign", "user", id, None, Some(doc.clone()), None).await?;
    // The invitation link is made by the sender (auth::issue_invite_token), so no token is ever stored.
    ctx.event("AdminInvited", "user", id, json!({ "user_id": id })).await?;
    // shortcut: emailed is false until the email service exists; the event above is what it will send from.
    doc["invitation"] = json!({ "emailed": false });
    ctx.idempotency_finish(&headers, 201, &doc).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::CREATED, doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserRoles {
    roles: Vec<GrantIn>,
}

/// R7: the user's full set of roles (the Employee role is kept as it is).
pub async fn user_roles(mut ctx: Ctx, Path(id): Path<Uuid>, headers: HeaderMap, Body(raw): Body) -> ApiResult<Response> {
    let expected = if_match(&headers)?;
    let req: UserRoles = parse(raw)?;
    lock_org_admins(&mut ctx.tx).await?;
    let old = manageable(&mut ctx, id).await?;
    let version = old["version"].as_i64().unwrap_or_default() as i32;
    if version != expected {
        return Err(stale(version));
    }
    let grants = check_grants(&mut ctx, req.roles, old["kind"] == "employee").await?;
    for g in &grants {
        if !within(&mut ctx, g).await? {
            return Err(exceeds().into());
        }
    }
    sqlx::query(
        "DELETE FROM app.user_roles ur USING app.roles r
         WHERE ur.user_id = $1 AND r.id = ur.role_id AND r.builtin_key IS DISTINCT FROM 'employee'",
    )
    .bind(id)
    .execute(&mut *ctx.tx)
    .await?;
    insert_grants(&mut ctx, id, &grants).await?;
    sqlx::query("UPDATE app.users SET row_version = row_version + 1 WHERE id = $1").bind(id).execute(&mut *ctx.tx).await?;
    keep_an_org_admin(&mut ctx.tx).await?;
    let new = user_doc(&mut ctx.tx, id).await?;
    ctx.audit("access.role.assign", "user", id, Some(json!({ "roles": old["roles"] })), Some(json!({ "roles": new["roles"] })), None).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::OK, new))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: Option<String>,
}

async fn set_user_state(mut ctx: Ctx, id: Uuid, raw: Value, suspend: bool) -> ApiResult<Response> {
    let reason = parse::<Reason>(raw)?.reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    if suspend && reason.is_none() {
        return Err(AppError::validation(vec![FieldError::new("reason", "required", "Give a reason")]).into());
    }
    lock_org_admins(&mut ctx.tx).await?;
    let old = manageable(&mut ctx, id).await?;
    let state = old["state"].as_str().unwrap_or_default().to_string();
    let wrong = || AppError::unprocessable("transition_not_allowed", "Not possible now", format!("The user is {state}"));
    let to = match (suspend, state.as_str()) {
        (true, "suspended") | (false, "active" | "invited") => {
            ctx.commit().await?;
            return Ok(crate::employees::employee_response(StatusCode::OK, old)); // already there
        }
        (true, "active" | "invited") => "suspended",
        // Back to where they were: invited until a password is set.
        (false, "suspended") => {
            let has_secret: bool = sqlx::query_scalar("SELECT coalesce((SELECT secret_hash IS NOT NULL FROM app.user_credentials WHERE user_id = $1), false)")
                .bind(id)
                .fetch_one(&mut *ctx.tx)
                .await?;
            if has_secret { "active" } else { "invited" }
        }
        _ => return Err(wrong().into()),
    };
    sqlx::query("UPDATE app.users SET state = $2::app.user_state, row_version = row_version + 1 WHERE id = $1")
        .bind(id)
        .bind(to)
        .execute(&mut *ctx.tx)
        .await?;
    if suspend {
        sqlx::query("UPDATE app.user_sessions SET revoked_at = now(), revoke_reason = 'user suspended' WHERE user_id = $1 AND revoked_at IS NULL")
            .bind(id)
            .execute(&mut *ctx.tx)
            .await?;
        keep_an_org_admin(&mut ctx.tx).await?;
    }
    ctx.audit("access.role.assign", "user", id, Some(json!({ "state": state })), Some(json!({ "state": to })), reason.as_deref()).await?;
    let doc = user_doc(&mut ctx.tx, id).await?;
    ctx.commit().await?;
    Ok(crate::employees::employee_response(StatusCode::OK, doc))
}

/// R8 suspend: sessions end at once.
pub async fn user_suspend(ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    set_user_state(ctx, id, raw, true).await
}

/// R8 reactivate.
pub async fn user_reactivate(ctx: Ctx, Path(id): Path<Uuid>, Body(raw): Body) -> ApiResult<Response> {
    set_user_state(ctx, id, raw, false).await
}
