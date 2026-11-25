//! Shared setup for the end-to-end tests: a fresh test organisation per test, tokens, and request helpers.
#![allow(dead_code)] // each test binary uses a different subset

use api::ctx::issue_access_token;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub const SECRET: &[u8] = b"test-secret-test-secret-test-secret!!";

pub struct World {
    pub app: axum::Router,
    pub state: api::AppState,
    pub pool: PgPool,
    pub org: Uuid,
    pub org_code: String,
    pub admin: String,
    pub hr_blr: String,
    pub chn: Uuid,
    pub blr: Uuid,
    pub hr_dept: Uuid,
    pub resigned: Uuid,
    pub dept: Uuid,
    pub cat: Uuid,
    pub etype: Uuid,
    pub group: Uuid,
    pub cf_choice: Uuid,
    pub cf_num: Uuid,
    pub cf_must: Uuid,
}

pub async fn session(pool: &PgPool, org: Uuid, user: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO app.user_sessions (org_id, user_id, refresh_token_hash, expires_at)
         VALUES ($1, $2, 'test', now() + interval '1 hour') RETURNING id",
    )
    .bind(org)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Seeds as the owner role (outside RLS): org, two branches, lists, an Org Admin and an HR Manager
/// limited to the Bengaluru branch.
pub async fn world() -> World {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_test_writer().try_init().ok();
    let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL")).await.unwrap();
    let tag = Uuid::new_v4().simple().to_string()[..5].to_uppercase();
    let org_code = format!("T{tag}");
    let q = |sql: &'static str| sqlx::query_scalar::<_, Uuid>(sql);

    let vendor = q("INSERT INTO app.vendor_users (email, display_name) VALUES ($1, 'Test') RETURNING id")
        .bind(format!("v-{tag}@example.test")).fetch_one(&pool).await.unwrap();
    let plan = q("INSERT INTO app.plans (code, name, includes_mobile_app) VALUES ($1, 'Test', true) RETURNING id")
        .bind(format!("t_{}", tag.to_lowercase())).fetch_one(&pool).await.unwrap();
    let org = q("SELECT app.provision_organisation($1, $2, 'Test Org', $3, 100, 'block', $4, 'Admin')")
        .bind(vendor).bind(&org_code).bind(plan).bind(format!("admin-{tag}@example.test"))
        .fetch_one(&pool).await.unwrap();
    let branch = |code: &'static str| {
        q("INSERT INTO app.branches (org_id, short_code, name, time_zone) VALUES ($1, $2, $2, 'Asia/Kolkata') RETURNING id")
            .bind(org).bind(code)
    };
    let chn = branch("CHN").fetch_one(&pool).await.unwrap();
    let blr = branch("BLR").fetch_one(&pool).await.unwrap();
    let dept = q("INSERT INTO app.departments (org_id, code, name) VALUES ($1, 'OPS', 'Operations') RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let hr_dept = q("INSERT INTO app.departments (org_id, code, name) VALUES ($1, 'HR', 'Human resources') RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let cat = q("INSERT INTO app.categories (org_id, code, name) VALUES ($1, 'SKL', 'Skilled') RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let etype = q("INSERT INTO app.employee_types (org_id, code, name) VALUES ($1, 'PERM', 'Permanent') RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let group = q("SELECT id FROM app.payroll_groups WHERE org_id = $1 AND is_default").bind(org).fetch_one(&pool).await.unwrap();
    let resigned = q("SELECT id FROM app.exit_reasons WHERE org_id = $1 AND code = 'RESIGNED'").bind(org).fetch_one(&pool).await.unwrap();

    // Custom fields (O19 not built yet): a dropdown with option C deactivated, a number, a mandatory yes/no.
    let cf_choice = q("INSERT INTO app.custom_field_definitions (org_id, label, field_type, options, inactive_options)
                       VALUES ($1, 'Shirt size', 'dropdown', '{A,B,C}', '{C}') RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let cf_num = q("INSERT INTO app.custom_field_definitions (org_id, label, field_type) VALUES ($1, 'Locker', 'number') RETURNING id")
        .bind(org).fetch_one(&pool).await.unwrap();
    let cf_must = q("INSERT INTO app.custom_field_definitions (org_id, label, field_type, is_mandatory) VALUES ($1, 'Has PF', 'yes_no', true) RETURNING id")
        .bind(org).fetch_one(&pool).await.unwrap();

    let admin = q("UPDATE app.users SET state = 'active' WHERE org_id = $1 RETURNING id").bind(org).fetch_one(&pool).await.unwrap();
    let hr = q("INSERT INTO app.users (org_id, kind, email, display_name) VALUES ($1, 'admin', $2, 'HR BLR') RETURNING id")
        .bind(org).bind(format!("hr-{tag}@example.test")).fetch_one(&pool).await.unwrap();
    let ur = q("INSERT INTO app.user_roles (org_id, user_id, role_id, scope_kind)
                SELECT $1, $2, id, 'branches' FROM app.roles WHERE org_id = $1 AND builtin_key = 'hr_manager' RETURNING id")
        .bind(org).bind(hr).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_role_scope_items (org_id, user_role_id, branch_id) VALUES ($1, $2, $3)")
        .bind(org).bind(ur).bind(blr).execute(&pool).await.unwrap();

    let admin_token = issue_access_token(SECRET, admin, org, session(&pool, org, admin).await, "admin");
    let hr_token = issue_access_token(SECRET, hr, org, session(&pool, org, hr).await, "admin");
    let state = api::AppState::new(pool.clone(), std::str::from_utf8(SECRET).unwrap(), "test-totp-key-test-totp-key-test-totp");
    let app = api::router(state.clone());
    World { app, state, pool, org, org_code, admin: admin_token, hr_blr: hr_token, chn, blr, hr_dept, resigned, dept, cat, etype, group, cf_choice, cf_num, cf_must }
}

pub async fn call(w: &World, method: Method, uri: &str, token: Option<&str>, headers: &[(&str, &str)], body: Option<Value>) -> (StatusCode, HeaderMap, Value) {
    call_app(&w.app, method, uri, token, headers, body).await
}

pub async fn call_app(app: &axum::Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&str, &str)], body: Option<Value>) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_default()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

pub fn new_employee(w: &World, name: &str, mobile: &str) -> Value {
    json!({
        "personal": { "full_name": name, "mobile": mobile },
        "joining_date": "2026-04-01",
        "initial_status": "probation",
        "job": { "master_branch_id": w.chn, "department_id": w.dept, "category_id": w.cat,
                 "employee_type_id": w.etype, "payroll_group_id": w.group }
    })
}

pub async fn hire_at_blr(w: &World, name: &str, mobile: &str, key: &str) -> String {
    let mut body = new_employee(w, name, mobile);
    body["job"]["master_branch_id"] = json!(w.blr);
    let (s, _, e) = call(w, Method::POST, "/v1/employees", Some(&w.admin), &[("idempotency-key", key)], Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    e["id"].as_str().unwrap().to_string()
}
