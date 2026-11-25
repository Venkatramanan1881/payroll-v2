//! End-to-end checks of the employee and sign-in routes against a real database (DATABASE_URL from backend/.env).
//! Ignored by default because it writes to the database: each run creates a fresh test organisation
//! with a random short code (the audit log is insert-only, so test orgs are not deleted).
//!
//!   cargo test -p api --test employees -- --ignored

use api::ctx::issue_access_token;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tower::ServiceExt;
use uuid::Uuid;

mod common;
use common::*;


#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn employee_routes_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());

    // No token.
    let (s, _, b) = call(&w, Method::GET, "/v1/employees", None, &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthenticated")));

    // E5: every missing field is named (AC-EM-01.2).
    let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-missing-1")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    let fields: Vec<&str> = b["errors"].as_array().unwrap().iter().map(|e| e["field"].as_str().unwrap()).collect();
    for f in ["personal.full_name", "personal.mobile", "joining_date", "job.master_branch_id", "job.payroll_group_id"] {
        assert!(fields.contains(&f), "{f} missing from {fields:?}");
    }

    // E5: Idempotency-Key required; unknown fields refused.
    let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[], Some(new_employee(&w, "Meena", "+919800000001"))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("Idempotency-Key")));
    let mut typo = new_employee(&w, "Meena", "+919800000001");
    typo["personal"]["full_nmae"] = json!("x");
    let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-typo-0001")], Some(typo)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_field")), "{b}");

    // E5: created with the default code pattern and probation end (AC-EM-02.1, EM-04).
    let body = new_employee(&w, "Meena", "+919800000001");
    let (s, h, meena) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-meena-001")], Some(body.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{meena}");
    assert_eq!(meena["employee_code"], format!("{}CHN0001", w.org_code));
    assert_eq!(h["etag"], "\"1\"");
    assert_eq!(meena["status"]["value"], "probation");
    assert_eq!(meena["employment"]["probation_end_date"], "2026-09-30");
    assert_eq!(meena["job"]["master_branch"]["short_code"], "CHN");
    assert_eq!(meena["can"]["edit"], true);
    let id = meena["id"].as_str().unwrap().to_string();

    // Same key + same body replays; same key + other body is refused (§1.6).
    let (s, _, again) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-meena-001")], Some(body)).await;
    assert_eq!((s, again["id"].as_str()), (StatusCode::CREATED, Some(id.as_str())));
    let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-meena-001")], Some(new_employee(&w, "Other", "+919800000009"))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("idempotency_key_reused")));

    // Mobile in use names the holder (BR-EM-09); next code is 0002.
    let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-dup-mob-1")], Some(new_employee(&w, "Ravi", "+919800000001"))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("mobile_in_use")));
    assert_eq!(b["meta"]["employee_code"], meena["employee_code"]);
    let (s, _, ravi) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-ravi-0001")], Some(new_employee(&w, "Ravi", "+919800000002"))).await;
    assert_eq!(s, StatusCode::CREATED, "{ravi}");
    // The duplicate-mobile attempt failed before a number was taken, so no number was used up.
    assert_eq!(ravi["employee_code"], format!("{}CHN0002", w.org_code));

    // E1 / E2 with scope: the Bengaluru-only HR Manager sees no Chennai employee, and gets 404 (AC-EM-20.5).
    let (s, _, list) = call(&w, Method::GET, "/v1/employees?q=meena", admin, &[], None).await;
    assert_eq!((s, list["items"].as_array().unwrap().len()), (StatusCode::OK, 1), "{list}");
    let (_, _, list) = call(&w, Method::GET, "/v1/employees?limit=1", admin, &[], None).await;
    assert!(list["next_cursor"].is_string());
    let (_, _, list) = call(&w, Method::GET, "/v1/employees", Some(&w.hr_blr), &[], None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
    let (s, _, _) = call(&w, Method::GET, &format!("/v1/employees/{id}"), Some(&w.hr_blr), &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // E8: If-Match required, stale refused, real change bumps the version.
    let uri = format!("/v1/employees/{id}/personal");
    let patch = json!({ "email": "meena@example.com", "full_name": "Meena R" });
    let (s, _, _) = call(&w, Method::PATCH, &uri, admin, &[], Some(patch.clone())).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, b) = call(&w, Method::PATCH, &uri, admin, &[("if-match", "\"7\"")], Some(patch.clone())).await;
    assert_eq!((s, b["meta"]["current_version"].as_i64()), (StatusCode::PRECONDITION_FAILED, Some(1)));
    let (s, h, b) = call(&w, Method::PATCH, &uri, admin, &[("if-match", "\"1\"")], Some(patch)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!((h["etag"].to_str().unwrap(), b["personal"]["full_name"].as_str()), ("\"2\"", Some("Meena R")));
    let (s, _, b) = call(&w, Method::PATCH, &uri, admin, &[("if-match", "\"2\"")], Some(json!({ "mobile": "+919800000002" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("mobile_in_use")));
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn audit_rows_are_written() {
    let w = world().await;
    let (s, _, e) = call(&w, Method::POST, "/v1/employees", Some(&w.admin), &[("idempotency-key", "key-audit-001")], Some(new_employee(&w, "Asha", "+919800000003"))).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    dotenvy::dotenv().ok();
    let pool = PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let id: Uuid = e["id"].as_str().unwrap().parse().unwrap();
    let row = sqlx::query(
        "SELECT (SELECT count(*) FROM app.audit_log WHERE target_id = $1 AND action = 'employee.create') AS audits,
                (SELECT count(*) FROM app.outbox_events WHERE aggregate_id = $1 AND event_type = 'EmployeeCreated') AS events",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((row.get::<i64, _>("audits"), row.get::<i64, _>("events")), (1, 1));
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn job_changes_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let (_, _, meena) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-jc-meena1")], Some(new_employee(&w, "Meena", "+919800000011"))).await;
    let id = meena["id"].as_str().unwrap().to_string();
    let uri = format!("/v1/employees/{id}/job-changes");

    // If-Match required; date before joining refused.
    let transfer = json!({ "effective_from": "2026-06-01", "changes": { "master_branch_id": w.blr }, "reason": "Transfer" });
    let (s, _, _) = call(&w, Method::POST, &uri, admin, &[], Some(transfer.clone())).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let early = json!({ "effective_from": "2026-01-01", "changes": { "master_branch_id": w.blr }, "reason": "x" });
    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"1\"")], Some(early)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("date_before_joining")));

    // Past-dated transfer splits the version (BR-EM-05) and prompts a review (AC-EM-10.4).
    let (s, h, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"1\"")], Some(transfer.clone())).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(h["etag"], "\"2\"");
    assert_eq!(b["scheduled"], false);
    assert_eq!(b["prompts"], json!(["review_payroll_group", "review_shift"]));
    let v = b["versions"].as_array().unwrap();
    assert_eq!((v[0]["valid_from"].as_str(), v[0]["valid_to"].as_str()), (Some("2026-04-01"), Some("2026-05-31")));
    assert_eq!((v[1]["valid_from"].as_str(), v[1]["valid_to"].is_null()), (Some("2026-06-01"), true));
    let (_, _, on_may) = call(&w, Method::GET, &format!("/v1/employees/{id}?as_of=2026-05-15"), admin, &[], None).await;
    assert_eq!(on_may["job"]["master_branch"]["short_code"], "CHN");
    let (_, _, on_jul) = call(&w, Method::GET, &format!("/v1/employees/{id}?as_of=2026-07-01"), admin, &[], None).await;
    assert_eq!(on_jul["job"]["master_branch"]["short_code"], "BLR");

    // Repeating it changes nothing; a stale version is refused.
    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"2\"")], Some(transfer)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("no_change")));
    let future = json!({ "effective_from": "2027-01-01", "changes": { "department_id": w.hr_dept }, "reason": "Moves to HR" });
    let (s, _, _) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"1\"")], Some(future.clone())).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);

    // Future change is scheduled; cancel it, twice (idempotent); an effective version can't be cancelled.
    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"2\"")], Some(future)).await;
    assert_eq!((s, b["scheduled"].as_bool()), (StatusCode::OK, Some(true)), "{b}");
    let scheduled = b["versions"].as_array().unwrap().iter().find(|v| v["state"] == "scheduled").unwrap()["version_id"].as_str().unwrap().to_string();
    let effective = b["versions"][0]["version_id"].as_str().unwrap().to_string();
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{id}"), admin, &[], None).await;
    assert_eq!(e["scheduled_changes_count"], 1);
    let cancel = |v: &str| format!("/v1/employees/{id}/job-changes/{v}/cancel");
    let (s, _, b) = call(&w, Method::POST, &cancel(&scheduled), admin, &[], Some(json!({ "reason": "Plan changed" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("cancelled")), "{b}");
    let (s, _, _) = call(&w, Method::POST, &cancel(&scheduled), admin, &[], Some(json!({ "reason": "again" }))).await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{id}?as_of=2027-02-01"), admin, &[], None).await;
    assert_eq!((e["scheduled_changes_count"].as_i64(), e["job"]["department"]["code"].as_str()), (Some(0), Some("OPS")));
    let (s, _, b) = call(&w, Method::POST, &cancel(&effective), admin, &[], Some(json!({ "reason": "x" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_scheduled")));

    // Reporting loop refused (BR-EM-11).
    let (_, _, ravi) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-jc-ravi01")], Some(new_employee(&w, "Ravi", "+919800000012"))).await;
    let ravi_id = ravi["id"].as_str().unwrap();
    let to_meena = json!({ "effective_from": "2026-11-01", "changes": { "reporting_manager_id": id }, "reason": "Team" });
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{ravi_id}/job-changes"), admin, &[("if-match", "\"1\"")], Some(to_meena)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{id}"), admin, &[], None).await;
    let ver = format!("\"{}\"", e["version"]);
    let to_ravi = json!({ "effective_from": "2026-11-01", "changes": { "reporting_manager_id": ravi_id }, "reason": "Loop" });
    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", ver.as_str())], Some(to_ravi)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("manager_loop")), "{b}");
}


#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn status_requests_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let meena = hire_at_blr(&w, "Meena", "+919800000021", "key-st-meena1").await;
    let raise = |e: &str| format!("/v1/employees/{e}/status-requests");
    let act = |r: &str, a: &str| format!("/v1/status-requests/{r}/{a}");

    // S1 by the HR Manager: the Org Admin approves (BR-EM-27). A second open request is refused.
    let confirm = json!({ "to_status": "active", "effective_date": "2026-10-01", "reason": "Confirmed" });
    let (s, h, req) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-conf01")], Some(confirm.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{req}");
    assert_eq!((req["state"].as_str(), req["approver_role"].as_str(), req["from_status"].as_str()), (Some("pending"), Some("org_admin"), Some("probation")));
    assert_eq!(h["etag"], "\"1\"");
    let r1 = req["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-conf02")], Some(confirm)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("request_already_pending")));

    // The raiser cannot decide; S2 awaiting_me shows it to the Org Admin only.
    let (s, _, b) = call(&w, Method::POST, &act(&r1, "approve"), hr, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("cannot_decide_own_request")));
    let (_, _, l) = call(&w, Method::GET, "/v1/status-requests?awaiting_me=true", admin, &[], None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1, "{l}");
    let (_, _, l) = call(&w, Method::GET, "/v1/status-requests?awaiting_me=true", hr, &[], None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 0);

    // S4: past date takes effect at once; approving again returns the same state.
    let (s, _, _) = call(&w, Method::POST, &act(&r1, "approve"), admin, &[], Some(json!({}))).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, b) = call(&w, Method::POST, &act(&r1, "approve"), admin, &[("if-match", "\"1\"")], Some(json!({ "note": "ok" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("effective")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act(&r1, "approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("effective")));
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{meena}"), admin, &[], None).await;
    assert_eq!((e["status"]["value"].as_str(), e["status"]["since"].as_str()), (Some("active"), Some("2026-10-01")));

    // SM-1: nothing moves back to probation.
    let back = json!({ "to_status": "probation", "effective_date": "2026-11-01", "reason": "x" });
    let (s, _, b) = call(&w, Method::POST, &raise(&meena), admin, &[("idempotency-key", "key-st-back01")], Some(back)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("transition_not_allowed")));

    // Raised by the Org Admin: an HR Manager approves; a future date is scheduled; S7 cancel, twice.
    let notice = json!({ "to_status": "notice", "effective_date": "2027-01-01", "reason": "Resigned" });
    let (_, _, req) = call(&w, Method::POST, &raise(&meena), admin, &[("idempotency-key", "key-st-note01")], Some(notice)).await;
    let r2 = req["id"].as_str().unwrap().to_string();
    assert_eq!(req["approver_role"], "hr_manager");
    let (s, _, b) = call(&w, Method::POST, &act(&r2, "approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("cannot_decide_own_request")));
    let (s, _, b) = call(&w, Method::POST, &act(&r2, "approve"), hr, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("scheduled")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act(&r2, "cancel"), hr, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("reason")));
    let (s, _, b) = call(&w, Method::POST, &act(&r2, "cancel"), hr, &[("if-match", "\"2\"")], Some(json!({ "reason": "Stays" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("cancelled")), "{b}");
    let (s, _, _) = call(&w, Method::POST, &act(&r2, "cancel"), hr, &[], Some(json!({ "reason": "again" }))).await;
    assert_eq!(s, StatusCode::OK);

    // S5 reject needs a reason; S6 only the raiser withdraws.
    let leave = json!({ "to_status": "long_leave", "effective_date": "2026-12-01", "reason": "Medical" });
    let (_, _, req) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-long01")], Some(leave.clone())).await;
    let r3 = req["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, &act(&r3, "reject"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, b) = call(&w, Method::POST, &act(&r3, "reject"), admin, &[("if-match", "\"1\"")], Some(json!({ "reason": "Not eligible" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("rejected")));
    let (_, _, req) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-long02")], Some(leave)).await;
    let r4 = req["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, &act(&r4, "withdraw"), admin, &[("if-match", "\"1\"")], None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, b) = call(&w, Method::POST, &act(&r4, "withdraw"), hr, &[("if-match", "\"1\"")], None).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("withdrawn")));

    // Exit: details required; effective the day after the last working day; frees the mobile (BR-EM-09).
    let bare = json!({ "to_status": "exited", "reason": "Left" });
    let (s, _, b) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-exit00")], Some(bare)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("exit_details_required")));
    let exit = json!({ "to_status": "exited", "reason": "Left",
                       "exit": { "last_working_day": "2026-11-15", "exit_reason_id": w.resigned, "remarks": "Moved city" } });
    let (s, _, req) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-exit01")], Some(exit)).await;
    assert_eq!((s, req["effective_date"].as_str()), (StatusCode::CREATED, Some("2026-11-16")), "{req}");
    let r5 = req["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &act(&r5, "approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("effective")), "{b}");
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{meena}?as_of=2026-11-10"), admin, &[], None).await;
    assert_eq!((e["employment"]["last_working_day"].as_str(), e["job"]["valid_to"].as_str()), (Some("2026-11-15"), Some("2026-11-15")), "{e}");
    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{meena}"), admin, &[], None).await;
    assert_eq!(e["status"]["value"], "exited");
    let after = json!({ "to_status": "active", "effective_date": "2026-12-01", "reason": "x" });
    let (s, _, b) = call(&w, Method::POST, &raise(&meena), hr, &[("idempotency-key", "key-st-after1")], Some(after)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("employee_not_current")));
    hire_at_blr(&w, "Ravi", "+919800000021", "key-st-ravi01").await; // same mobile, now free
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn employee_details_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let asha = hire_at_blr(&w, "Asha", "+919800000031", "key-dt-asha01").await;
    let at = |p: &str| format!("/v1/employees/{asha}{p}");
    let m = |v: &'static str| [("if-match", v)];

    // E9 probation: If-Match, date rules, no_change, stale.
    let ext = json!({ "probation_end_date": "2026-12-31", "reason": "Needs more time" });
    let (s, _, _) = call(&w, Method::PATCH, &at("/probation"), hr, &[], Some(ext.clone())).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let early = json!({ "probation_end_date": "2026-03-01", "reason": "x" });
    let (s, _, b) = call(&w, Method::PATCH, &at("/probation"), hr, &m("\"1\""), Some(early)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("date_before_joining")), "{b}");
    let (s, _, b) = call(&w, Method::PATCH, &at("/probation"), hr, &m("\"1\""), Some(json!({ "probation_end_date": "2026-12-31" }))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("reason")));
    let (s, h, b) = call(&w, Method::PATCH, &at("/probation"), hr, &m("\"1\""), Some(ext.clone())).await;
    assert_eq!((s, b["employment"]["probation_end_date"].as_str()), (StatusCode::OK, Some("2026-12-31")), "{b}");
    assert_eq!(h["etag"], "\"2\"");
    let (s, _, b) = call(&w, Method::PATCH, &at("/probation"), hr, &m("\"2\""), Some(ext.clone())).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("no_change")));
    let (s, _, _) = call(&w, Method::PATCH, &at("/probation"), hr, &m("\"1\""), Some(ext)).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);

    // E10 custom fields: typed values, inactive option, mandatory, delete with null.
    let (choice, num, must) = (w.cf_choice.to_string(), w.cf_num.to_string(), w.cf_must.to_string());
    let (s, h, b) = call(&w, Method::PUT, &at("/custom-fields"), hr, &m("\"2\""), Some(json!({ &choice: "A", &num: 12.5, &must: true }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(h["etag"], "\"3\"");
    let cf = &b["custom_fields"];
    assert_eq!((cf[&choice].as_str(), cf[&num].as_f64(), cf[&must].as_bool()), (Some("A"), Some(12.5), Some(true)), "{b}");
    for (body, code) in [
        (json!({ &choice: "C" }), "option_inactive"),
        (json!({ &choice: "Z" }), "invalid_value"),
        (json!({ &num: "twelve" }), "invalid_value"),
        (json!({ &must: null }), "required"),
        (json!({ Uuid::new_v4().to_string(): 1 }), "invalid_value"),
    ] {
        let (s, _, b) = call(&w, Method::PUT, &at("/custom-fields"), hr, &m("\"3\""), Some(body.clone())).await;
        assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some(code)), "{body} -> {b}");
    }
    let (s, _, b) = call(&w, Method::PUT, &at("/custom-fields"), hr, &m("\"3\""), Some(json!({ &choice: null }))).await;
    assert_eq!((s, b["custom_fields"].get(&choice)), (StatusCode::OK, None), "{b}");

    // E11 enrolment status.
    let (s, _, _) = call(&w, Method::PATCH, &at("/enrolment-status"), hr, &m("\"4\""), Some(json!({ "enrolment_status": "lost" }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, h, b) = call(&w, Method::PATCH, &at("/enrolment-status"), hr, &m("\"4\""), Some(json!({ "enrolment_status": "enrolled" }))).await;
    assert_eq!((s, b["enrolment_status"].as_str()), (StatusCode::OK, Some("enrolled")), "{b}");
    assert_eq!(h["etag"], "\"5\"");

    // E8 then E3 history: versions, status, periods, personal changes; unknown employee is 404.
    let (s, _, _) = call(&w, Method::PATCH, &at("/personal"), hr, &m("\"5\""), Some(json!({ "full_name": "Asha K" }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, hist) = call(&w, Method::GET, &at("/history"), hr, &[], None).await;
    assert_eq!(s, StatusCode::OK, "{hist}");
    assert_eq!((hist["job_versions"][0]["state"].as_str(), hist["status"][0]["status"].as_str()), (Some("effective"), Some("probation")), "{hist}");
    assert_eq!(hist["periods"].as_array().unwrap().len(), 1);
    let pc = hist["personal_changes"].as_array().unwrap();
    assert_eq!((pc.len(), pc[0]["field"].as_str(), pc[0]["old"].as_str(), pc[0]["new"].as_str()), (1, Some("full_name"), Some("Asha"), Some("Asha K")), "{hist}");
    let (s, _, _) = call(&w, Method::GET, &format!("/v1/employees/{}/history", Uuid::new_v4()), hr, &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // E12 rejoin: only after the exit has taken effect, after the last working day, in scope; idempotent.
    let mut rejoin = json!({ "joining_date": "2026-11-20", "initial_status": "active", "reason": "Back",
        "job": { "master_branch_id": w.blr, "department_id": w.dept, "category_id": w.cat,
                 "employee_type_id": w.etype, "payroll_group_id": w.group } });
    let (s, _, b) = call(&w, Method::POST, &at("/rejoin"), admin, &[("idempotency-key", "key-dt-rj0001"), ("if-match", "\"6\"")], Some(rejoin.clone())).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_exited")), "{b}");
    let exit = json!({ "to_status": "exited", "reason": "Left",
                       "exit": { "last_working_day": "2026-11-15", "exit_reason_id": w.resigned } });
    let (_, _, req) = call(&w, Method::POST, &at("/status-requests"), hr, &[("idempotency-key", "key-dt-exit01")], Some(exit)).await;
    let r = req["id"].as_str().unwrap();
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/status-requests/{r}/approve"), admin, &m("\"1\""), Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("effective")), "{b}");
    let (_, _, e) = call(&w, Method::GET, &at(""), admin, &[], None).await;
    let v = format!("\"{}\"", e["version"]);

    rejoin["joining_date"] = json!("2026-11-10");
    let (s, _, b) = call(&w, Method::POST, &at("/rejoin"), admin, &[("idempotency-key", "key-dt-rj0002"), ("if-match", &v)], Some(rejoin.clone())).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("date_before_last_exit")), "{b}");
    rejoin["joining_date"] = json!("2026-11-20");
    rejoin["job"]["master_branch_id"] = json!(w.chn);
    let (s, _, b) = call(&w, Method::POST, &at("/rejoin"), hr, &[("idempotency-key", "key-dt-rj0003"), ("if-match", &v)], Some(rejoin.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    rejoin["job"]["master_branch_id"] = json!(w.blr);
    let (s, _, b) = call(&w, Method::POST, &at("/rejoin"), hr, &[("idempotency-key", "key-dt-rj0004"), ("if-match", &v)], Some(rejoin.clone())).await;
    assert_eq!((s, b["status"]["value"].as_str(), b["employment"]["joining_date"].as_str()), (StatusCode::OK, Some("active"), Some("2026-11-20")), "{b}");
    let (s, _, again) = call(&w, Method::POST, &at("/rejoin"), hr, &[("idempotency-key", "key-dt-rj0004"), ("if-match", &v)], Some(rejoin)).await;
    assert_eq!((s, &again["id"]), (StatusCode::OK, &b["id"]));
    let (_, _, hist) = call(&w, Method::GET, &at("/history"), hr, &[], None).await;
    assert_eq!(hist["periods"].as_array().unwrap().len(), 2, "{hist}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn sign_in_end_to_end() {
    let w = world().await;
    let post = |uri: &'static str, body: Value| call(&w, Method::POST, uri, None, &[], Some(body));

    // Seed an employee with portal access and a temporary PIN (E13–E16 not built yet).
    let asha: Uuid = hire_at_blr(&w, "Asha", "+919800000041", "key-au-asha01").await.parse().unwrap();
    let code: String = sqlx::query_scalar(
        "UPDATE app.employees SET portal_access = 'employee_portal', email = 'asha@example.test' WHERE id = $1 RETURNING employee_code",
    )
    .bind(asha).fetch_one(&w.pool).await.unwrap();
    let user: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, employee_id, display_name) VALUES ($1, 'employee', $2, 'Asha') RETURNING id")
        .bind(w.org).bind(asha).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_credentials (user_id, org_id, kind, secret_hash, state, temporary_expires_at)
                 VALUES ($1, $2, 'pin', $3, 'temporary', now() + interval '7 days')")
        .bind(user).bind(w.org).bind(api::auth::hash_secret("135790".into()).await.unwrap()).execute(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'employee'")
        .bind(w.org).bind(user).execute(&w.pool).await.unwrap();
    let pin_body = |pin: &str, device: &str, platform: &str| json!({
        "org_short_code": w.org_code.to_lowercase(), "employee_code": code, "pin": pin,
        "device": { "id": device, "platform": platform } });

    // A1 with the temporary PIN: everything but A4 is closed until the PIN is changed.
    let (s, _, b) = post("/v1/auth/employee/sign-in", pin_body("135790", "phone-1", "android")).await;
    assert_eq!((s, b["must_change_pin"].as_bool(), b["new_device"].as_bool()), (StatusCode::OK, Some(true), Some(true)), "{b}");
    let tok = b["access_token"].as_str().unwrap().to_string();
    let me = format!("/v1/employees/{asha}");
    let (s, _, b) = call(&w, Method::GET, &me, Some(&tok), &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("pin_change_required")));

    // A4: weak PIN, wrong current PIN, then success.
    let set = |cur: &str, new: &str| json!({ "current_pin": cur, "new_pin": new });
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/employee/pin", Some(&tok), &[], Some(set("135790", "123456"))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("pin_too_weak")));
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/employee/pin", Some(&tok), &[], Some(set("999999", "482913"))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_credentials")));
    let (s, _, _) = call(&w, Method::POST, "/v1/auth/employee/pin", Some(&tok), &[], Some(set("135790", "482913"))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = call(&w, Method::GET, &me, Some(&tok), &[], None).await;
    assert_eq!(s, StatusCode::OK);

    // Known device, then a second phone: recorded and an event for HR (DEC-20).
    let (_, _, b) = post("/v1/auth/employee/sign-in", pin_body("482913", "phone-1", "android")).await;
    assert_eq!((b["must_change_pin"].as_bool(), b["new_device"].as_bool()), (Some(false), Some(false)), "{b}");
    let (_, _, b) = post("/v1/auth/employee/sign-in", pin_body("482913", "phone-2", "ios")).await;
    assert_eq!(b["new_device"].as_bool(), Some(true));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE aggregate_id = $1 AND event_type = 'NewDeviceSignIn'")
        .bind(user).fetch_one(&w.pool).await.unwrap();
    assert_eq!(events, 1);

    // A10 for the employee: own scope.
    let tok = b["access_token"].as_str().unwrap().to_string();
    let (s, _, acc) = call(&w, Method::GET, "/v1/me/access", Some(&tok), &[], None).await;
    assert_eq!((s, acc["scope"]["kind"].as_str(), acc["employee_id"].as_str()), (StatusCode::OK, Some("own"), Some(asha.to_string().as_str())), "{acc}");
    assert!(acc["permissions"].as_array().unwrap().contains(&json!("employee.view")));

    // A8 rotates; reusing the old refresh token ends the session, so the new one dies too.
    let rt = b["refresh_token"].as_str().unwrap().to_string();
    let (s, _, r2) = post("/v1/auth/refresh", json!({ "refresh_token": rt })).await;
    assert_eq!(s, StatusCode::OK, "{r2}");
    let (s, _, b) = post("/v1/auth/refresh", json!({ "refresh_token": rt })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("session_revoked")));
    let (s, _, _) = post("/v1/auth/refresh", json!({ "refresh_token": r2["refresh_token"] })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = post("/v1/auth/refresh", json!({ "refresh_token": format!("{}.{}.x", Uuid::new_v4(), Uuid::new_v4()) })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // A9: the access token stops working at once.
    let (_, _, b) = post("/v1/auth/employee/sign-in", pin_body("482913", "phone-1", "android")).await;
    let tok = b["access_token"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, "/v1/auth/sign-out", Some(&tok), &[], None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, b) = call(&w, Method::GET, "/v1/me/access", Some(&tok), &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("session_revoked")));

    // A2 always 202; A3 with a known code (email sending not built), single use.
    let who = json!({ "org_short_code": w.org_code, "employee_code": code });
    let (s, _, _) = post("/v1/auth/employee/otp/request", who.clone()).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let (s, _, _) = post("/v1/auth/employee/otp/request", json!({ "org_short_code": "NOPE", "employee_code": "X" })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    sqlx::query("UPDATE app.user_credentials SET otp_hash = $2, otp_expires_at = now() + interval '10 minutes' WHERE user_id = $1")
        .bind(user).bind(api::auth::hash_secret("246810".into()).await.unwrap()).execute(&w.pool).await.unwrap();
    let otp = |c: &str| json!({ "org_short_code": w.org_code, "employee_code": code, "code": c });
    let (s, _, _) = post("/v1/auth/employee/otp/verify", otp("111111")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = post("/v1/auth/employee/otp/verify", otp("246810")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, _, _) = post("/v1/auth/employee/otp/verify", otp("246810")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Unknown organisation or code: the same answer. Plan without the app: web only (MD-02).
    let (s, _, b) = post("/v1/auth/employee/sign-in", json!({ "org_short_code": "NOPE", "employee_code": code, "pin": "482913" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_credentials")));
    sqlx::query("UPDATE app.plans SET includes_mobile_app = false WHERE id = (SELECT plan_id FROM app.organisations WHERE id = $1)")
        .bind(w.org).execute(&w.pool).await.unwrap();
    let (s, _, b) = post("/v1/auth/employee/sign-in", pin_body("482913", "phone-1", "android")).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("mobile_app_not_in_plan")));
    let (s, _, _) = post("/v1/auth/employee/sign-in", pin_body("482913", "browser-1", "web")).await;
    assert_eq!(s, StatusCode::OK);

    // Lockout: 4 wrong → 401, the 5th locks, then even the right PIN is refused (AC-EM-19.6).
    for _ in 0..4 {
        let (s, _, _) = post("/v1/auth/employee/sign-in", pin_body("000001", "browser-1", "web")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, b) = post("/v1/auth/employee/sign-in", pin_body("000001", "browser-1", "web")).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::LOCKED, Some("account_locked")), "{b}");
    assert!(b["meta"]["locked_until"].is_string());
    let (s, _, _) = post("/v1/auth/employee/sign-in", pin_body("482913", "browser-1", "web")).await;
    assert_eq!(s, StatusCode::LOCKED);

    // Admin: invitation → password + authenticator → sign-in in two steps.
    let email = format!("new-admin-{}@example.test", w.org_code.to_lowercase());
    let admin: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, email, display_name, state) VALUES ($1, 'admin', $2, 'New Admin', 'invited') RETURNING id")
        .bind(w.org).bind(&email).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'org_admin'")
        .bind(w.org).bind(admin).execute(&w.pool).await.unwrap();
    let invite = api::auth::issue_invite_token(&w.state, admin, w.org);
    let (s, _, b) = post("/v1/auth/admin/accept-invite", json!({ "invite_token": invite, "password": "short" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("password_too_weak")));
    let (s, _, b) = post("/v1/auth/admin/accept-invite", json!({ "invite_token": invite, "password": "correct horse battery" })).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let secret = b["mfa_setup"]["secret"].as_str().unwrap().to_string();
    assert!(b["mfa_setup"]["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/"));
    let (s, _, b) = post("/v1/auth/admin/accept-invite", json!({ "invite_token": invite, "password": "correct horse battery" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::GONE, Some("invite_invalid_or_expired")));
    let (s, _, _) = post("/v1/auth/admin/accept-invite", json!({ "invite_token": "nonsense", "password": "correct horse battery" })).await;
    assert_eq!(s, StatusCode::GONE);

    let (s, _, _) = post("/v1/auth/admin/sign-in", json!({ "email": email, "password": "wrong password!!" })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = post("/v1/auth/admin/sign-in", json!({ "email": "nobody@example.test", "password": "correct horse battery" })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = post("/v1/auth/admin/sign-in", json!({ "email": email.to_uppercase(), "password": "correct horse battery" })).await;
    assert_eq!((s, b["mfa_setup_required"].as_bool()), (StatusCode::OK, Some(false)), "{b}");
    let mfa = b["mfa_token"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::GET, "/v1/me/access", Some(&mfa), &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "an MFA token is not an access token");
    let code_now = totp_rs::TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30,
        totp_rs::Secret::Encoded(secret).to_bytes().unwrap(), Some("Payroll".into()), email.clone())
        .unwrap().generate_current().unwrap();
    let wrong = if code_now == "000000" { "111111" } else { "000000" };
    let (s, _, _) = post("/v1/auth/admin/mfa", json!({ "mfa_token": mfa, "totp_code": wrong })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = post("/v1/auth/admin/mfa", json!({ "mfa_token": mfa, "totp_code": code_now })).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, _, acc) = call(&w, Method::GET, "/v1/me/access", b["access_token"].as_str(), &[], None).await;
    assert_eq!((s, acc["scope"]["kind"].as_str(), acc["user"]["kind"].as_str()), (StatusCode::OK, Some("organisation"), Some("admin")), "{acc}");
    assert!(acc["permissions"].as_array().unwrap().contains(&json!("employee.rejoin")));
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn portal_access_end_to_end() {
    let w = world().await;
    let hr = Some(w.hr_blr.as_str());
    let kiran = hire_at_blr(&w, "Kiran", "+919800000051", "key-pa-kiran1").await;
    let code: String = sqlx::query_scalar("SELECT employee_code FROM app.employees WHERE id = $1::uuid")
        .bind(&kiran).fetch_one(&w.pool).await.unwrap();
    let at = |p: &str| format!("/v1/employees/{kiran}{p}");
    let sign_in = |pin: &str| call(&w, Method::POST, "/v1/auth/employee/sign-in", None, &[],
        Some(json!({ "org_short_code": w.org_code, "employee_code": code, "pin": pin, "device": { "id": "phone-1", "platform": "android" } })));
    let key = |k: &'static str| [("idempotency-key", k)];

    // E13: key required; PIN shown once, not cached; a retry with the same key is refused, not replayed.
    let (s, _, _) = call(&w, Method::POST, &at("/portal-access"), hr, &[], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, h, b) = call(&w, Method::POST, &at("/portal-access"), hr, &key("key-pa-grant1"), Some(json!({ "send_email": false }))).await;
    assert_eq!((s, b["emailed"].as_bool()), (StatusCode::CREATED, Some(false)), "{b}");
    assert_eq!(h["cache-control"], "no-store");
    let pin = b["temporary_pin"].as_str().unwrap().to_string();
    assert_eq!(pin.len(), 6);
    let (s, _, b) = call(&w, Method::POST, &at("/portal-access"), hr, &key("key-pa-grant1"), Some(json!({ "send_email": false }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("pin_already_issued")));
    let (s, _, b) = call(&w, Method::POST, &at("/portal-access"), hr, &key("key-pa-grant2"), Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("already_has_access")));
    let leaked: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND (new_values::text LIKE $2 OR old_values::text LIKE $2))
              + (SELECT count(*) FROM app.idempotency_keys WHERE org_id = $1 AND response_body::text LIKE $2)
              + (SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND payload::text LIKE $2)",
    )
    .bind(w.org).bind(format!("%{pin}%")).fetch_one(&w.pool).await.unwrap();
    assert_eq!(leaked, 0, "the temporary PIN must not be stored anywhere");
    let (_, _, e) = call(&w, Method::GET, &at(""), hr, &[], None).await;
    assert_eq!(e["portal_access"], "employee_portal");

    // The employee signs in; E4 shows the phone.
    let (s, _, b) = sign_in(&pin).await;
    assert_eq!((s, b["must_change_pin"].as_bool()), (StatusCode::OK, Some(true)), "{b}");
    let tok = b["access_token"].as_str().unwrap().to_string();
    let (s, _, d) = call(&w, Method::GET, &at("/devices"), hr, &[], None).await;
    assert_eq!((s, d.as_array().map(Vec::len), d[0]["platform"].as_str()), (StatusCode::OK, Some(1), Some("android")), "{d}");

    // E16: a lockout cleared early.
    for _ in 0..5 {
        sign_in("000001").await;
    }
    let (s, _, _) = sign_in(&pin).await;
    assert_eq!(s, StatusCode::LOCKED);
    let (s, _, _) = call(&w, Method::POST, &at("/unlock"), hr, &[], None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = sign_in(&pin).await;
    assert_eq!(s, StatusCode::OK);

    // E15: new temporary PIN; the old PIN and every session stop at once.
    let (s, h, b) = call(&w, Method::POST, &at("/pin-reset"), hr, &[], Some(json!({}))).await;
    assert_eq!((s, h["cache-control"].to_str().unwrap()), (StatusCode::OK, "no-store"), "{b}");
    let new_pin = b["temporary_pin"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/employee/pin", Some(&tok), &[], Some(json!({ "current_pin": pin, "new_pin": "482913" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("session_revoked")));
    if new_pin != pin {
        let (s, _, _) = sign_in(&pin).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, b) = sign_in(&new_pin).await;
    assert_eq!((s, b["must_change_pin"].as_bool()), (StatusCode::OK, Some(true)));
    let tok = b["access_token"].as_str().unwrap().to_string();

    // E14: reason required; access, credential and sessions end; repeating it is no_access.
    let (s, _, _) = call(&w, Method::DELETE, &at("/portal-access"), hr, &[], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, _) = call(&w, Method::DELETE, &at("/portal-access"), hr, &[], Some(json!({ "reason": "Contract role" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = call(&w, Method::GET, "/v1/me/access", Some(&tok), &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = sign_in(&new_pin).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    for (m, p) in [(Method::DELETE, "/portal-access"), (Method::POST, "/pin-reset"), (Method::POST, "/unlock")] {
        let body = if p == "/portal-access" { json!({ "reason": "x" }) } else { json!({}) };
        let (s, _, b) = call(&w, m, &at(p), hr, &[], Some(body)).await;
        assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("no_access")), "{p}");
    }
    let (_, _, e) = call(&w, Method::GET, &at(""), hr, &[], None).await;
    assert_eq!(e["portal_access"], "none");

    // Granting again reactivates the same user with a fresh temporary PIN.
    let (s, _, b) = call(&w, Method::POST, &at("/portal-access"), hr, &key("key-pa-grant3"), Some(json!({}))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let (s, _, _) = sign_in(b["temporary_pin"].as_str().unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM app.users WHERE employee_id = $1::uuid")
        .bind(&kiran).fetch_one(&w.pool).await.unwrap();
    assert_eq!(users, 1);

    // Not a current employee: refused.
    let gone = hire_at_blr(&w, "Gone", "+919800000052", "key-pa-gone01").await;
    sqlx::query("UPDATE app.employees SET is_current = false WHERE id = $1::uuid").bind(&gone).execute(&w.pool).await.unwrap();
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{gone}/portal-access"), hr, &key("key-pa-gone02"), Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("employee_not_current")));
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn org_structure_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let hr = Some(w.hr_blr.as_str());
    let key = |k: &'static str| [("idempotency-key", k)];
    let field = |b: &Value| b["errors"][0]["code"].as_str().map(str::to_string);

    // O2 branches: default time zone, replay, duplicate code, bad code, unknown zone.
    let body = json!({ "short_code": "MDU", "name": "Madurai" });
    let (s, h, br) = call(&w, Method::POST, "/v1/org/branches", admin, &key("key-or-br01"), Some(body.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{br}");
    assert_eq!((br["time_zone"].as_str(), br["state"].as_str(), br["version"].as_i64()), (Some("Asia/Kolkata"), Some("active"), Some(1)));
    assert_eq!(h["etag"], "\"1\"");
    let (s, _, again) = call(&w, Method::POST, "/v1/org/branches", admin, &key("key-or-br01"), Some(body.clone())).await;
    assert_eq!((s, &again["id"]), (StatusCode::CREATED, &br["id"]));
    let (s, _, b) = call(&w, Method::POST, "/v1/org/branches", admin, &key("key-or-br02"), Some(body)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("short_code_in_use")), "{b}");
    let (s, _, b) = call(&w, Method::POST, "/v1/org/branches", admin, &key("key-or-br03"), Some(json!({ "short_code": "md", "name": "X" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_short_code")));
    let (s, _, b) = call(&w, Method::POST, "/v1/org/branches", admin, &key("key-or-br04"), Some(json!({ "short_code": "TRY", "name": "X", "time_zone": "Mars/Base" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_time_zone")));

    // O1 list and search; O3 rename needs If-Match with the current version.
    let (s, _, b) = call(&w, Method::GET, "/v1/org/branches?q=madu", hr, &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(1)), "{b}");
    let uri = format!("/v1/org/branches/{}", br["id"].as_str().unwrap());
    let (s, _, _) = call(&w, Method::PATCH, &uri, admin, &[], Some(json!({ "name": "Madurai Main" }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, _) = call(&w, Method::PATCH, &uri, admin, &[("if-match", "\"9\"")], Some(json!({ "name": "Madurai Main" }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);
    let (s, _, b) = call(&w, Method::PATCH, &uri, admin, &[("if-match", "\"1\"")], Some(json!({ "name": "Madurai Main", "address": "" }))).await;
    assert_eq!((s, b["name"].as_str(), b["version"].as_i64(), &b["address"]), (StatusCode::OK, Some("Madurai Main"), Some(2), &Value::Null), "{b}");
    let renames: i64 = sqlx::query_scalar("SELECT count(*) FROM app.reference_name_history WHERE item_id = $1::uuid")
        .bind(br["id"].as_str().unwrap()).fetch_one(&w.pool).await.unwrap();
    assert_eq!(renames, 2, "created + renamed");

    // O4: deactivating a branch with staff is allowed and lists them (edge case 9).
    let (s, _, e) = call(&w, Method::POST, "/v1/employees", admin, &key("key-or-emp1"), Some(new_employee(&w, "Chennai One", "+919800000101"))).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    let chn_emp = e["id"].as_str().unwrap().to_string();
    let chn_uri = format!("/v1/org/branches/{}/deactivate", w.chn);
    let (s, _, b) = call(&w, Method::POST, &chn_uri, admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("required")));
    let (s, _, b) = call(&w, Method::POST, &chn_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "reason": "Closing" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("inactive")), "{b}");
    assert!(b["employees_with_this_branch"].as_array().unwrap().iter().any(|x| x["id"] == chn_emp.as_str()));
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/org/branches/{}/activate", w.chn), admin, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("active")));

    // O5–O9 lists: code unique ignoring case, unknown list, template only on empty lists.
    let (s, _, b) = call(&w, Method::POST, "/v1/org/lists/departments", admin, &key("key-or-ls01"), Some(json!({ "code": "ops", "name": "Ops again" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("code_in_use")), "{b}");
    let (s, _, d) = call(&w, Method::POST, "/v1/org/lists/designations", admin, &key("key-or-ls02"), Some(json!({ "code": "OPR", "name": "Operator" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{d}");
    let (s, _, _) = call(&w, Method::GET, "/v1/org/lists/colours", admin, &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, b) = call(&w, Method::GET, "/v1/org/lists/exit-reasons?state=active", admin, &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(6)));
    let d_uri = format!("/v1/org/lists/designations/{}", d["id"].as_str().unwrap());
    let (s, _, b) = call(&w, Method::PATCH, &d_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "name": "Machine operator" }))).await;
    assert_eq!((s, b["name"].as_str()), (StatusCode::OK, Some("Machine operator")));
    let (s, _, b) = call(&w, Method::POST, &format!("{d_uri}/deactivate"), admin, &[("if-match", "\"2\"")], Some(json!({ "reason": "Merged" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("inactive")));
    let (s, _, b) = call(&w, Method::POST, "/v1/org/lists/apply-template", admin, &key("key-or-tp01"), Some(json!({ "sector": "factory" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("lists_not_empty")), "{b}");

    // O10–O11 contractors: workers are those linked on the date and visible to the caller.
    let (s, _, c) = call(&w, Method::POST, "/v1/org/contractors", admin, &key("key-or-ct01"), Some(json!({ "code": "ACME", "name": "Acme Labour", "contact_phone": "+919800000199" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{c}");
    let mut body = new_employee(&w, "Contract One", "+919800000102");
    body["job"]["contractor_id"] = c["id"].clone();
    let (s, _, e) = call(&w, Method::POST, "/v1/employees", admin, &key("key-or-emp2"), Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    let workers = format!("/v1/org/contractors/{}/workers", c["id"].as_str().unwrap());
    let (s, _, b) = call(&w, Method::GET, &workers, admin, &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(1)), "{b}");
    let (_, _, b) = call(&w, Method::GET, &workers, hr, &[], None).await;
    assert_eq!(b["items"].as_array().map(Vec::len), Some(0), "HR BLR can't see a Chennai worker");
    let (_, _, b) = call(&w, Method::GET, &format!("{workers}?as_of=2026-03-01"), admin, &[], None).await;
    assert_eq!(b["items"].as_array().map(Vec::len), Some(0), "before joining");

    // O12–O15 payroll groups.
    let (s, _, b) = call(&w, Method::POST, "/v1/org/payroll-groups", admin, &key("key-or-pg01"), Some(json!({ "name": "Weekly", "pay_frequency": "weekly" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_cycle")), "{b}");
    let (s, _, g) = call(&w, Method::POST, "/v1/org/payroll-groups", admin, &key("key-or-pg02"), Some(json!({ "name": "Weekly", "pay_frequency": "weekly", "week_start_day": 1 }))).await;
    assert_eq!((s, g["member_count_today"].as_i64()), (StatusCode::CREATED, Some(0)), "{g}");
    let (s, _, b) = call(&w, Method::POST, "/v1/org/payroll-groups", admin, &key("key-or-pg03"), Some(json!({ "name": "weekly" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("name_in_use")));
    let g_uri = format!("/v1/org/payroll-groups/{}", g["id"].as_str().unwrap());
    let (s, _, b) = call(&w, Method::PATCH, &g_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "pay_frequency": "monthly" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_cycle")), "week_start_day still set: {b}");
    let (s, _, g) = call(&w, Method::PATCH, &g_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "name": "Weekly staff", "pay_frequency": "monthly", "week_start_day": null, "month_start_day": 26 }))).await;
    assert_eq!((s, g["month_start_day"].as_i64(), g["version"].as_i64()), (StatusCode::OK, Some(26), Some(2)), "{g}");

    let assign = format!("{g_uri}/assign");
    let req = json!({ "effective_from": "2026-05-01", "employee_ids": [chn_emp, e["id"], Uuid::new_v4()], "reason": "Move to 26th cycle" });
    let (s, _, b) = call(&w, Method::POST, &assign, hr, &key("key-or-as01"), Some(req.clone())).await;
    assert_eq!((s, b["changed"].as_i64()), (StatusCode::OK, Some(0)), "HR BLR edits no Chennai staff: {b}");
    assert!(b["skipped"].as_array().unwrap().iter().all(|x| x["code"] == "not_found"));
    let (s, _, b) = call(&w, Method::POST, &assign, admin, &key("key-or-as02"), Some(req.clone())).await;
    assert_eq!((s, b["changed"].as_i64(), b["skipped"].as_array().map(Vec::len)), (StatusCode::OK, Some(2), Some(1)), "{b}");
    let (_, _, replay) = call(&w, Method::POST, &assign, admin, &key("key-or-as02"), Some(req)).await;
    assert_eq!(replay, b);
    let rule = json!({ "effective_from": "2026-05-01", "rule": { "branch_ids": [w.chn] }, "reason": "Again" });
    let (_, _, b) = call(&w, Method::POST, &assign, admin, &key("key-or-as03"), Some(rule)).await;
    assert_eq!(b["changed"].as_i64(), Some(0), "{b}");
    assert!(b["skipped"].as_array().unwrap().iter().all(|x| x["code"] == "no_change"));
    let (_, _, emp) = call(&w, Method::GET, &format!("/v1/employees/{chn_emp}?as_of=2026-04-15"), admin, &[], None).await;
    assert_eq!(emp["job"]["payroll_group"]["id"], json!(w.group), "April keeps the old group");
    let (_, _, b) = call(&w, Method::GET, "/v1/org/payroll-groups", admin, &[], None).await;
    let mine = b["items"].as_array().unwrap().iter().find(|x| x["id"] == g["id"]).unwrap().clone();
    assert_eq!(mine["member_count_today"].as_i64(), Some(2));

    let (s, _, b) = call(&w, Method::DELETE, &g_uri, admin, &[("if-match", "\"2\"")], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("group_has_members")));
    let (s, _, b) = call(&w, Method::DELETE, &format!("/v1/org/payroll-groups/{}", w.group), admin, &[("if-match", "\"1\"")], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("cannot_delete_default")), "{b}");
    let (_, _, spare) = call(&w, Method::POST, "/v1/org/payroll-groups", admin, &key("key-or-pg04"), Some(json!({ "name": "Spare", "pay_frequency": "daily" }))).await;
    let (s, _, _) = call(&w, Method::DELETE, &format!("/v1/org/payroll-groups/{}", spare["id"].as_str().unwrap()), admin, &[("if-match", "\"1\"")], None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // O16–O17 settings: defaults filled in, each bad key named, versions by date.
    let (s, _, b) = call(&w, Method::GET, "/v1/org/settings", hr, &[], None).await;
    assert_eq!((s, b["groups"]["OR"]["currency"].as_str(), &b["groups"]["EM"]["valid_from"]), (StatusCode::OK, Some("INR"), &Value::Null), "{b}");
    let bad = json!({ "valid_from": "2026-12-01", "values": { "week_start_day": 9, "colour": "red" }, "reason": "x" });
    let (s, _, b) = call(&w, Method::PUT, "/v1/org/settings/OR", admin, &key("key-or-st01"), Some(bad)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let fields: Vec<&str> = b["errors"].as_array().unwrap().iter().map(|e| e["field"].as_str().unwrap()).collect();
    assert!(fields.contains(&"values.colour") && fields.contains(&"values.week_start_day"), "{b}");
    let good = json!({ "valid_from": "2026-12-01", "values": { "week_start_day": 7 }, "reason": "Sunday start" });
    let (s, _, b) = call(&w, Method::PUT, "/v1/org/settings/OR", admin, &key("key-or-st02"), Some(good)).await;
    assert_eq!((s, b["values"]["week_start_day"].as_i64()), (StatusCode::CREATED, Some(7)), "{b}");
    let (_, _, b) = call(&w, Method::GET, "/v1/org/settings?as_of=2026-12-01", admin, &[], None).await;
    assert_eq!((b["groups"]["OR"]["week_start_day"].as_i64(), b["groups"]["OR"]["valid_from"].as_str()), (Some(7), Some("2026-12-01")));
    let (_, _, b) = call(&w, Method::GET, "/v1/org/settings?as_of=2026-11-30", admin, &[], None).await;
    assert_eq!(b["groups"]["OR"]["week_start_day"].as_i64(), Some(1));
    let (s, _, _) = call(&w, Method::PUT, "/v1/org/settings/ZZ", admin, &key("key-or-st03"), Some(json!({}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // O18 preview reads the sequence and never takes a number.
    let preview = format!("/v1/org/settings/employee-code/preview?branch_id={}", w.chn);
    let (s, _, p1) = call(&w, Method::GET, &preview, admin, &[], None).await;
    let (_, _, p2) = call(&w, Method::GET, &preview, admin, &[], None).await;
    assert_eq!((s, &p1), (StatusCode::OK, &p2));
    assert_eq!(p1["next_code"], format!("{}CHN0003", w.org_code), "two hired at CHN so far: {p1}");
    let (s, _, b) = call(&w, Method::GET, "/v1/org/settings/employee-code/preview?pattern=%7B%7D", admin, &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_pattern")));

    // O19 custom field definitions.
    let (s, _, b) = call(&w, Method::POST, "/v1/org/custom-fields", admin, &key("key-or-cf01"), Some(json!({ "label": "Blood group", "field_type": "dropdown" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("options_required")));
    let (s, _, f) = call(&w, Method::POST, "/v1/org/custom-fields", admin, &key("key-or-cf02"), Some(json!({ "label": "Blood group", "field_type": "dropdown", "options": ["A+", "B+"] }))).await;
    assert_eq!(s, StatusCode::CREATED, "{f}");
    let (s, _, b) = call(&w, Method::POST, "/v1/org/custom-fields", admin, &key("key-or-cf03"), Some(json!({ "label": "blood GROUP", "field_type": "text" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("label_in_use")));
    let f_uri = format!("/v1/org/custom-fields/{}", f["id"].as_str().unwrap());
    let (s, _, b) = call(&w, Method::PATCH, &f_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "options": ["A+"] }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_value")), "can't remove an option");
    let (s, _, b) = call(&w, Method::PATCH, &f_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "options": ["A+", "B+", "O+"], "inactive_options": ["B+"] }))).await;
    assert_eq!((s, b["options"].as_array().map(Vec::len), &b["inactive_options"]), (StatusCode::OK, Some(3), &json!(["B+"])), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("{f_uri}/deactivate"), admin, &[("if-match", "\"2\"")], Some(json!({ "reason": "Not needed" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("inactive")));
    let (_, _, b) = call(&w, Method::GET, "/v1/org/custom-fields?state=active", hr, &[], None).await;
    assert!(b["items"].as_array().unwrap().iter().all(|x| x["id"] != f["id"]));

    // Type changes only while no employee has a value; options follow the type.
    let (_, _, t) = call(&w, Method::POST, "/v1/org/custom-fields", admin, &key("key-or-cf04"), Some(json!({ "label": "Nickname", "field_type": "text" }))).await;
    let t_uri = format!("/v1/org/custom-fields/{}", t["id"].as_str().unwrap());
    let (s, _, b) = call(&w, Method::PATCH, &t_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "field_type": "dropdown" }))).await;
    assert_eq!((s, field(&b).as_deref()), (StatusCode::UNPROCESSABLE_ENTITY, Some("options_required")));
    let (s, _, b) = call(&w, Method::PATCH, &t_uri, admin, &[("if-match", "\"1\"")], Some(json!({ "field_type": "dropdown", "options": ["X", "Y"] }))).await;
    assert_eq!((s, b["field_type"].as_str(), b["options"].as_array().map(Vec::len)), (StatusCode::OK, Some("dropdown"), Some(2)), "{b}");
    let (s, _, b) = call(&w, Method::PATCH, &t_uri, admin, &[("if-match", "\"2\"")], Some(json!({ "field_type": "number" }))).await;
    assert_eq!((s, b["field_type"].as_str(), &b["options"]), (StatusCode::OK, Some("number"), &Value::Null), "{b}");
    sqlx::query("INSERT INTO app.custom_field_values (org_id, employee_id, field_id, value_number) VALUES ($1, $2::uuid, $3, 7)")
        .bind(w.org).bind(&chn_emp).bind(w.cf_num).execute(&w.pool).await.unwrap();
    let (s, _, b) = call(&w, Method::PATCH, &format!("/v1/org/custom-fields/{}", w.cf_num), admin, &[("if-match", "\"1\"")], Some(json!({ "field_type": "text" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("cannot_change_type_with_values")), "{b}");

    // Each successful write is audited once under its permission code; refused ones leave nothing.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND action LIKE 'org.%'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(n, 14);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND action = 'employee.custom_fields.manage'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(n, 6);
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn roles_and_users_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let hr = Some(w.hr_blr.as_str());
    let key = |k: &'static str| [("idempotency-key", k)];
    let ver = |v: &Value| format!("\"{}\"", v["version"]);

    // R1 catalogue; R2 the four built-in roles.
    let (s, _, b) = call(&w, Method::GET, "/v1/access/permissions", admin, &[], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b["items"].as_array().unwrap().iter().any(|p| p["code"] == "support.access" && p["is_fixed"] == true));
    let (_, _, b) = call(&w, Method::GET, "/v1/access/roles", hr, &[], None).await;
    let roles = b["items"].as_array().unwrap().clone();
    let role = |k: &str| roles.iter().find(|r| r["builtin_key"] == k).unwrap().clone();
    assert_eq!((roles.len(), role("org_admin")["user_count"].as_i64()), (4, Some(1)));
    let (org_admin, hr_role, manager) = (role("org_admin"), role("hr_manager"), role("manager"));

    // R3 custom roles: fixed codes refused, own rights only, copies never carry a fixed code.
    let body = json!({ "name": "Auditor", "permissions": ["employee.view", "audit.view"] });
    let (s, _, aud) = call(&w, Method::POST, "/v1/access/roles", admin, &key("key-ac-r01"), Some(body.clone())).await;
    assert_eq!((s, aud["permissions"].as_array().map(Vec::len)), (StatusCode::CREATED, Some(2)), "{aud}");
    let (s, _, b) = call(&w, Method::POST, "/v1/access/roles", admin, &key("key-ac-r02"), Some(body)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("name_in_use")));
    let (s, _, b) = call(&w, Method::POST, "/v1/access/roles", admin, &key("key-ac-r03"), Some(json!({ "name": "S", "permissions": ["support.access"] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("fixed_permission")));
    let (s, _, b) = call(&w, Method::POST, "/v1/access/roles", hr, &key("key-ac-r04"), Some(json!({ "name": "Self", "permissions": ["employee.profile.edit_own"] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("exceeds_own_rights")));
    let (s, _, copy) = call(&w, Method::POST, "/v1/access/roles", admin, &key("key-ac-r05"), Some(json!({ "name": "Admin copy", "copy_from_role_id": org_admin["id"] }))).await;
    assert_eq!(s, StatusCode::CREATED, "{copy}");
    // Org Admin's fixed codes (support.access, attendance.period.step_back) are never copied.
    let fixed = ["support.access", "attendance.period.step_back"];
    let expected = org_admin["permissions"].as_array().unwrap().iter().filter(|p| !fixed.iter().any(|f| *p == f)).count();
    assert!(!copy["permissions"].as_array().unwrap().iter().any(|p| fixed.iter().any(|f| p == f)));
    assert_eq!(copy["permissions"].as_array().map(Vec::len), Some(expected));

    // R4: full set with If-Match; Org Admin is fixed; branch editors can't touch built-in roles.
    let aud_uri = format!("/v1/access/roles/{}/permissions", aud["id"].as_str().unwrap());
    let set = json!({ "permissions": ["employee.view", "report.r10"] });
    let (s, _, _) = call(&w, Method::PUT, &aud_uri, admin, &[], Some(set.clone())).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, aud) = call(&w, Method::PUT, &aud_uri, admin, &[("if-match", "\"1\"")], Some(set)).await;
    assert_eq!((s, aud["permissions"].clone(), aud["version"].as_i64()), (StatusCode::OK, json!(["employee.view", "report.r10"]), Some(2)), "{aud}");
    let audit: Value = sqlx::query_scalar("SELECT jsonb_build_object('old', old_values, 'new', new_values) FROM app.audit_log WHERE target_id = $1::uuid AND old_values IS NOT NULL")
        .bind(aud["id"].as_str().unwrap()).fetch_one(&w.pool).await.unwrap();
    assert_eq!(audit, json!({ "old": { "removed": ["audit.view"] }, "new": { "added": ["report.r10"] } }));
    let (s, _, b) = call(&w, Method::PUT, &format!("/v1/access/roles/{}/permissions", org_admin["id"].as_str().unwrap()), admin, &[("if-match", "\"1\"")], Some(json!({ "permissions": [] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("fixed_permission")));
    let (s, _, b) = call(&w, Method::PUT, &format!("/v1/access/roles/{}/permissions", hr_role["id"].as_str().unwrap()), hr, &[("if-match", "\"1\"")], Some(json!({ "permissions": ["employee.view"] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("exceeds_own_rights")));

    // R6: HR limited to Bengaluru can invite only within Bengaluru.
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let email = format!("new-{tag}@example.test");
    let invite = |scope: Value| json!({ "email": email, "display_name": "New HR", "roles": [scope] });
    let blr_only = json!({ "role_id": hr_role["id"], "scope_kind": "branches", "branch_ids": [w.blr] });
    let (s, _, b) = call(&w, Method::POST, "/v1/access/users", hr, &key("key-ac-u01"), Some(invite(json!({ "role_id": hr_role["id"] })))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("exceeds_own_rights")), "organisation-wide");
    let (s, _, b) = call(&w, Method::POST, "/v1/access/users", hr, &key("key-ac-u02"), Some(invite(json!({ "role_id": hr_role["id"], "scope_kind": "branches", "branch_ids": [w.chn] })))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("exceeds_own_rights")), "another branch");
    let (s, _, b) = call(&w, Method::POST, "/v1/access/users", admin, &key("key-ac-u03"), Some(invite(json!({ "role_id": manager["id"] })))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("validation_failed")), "Manager is for employees");
    let (s, _, nu) = call(&w, Method::POST, "/v1/access/users", hr, &key("key-ac-u04"), Some(invite(blr_only.clone()))).await;
    assert_eq!((s, nu["state"].as_str(), nu["invitation"]["emailed"].as_bool()), (StatusCode::CREATED, Some("invited"), Some(false)), "{nu}");
    assert_eq!(nu["roles"][0]["branch_ids"], json!([w.blr]));
    let mut dup = invite(blr_only.clone());
    dup["email"] = json!(email.to_uppercase());
    let (s, _, b) = call(&w, Method::POST, "/v1/access/users", admin, &key("key-ac-u05"), Some(dup)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("email_in_use")), "{b}");
    let nu_id: Uuid = nu["id"].as_str().unwrap().parse().unwrap();
    let payload: Value = sqlx::query_scalar("SELECT payload FROM app.outbox_events WHERE aggregate_id = $1 AND event_type = 'AdminInvited'")
        .bind(nu_id).fetch_one(&w.pool).await.unwrap();
    assert_eq!(payload, json!({ "user_id": nu_id }), "no invitation token is stored");

    // The invitation works through A7.
    let token = api::auth::issue_invite_token(&w.state, nu_id, w.org);
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/admin/accept-invite", None, &[], Some(json!({ "invite_token": token, "password": "correct horse battery" }))).await;
    assert!(s.is_success(), "{b}");

    // R5
    let (s, _, b) = call(&w, Method::GET, "/v1/access/users?kind=admin", admin, &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(3)));
    let nu = b["items"].as_array().unwrap().iter().find(|u| u["id"] == nu["id"]).unwrap().clone();
    assert_eq!(nu["state"], "active");

    // R7: full set; the last Org Admin keeps the role; a branch HR can't change an organisation-wide admin.
    let nu_uri = format!("/v1/access/users/{nu_id}/roles");
    let (s, _, _) = call(&w, Method::PUT, &nu_uri, admin, &[("if-match", "\"9\"")], Some(json!({ "roles": [] }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);
    let (s, _, nu) = call(&w, Method::PUT, &nu_uri, admin, &[("if-match", ver(&nu).as_str())], Some(json!({ "roles": [{ "role_id": aud["id"] }] }))).await;
    assert_eq!((s, nu["roles"][0]["name"].as_str()), (StatusCode::OK, Some("Auditor")), "{nu}");
    let admin_id: Uuid = sqlx::query_scalar("SELECT ur.user_id FROM app.user_roles ur WHERE ur.role_id = $1::uuid")
        .bind(org_admin["id"].as_str().unwrap()).fetch_one(&w.pool).await.unwrap();
    let (_, _, me) = call(&w, Method::GET, "/v1/access/users?kind=admin", admin, &[], None).await;
    let me = me["items"].as_array().unwrap().iter().find(|u| u["id"] == json!(admin_id)).unwrap().clone();
    let admin_uri = format!("/v1/access/users/{admin_id}/roles");
    let (s, _, b) = call(&w, Method::PUT, &admin_uri, admin, &[("if-match", ver(&me).as_str())], Some(json!({ "roles": [{ "role_id": hr_role["id"] }] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("last_org_admin")), "{b}");
    let (s, _, b) = call(&w, Method::PUT, &admin_uri, hr, &[("if-match", ver(&me).as_str())], Some(json!({ "roles": [{ "role_id": org_admin["id"] }] }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("exceeds_own_rights")));

    // A role change applies from the holder's next request (AC-AC-01.1).
    let nu_token = issue_access_token(SECRET, nu_id, w.org, session(&w.pool, w.org, nu_id).await, "admin");
    let (s, _, _) = call(&w, Method::GET, "/v1/employees", Some(&nu_token), &[], None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, _) = call(&w, Method::PUT, &aud_uri, admin, &[("if-match", "\"2\"")], Some(json!({ "permissions": ["report.r10"] }))).await;
    let (s, _, _) = call(&w, Method::GET, "/v1/employees", Some(&nu_token), &[], None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // R8: suspend ends sessions; the last Org Admin can't be suspended; reactivate restores.
    let suspend = format!("/v1/access/users/{nu_id}/suspend");
    let (s, _, b) = call(&w, Method::POST, &suspend, admin, &[], Some(json!({}))).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("required")));
    let (s, _, b) = call(&w, Method::POST, &suspend, admin, &[], Some(json!({ "reason": "Left the company" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("suspended")), "{b}");
    let (s, _, b) = call(&w, Method::GET, "/v1/employees", Some(&nu_token), &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("session_revoked")));
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/access/users/{admin_id}/suspend"), admin, &[], Some(json!({ "reason": "Test" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("last_org_admin")));
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/access/users/{nu_id}/reactivate"), admin, &[], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("active")), "{b}");

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND action LIKE 'access.%'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(n, 8, "2 role creates, 2 permission sets, 1 invite, 1 role change, suspend, reactivate");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn status_due_job_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let token = "jobs-token-jobs-token-jobs-token-01";
    let jobs = api::router(w.state.clone().with_jobs_token(token));
    let run = |auth: Option<&str>| {
        let mut req = Request::builder().method(Method::POST).uri("/internal/jobs/status-due").header("content-type", "application/json");
        if let Some(t) = auth {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let req = req.body(Body::from(json!({ "org_id": w.org }).to_string())).unwrap();
        let app = jobs.clone();
        async move {
            let res = app.oneshot(req).await.unwrap();
            let s = res.status();
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
            (s, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
        }
    };
    let schedule = |e: String, body: Value, k: &'static str| {
        let w = &w;
        async move {
            let (s, _, req) = call(w, Method::POST, &format!("/v1/employees/{e}/status-requests"), hr, &[("idempotency-key", k)], Some(body)).await;
            assert_eq!(s, StatusCode::CREATED, "{req}");
            let id = req["id"].as_str().unwrap().to_string();
            let (s, _, b) = call(w, Method::POST, &format!("/v1/status-requests/{id}/approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
            assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("scheduled")), "{b}");
            id
        }
    };

    // An exit scheduled for January, and a notice whose starting status will no longer match.
    let leaver = hire_at_blr(&w, "Leaver", "+919800000301", "key-j2-hire01").await;
    let other = hire_at_blr(&w, "Other", "+919800000302", "key-j2-hire02").await;
    let exit = json!({ "to_status": "exited", "reason": "Left",
                       "exit": { "last_working_day": "2027-01-15", "exit_reason_id": w.resigned } });
    let exit_id = schedule(leaver.clone(), exit, "key-j2-exit01").await;
    let notice_id = schedule(other.clone(), json!({ "to_status": "notice", "effective_date": "2027-01-01", "reason": "Resigned" }), "key-j2-ntc01").await;

    // Not due yet: nothing happens.
    let (s, b) = run(Some(token)).await;
    assert_eq!((s, b), (StatusCode::OK, json!({ "applied": [], "failed": [] })));

    // Time passes: both are due today (India time, as the API's today()).
    sqlx::query(
        "UPDATE app.status_change_requests SET effective_date = (now() AT TIME ZONE 'Asia/Kolkata')::date,
           last_working_day = CASE WHEN to_status = 'exited' THEN (now() AT TIME ZONE 'Asia/Kolkata')::date - 1 END
         WHERE id = ANY($1::uuid[])",
    )
    .bind(vec![exit_id.clone(), notice_id.clone()])
    .execute(&w.pool).await.unwrap();
    sqlx::query("UPDATE app.status_change_requests SET from_status = 'active' WHERE id = $1::uuid")
        .bind(&notice_id).execute(&w.pool).await.unwrap();

    // Only the scheduler's token gets in.
    assert_eq!(run(None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(run(Some("jobs-token-jobs-token-jobs-token-XX")).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&w, Method::POST, "/internal/jobs/status-due", None, &[], Some(json!({ "org_id": w.org }))).await.0, StatusCode::UNAUTHORIZED, "no token configured");

    let (s, b) = run(Some(token)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["applied"], json!([exit_id]));
    assert_eq!(b["failed"], json!([{ "request_id": notice_id, "code": "transition_not_allowed" }]));

    let (_, _, e) = call(&w, Method::GET, &format!("/v1/employees/{leaver}"), admin, &[], None).await;
    assert_eq!(e["status"]["value"], "exited", "{e}");
    let (_, _, r) = call(&w, Method::GET, &format!("/v1/status-requests/{exit_id}"), admin, &[], None).await;
    assert_eq!(r["state"], "effective");
    let (_, _, r) = call(&w, Method::GET, &format!("/v1/status-requests/{notice_id}"), admin, &[], None).await;
    assert_eq!(r["state"], "scheduled", "left for HR to sort out");
    let actor: (String, Option<Uuid>) = sqlx::query_as("SELECT actor_kind::text, actor_id FROM app.audit_log WHERE target_id = $1::uuid AND action = 'job.status_due'")
        .bind(&exit_id).fetch_one(&w.pool).await.unwrap();
    assert_eq!(actor, ("system".to_string(), None));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE aggregate_id = $1::uuid AND event_type IN ('StatusChanged', 'EmployeeExited')")
        .bind(&leaver).fetch_one(&w.pool).await.unwrap();
    assert_eq!(events, 2);

    // Safe to run twice: nothing is applied again.
    let (_, b) = run(Some(token)).await;
    assert_eq!(b["applied"], json!([]));
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn mail_and_outbox_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let jobs_token = "jobs-token-jobs-token-jobs-token-02";
    let bearer = format!("Bearer {jobs_token}");
    let auth = [("authorization", bearer.as_str())];
    let mailbox = std::sync::Arc::new(std::sync::Mutex::new(Vec::<api::mail::Email>::new()));
    let no_mail = api::router(w.state.clone().with_jobs_token(jobs_token));
    let with_mail = api::router(
        w.state.clone().with_jobs_token(jobs_token).with_mailer(api::mail::Mailer::Memory(mailbox.clone()), "https://app.test/"),
    );
    let sent_to = |to: &str| mailbox.lock().unwrap().iter().filter(|e| e.to == to).cloned().collect::<Vec<_>>();
    // Runs J4 until nothing is left (dev holds other test organisations' events too).
    let drain = |app: axum::Router| async move {
        for _ in 0..30 {
            let (s, _, b) = call_app(&app, Method::POST, "/internal/outbox/dispatch", None, &auth, None).await;
            assert_eq!(s, StatusCode::OK, "{b}");
            if b["dispatched"] == 0 && b["failed"] == 0 {
                return;
            }
        }
        panic!("outbox never drained");
    };

    let (s, _, _) = call(&w, Method::POST, "/internal/outbox/dispatch", None, &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // R6 queues an invitation. With no mailer it waits, without using up attempts.
    let (_, _, roles) = call(&w, Method::GET, "/v1/access/roles", admin, &[], None).await;
    let hr_role = roles["items"].as_array().unwrap().iter().find(|r| r["builtin_key"] == "hr_manager").unwrap()["id"].clone();
    let email = format!("invitee-{}@example.test", &Uuid::new_v4().simple().to_string()[..8]);
    let body = json!({ "email": email, "display_name": "Invitee <Ann>", "roles": [{ "role_id": hr_role }] });
    let (s, _, u) = call(&w, Method::POST, "/v1/access/users", admin, &[("idempotency-key", "key-ml-inv01")], Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{u}");
    let user: Uuid = u["id"].as_str().unwrap().parse().unwrap();
    let event = |pool: PgPool| async move {
        sqlx::query_as::<_, (bool, i32)>("SELECT dispatched_at IS NOT NULL, attempts FROM app.outbox_events WHERE aggregate_id = $1 AND event_type = 'AdminInvited'")
            .bind(user).fetch_one(&pool).await.unwrap()
    };
    drain(no_mail.clone()).await;
    assert_eq!(event(w.pool.clone()).await, (false, 0));
    let others: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND dispatched_at IS NULL AND event_type <> 'AdminInvited'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(others, 0, "events with no subscriber are marked delivered");

    // With mail on, the invitation goes out once; its link signs the user up (A7).
    drain(with_mail.clone()).await;
    assert_eq!(event(w.pool.clone()).await, (true, 0));
    let mails = sent_to(&email);
    assert_eq!(mails.len(), 1);
    assert!(mails[0].html.contains("Invitee &lt;Ann&gt;"), "name is escaped");
    let link = mails[0].html.split("href=\"").nth(1).unwrap().split('"').next().unwrap().to_string();
    let token = link.strip_prefix("https://app.test/accept-invite?token=").expect("link to the web app").to_string();
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/admin/accept-invite", None, &[], Some(json!({ "invite_token": token, "password": "correct horse battery" }))).await;
    assert!(s.is_success(), "{b}");
    drain(with_mail.clone()).await;
    assert_eq!(sent_to(&email).len(), 1, "never sent twice");

    // E13 with send_email: the PIN arrives by email only when asked and the employee has an email.
    let mut body = new_employee(&w, "Mailed One", "+919800000401");
    let worker_email = format!("worker-{}@example.test", &Uuid::new_v4().simple().to_string()[..8]);
    body["personal"]["email"] = json!(worker_email);
    let (s, _, e) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-ml-emp01")], Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    let (id, code) = (e["id"].as_str().unwrap().to_string(), e["employee_code"].as_str().unwrap().to_string());
    let (s, _, p) = call_app(&with_mail, Method::POST, &format!("/v1/employees/{id}/portal-access"), admin, &[("idempotency-key", "key-ml-pa01")], Some(json!({ "send_email": true }))).await;
    assert_eq!((s, p["emailed"].as_bool()), (StatusCode::CREATED, Some(true)), "{p}");
    let pin = p["temporary_pin"].as_str().unwrap();
    assert!(sent_to(&worker_email)[0].html.contains(pin));
    let (_, _, p) = call_app(&with_mail, Method::POST, &format!("/v1/employees/{id}/pin-reset"), admin, &[], Some(json!({}))).await;
    assert_eq!(p["emailed"].as_bool(), Some(false), "not asked");
    assert_eq!(sent_to(&worker_email).len(), 1);
    let (_, _, p) = call(&w, Method::POST, &format!("/v1/employees/{id}/pin-reset"), admin, &[], Some(json!({ "send_email": true }))).await;
    assert_eq!(p["emailed"].as_bool(), Some(false), "no mailer configured");

    // A2: the one-time code is emailed (in the background) and signs the employee in (A3).
    let who = json!({ "org_short_code": w.org_code, "employee_code": code });
    let (s, _, _) = call_app(&with_mail, Method::POST, "/v1/auth/employee/otp/request", None, &[], Some(who.clone())).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let mut otp = None;
    for _ in 0..50 {
        if let Some(m) = sent_to(&worker_email).into_iter().find(|m| m.subject == "Your sign-in code") {
            otp = Some(m.html.split("<b>").nth(1).unwrap().split("</b>").next().unwrap().to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let mut verify = who.clone();
    verify["code"] = json!(otp.expect("code emailed"));
    let (s, _, b) = call(&w, Method::POST, "/v1/auth/employee/otp/verify", None, &[], Some(verify)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn vendor_console_end_to_end() {
    let w = world().await;
    let tag = Uuid::new_v4().simple().to_string()[..5].to_uppercase();
    let email = format!("vendor-{tag}@example.test");
    let password = "vendor-password-123";

    // Bootstrap (the vendor_user command), then sign in: password, then authenticator.
    let (vendor_id, secret, uri) = api::vendor::create_vendor_user(&w.state, &email, "Ops", password.into()).await.map_err(|e| e.0.code).unwrap();
    assert!(uri.starts_with("otpauth://totp/"));
    let (s, _, _) = call(&w, Method::POST, "/v1/vendor/auth/sign-in", None, &[], Some(json!({ "email": email, "password": "wrong-password-123" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = call(&w, Method::POST, "/v1/vendor/auth/sign-in", None, &[], Some(json!({ "email": email.to_uppercase(), "password": password }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let mfa = b["mfa_token"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, "/v1/vendor/auth/mfa", None, &[], Some(json!({ "mfa_token": mfa, "totp_code": "000000" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let code = totp_rs::TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30, totp_rs::Secret::Encoded(secret).to_bytes().unwrap(), Some("Payroll".into()), email.clone())
        .unwrap()
        .generate_current()
        .unwrap();
    let (s, _, b) = call(&w, Method::POST, "/v1/vendor/auth/mfa", None, &[], Some(json!({ "mfa_token": mfa, "totp_code": code }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let vendor_token = b["access_token"].as_str().unwrap().to_string();
    let vendor = Some(vendor_token.as_str());

    // Tokens don't cross: admin token on vendor routes, vendor token on client routes.
    let (s, _, _) = call(&w, Method::GET, "/v1/vendor/organisations", Some(&w.admin), &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = call(&w, Method::GET, "/v1/employees", vendor, &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // V1: validation, then create.
    let plan: Uuid = sqlx::query_scalar("SELECT plan_id FROM app.organisations WHERE id = $1").bind(w.org).fetch_one(&w.pool).await.unwrap();
    let (s, _, b) = call(&w, Method::POST, "/v1/vendor/organisations", vendor, &[], Some(json!({
        "short_code": "ab", "name": " ", "plan_id": Uuid::new_v4(), "employee_limit": 0, "limit_behaviour": "maybe",
        "admin_email": "nope", "admin_name": "Admin" }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    assert_eq!(b["errors"].as_array().unwrap().len(), 6, "{b}");
    let code = format!("V{}", &tag[..4]);
    let admin_email = format!("first-admin-{tag}@example.test");
    let new_org = json!({ "short_code": code, "name": "Vendor Test Org", "plan_id": plan, "employee_limit": 50,
                          "limit_behaviour": "block", "admin_email": admin_email, "admin_name": "First Admin" });
    let (s, _, b) = call(&w, Method::POST, "/v1/vendor/organisations", vendor, &[], Some(new_org.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let org: Uuid = b["org_id"].as_str().unwrap().parse().unwrap();
    let (s, _, b) = call(&w, Method::POST, "/v1/vendor/organisations", vendor, &[], Some(new_org)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("short_code_in_use")), "{b}");
    // The first admin's invitation is queued for J4.
    let invited: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND event_type = 'AdminInvited'")
        .bind(org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(invited, 1);

    // V2: the new org with no employees; the test world's org shows its headcount.
    let (s, _, b) = call(&w, Method::GET, "/v1/vendor/organisations", vendor, &[], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let find = |id: Uuid| b["items"].as_array().unwrap().iter().find(|o| o["id"] == id.to_string()).cloned().unwrap();
    let mine = find(org);
    assert_eq!((mine["employee_count"].clone(), mine["state"].clone(), mine["version"].clone()), (json!(0), json!("active"), json!(1)));
    assert_eq!(mine["plan"]["id"], plan.to_string());
    let world_count: i64 = sqlx::query_scalar("SELECT count(*) FROM app.employees WHERE org_id = $1 AND is_current").bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(find(w.org)["employee_count"], world_count);

    // V3: If-Match required and checked; then a change.
    let uri = format!("/v1/vendor/organisations/{org}");
    let (s, _, _) = call(&w, Method::PATCH, &uri, vendor, &[], Some(json!({ "employee_limit": 80 }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, b) = call(&w, Method::PATCH, &uri, vendor, &[("if-match", "\"7\"")], Some(json!({ "employee_limit": 80 }))).await;
    assert_eq!((s, b["meta"]["current_version"].clone()), (StatusCode::PRECONDITION_FAILED, json!(1)));
    let (s, _, b) = call(&w, Method::PATCH, &uri, vendor, &[("if-match", "\"1\"")], Some(json!({ "employee_limit": -1, "limit_behaviour": "x" }))).await;
    assert_eq!((s, b["errors"].as_array().map(Vec::len)), (StatusCode::UNPROCESSABLE_ENTITY, Some(2)), "{b}");
    let (s, h, b) = call(&w, Method::PATCH, &uri, vendor, &[("if-match", "\"1\"")], Some(json!({ "employee_limit": 80, "limit_behaviour": "allow_and_alert" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!((b["employee_limit"].clone(), b["limit_behaviour"].clone(), h["etag"].to_str().unwrap()), (json!(80), json!("allow_and_alert"), "\"2\""));

    // V4: reason required; suspend, again (no change), reactivate.
    let (s, _, _) = call(&w, Method::POST, &format!("{uri}/suspend"), vendor, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, b) = call(&w, Method::POST, &format!("{uri}/suspend"), vendor, &[("if-match", "\"2\"")], Some(json!({ "reason": "Unpaid invoice" }))).await;
    assert_eq!((s, b["state"].clone()), (StatusCode::OK, json!("suspended")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("{uri}/suspend"), vendor, &[("if-match", "\"3\"")], Some(json!({ "reason": "Again" }))).await;
    assert_eq!((s, b["version"].clone()), (StatusCode::OK, json!(3)));
    let (s, _, b) = call(&w, Method::POST, &format!("{uri}/reactivate"), vendor, &[("if-match", "\"3\"")], Some(json!({ "reason": "Paid" }))).await;
    assert_eq!((s, b["state"].clone()), (StatusCode::OK, json!("active")), "{b}");

    // Audited in the organisation's own log, as the vendor user.
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM app.audit_log WHERE org_id = $1 AND actor_kind = 'vendor' AND actor_id = $2 ORDER BY occurred_at, id")
        .bind(org).bind(vendor_id).fetch_all(&w.pool).await.unwrap();
    assert_eq!(actions, ["org.create", "org.update", "org.suspend", "org.reactivate"]);

    // A disabled vendor user's token stops working.
    sqlx::query("UPDATE app.vendor_users SET state = 'disabled' WHERE id = $1").bind(vendor_id).execute(&w.pool).await.unwrap();
    let (s, _, _) = call(&w, Method::GET, "/v1/vendor/organisations", vendor, &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

/// Raw request for routes whose body or response isn't JSON (multipart upload, .xlsx download).
async fn call_raw(app: &axum::Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&str, &str)], body: Vec<u8>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = app.clone().oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    (status, headers, axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec())
}

async fn upload(app: &axum::Router, token: &str, key: &str, file: &[u8]) -> (StatusCode, Value) {
    let mut body = b"--XBOUNDARY\r\nContent-Disposition: form-data; name=\"file\"; filename=\"people.csv\"\r\nContent-Type: text/csv\r\n\r\n".to_vec();
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n--XBOUNDARY--\r\n");
    let headers = [("content-type", "multipart/form-data; boundary=XBOUNDARY"), ("idempotency-key", key)];
    let (s, _, b) = call_raw(app, Method::POST, "/v1/employee-imports", Some(token), &headers, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// Waits for the background run to finish (it starts right after the upload).
async fn finished(app: &axum::Router, token: &str, id: &str) -> Value {
    for _ in 0..120 {
        let (s, _, b) = call_app(app, Method::GET, &format!("/v1/employee-imports/{id}"), Some(token), &[], None).await;
        assert_eq!(s, StatusCode::OK, "{b}");
        if b["state"] == "completed" || b["state"] == "failed" {
            return b;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("import {id} never finished");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn bulk_import_end_to_end() {
    let w = world().await;
    let dir = std::env::temp_dir().join(format!("payroll-files-{}", Uuid::new_v4().simple()));
    let jobs_token = "jobs-token-jobs-token-jobs-token-05";
    let app = api::router(w.state.clone().with_files_dir(&dir).with_jobs_token(jobs_token));
    let (admin, hr) = (w.admin.as_str(), w.hr_blr.as_str());
    let n = Uuid::new_v4().as_u128() % 10_000_000;
    let mobile = |i: u128| format!("+9196{:08}", n * 10 + i);

    // B1: the template names the custom fields and marks the mandatory ones.
    let (s, h, bytes) = call_raw(&app, Method::GET, "/v1/employee-imports/template", Some(admin), &[], vec![]).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().contains("spreadsheetml"));
    let mut book: calamine::Xlsx<_> = calamine::open_workbook_from_rs(std::io::Cursor::new(bytes)).unwrap();
    use calamine::Reader;
    let heads: Vec<String> = book.worksheet_range_at(0).unwrap().unwrap().rows().next().unwrap().iter().map(|c| c.to_string()).collect();
    for want in ["employee_code", "full_name *", "master_branch *", "Has PF *", "Shirt size", "Locker"] {
        assert!(heads.contains(&want.to_string()), "{want} missing from {heads:?}");
    }

    // An employee to update, made on screen.
    let (s, _, made) = call_app(&app, Method::POST, "/v1/employees", Some(admin), &[("idempotency-key", "imp-made-0001")], Some(new_employee(&w, "Kiran", &mobile(1)))).await;
    assert_eq!(s, StatusCode::CREATED, "{made}");
    let (kiran_id, kiran) = (made["id"].as_str().unwrap().parse::<Uuid>().unwrap(), made["employee_code"].as_str().unwrap().to_string());

    // B2: refused files.
    let (s, b) = upload(&app, admin, "imp-bad-00001", b"employee_code,salary\nE1,100\n").await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("wrong_template")), "{b}");
    let (s, b) = upload(&app, admin, "imp-bad-00002", &[0xff, 0xfe, 0x00, 0x01]).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("file_unreadable")), "{b}");
    let (s, _) = upload(&app, hr, "imp-bad-00003", &vec![b'a'; api::imports::MAX_BYTES + 10]).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);

    // Row 2 new, row 3 bad references, row 4 blank, row 5 update, row 6 update without a date, row 7 code in an automatic-code org.
    let csv = format!(
        "employee_code,full_name *,mobile *,joining_date,initial_status,master_branch,department,category,employee_type,payroll_group,Shirt size,Has PF *,effective_from\n\
         ,Asha {n},{m2},2026-04-01,Probation,chn,OPS,SKL,PERM,all employees,A,yes,\n\
         ,Bad {n},{m3},2026-04-01,probation,XXX,OPS,SKL,PERM,All employees,A,maybe,\n\
         ,,,,,,,,,,,,\n\
         {kiran},,,,,,HR,,,,B,,2026-05-01\n\
         {kiran},,,,,,OPS,,,,,,\n\
         ZZZ{n},Zed,{m7},2026-04-01,probation,CHN,OPS,SKL,PERM,All employees,,,\n",
        m2 = mobile(2), m3 = mobile(3), m7 = mobile(7)
    );
    let (s, b) = upload(&app, admin, "imp-first-001", csv.as_bytes()).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{b}");
    assert_eq!((b["state"].as_str(), b["duplicate_of"].clone()), (Some("uploaded"), Value::Null));
    let first = b["import_id"].as_str().unwrap().to_string();
    let (s, b) = upload(&app, admin, "imp-first-001", csv.as_bytes()).await; // same key: the same answer
    assert_eq!((s, b["import_id"].as_str()), (StatusCode::ACCEPTED, Some(first.as_str())));

    let done = finished(&app, admin, &first).await;
    assert_eq!((done["state"].clone(), done["total_rows"].clone(), done["applied_count"].clone(), done["error_count"].clone()),
               (json!("completed"), json!(5), json!(2), json!(3)), "{done}");
    // Row 5 applied: Kiran's department from 1 May, and the custom field.
    let dept: Uuid = sqlx::query_scalar("SELECT department_id FROM app.employee_job_versions WHERE employee_id = $1 AND state = 'active' AND valid @> current_date")
        .bind(kiran_id).fetch_one(&w.pool).await.unwrap();
    assert_eq!(dept, w.hr_dept);
    let shirt: String = sqlx::query_scalar("SELECT value_text FROM app.custom_field_values WHERE employee_id = $1 AND field_id = $2")
        .bind(kiran_id).bind(w.cf_choice).fetch_one(&w.pool).await.unwrap();
    assert_eq!(shirt, "B");

    // B4: the error report, through its short-lived link.
    let (s, _, b) = call_app(&app, Method::GET, &format!("/v1/employee-imports/{first}/errors"), Some(admin), &[], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, _, bytes) = call_raw(&app, Method::GET, b["url"].as_str().unwrap(), None, &[], vec![]).await;
    assert_eq!(s, StatusCode::OK);
    let mut book: calamine::Xlsx<_> = calamine::open_workbook_from_rs(std::io::Cursor::new(bytes)).unwrap();
    let report: Vec<Vec<String>> = book.worksheet_range_at(0).unwrap().unwrap().rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect();
    let has = |row: &str, field: &str, code: &str| report.iter().any(|r| r[0] == row && r[2] == field && r[3] == code);
    assert!(has("3", "master_branch", "not_found") && has("3", "Has PF", "invalid_value"), "{report:?}");
    assert!(has("6", "effective_from", "required"), "{report:?}");
    assert!(has("7", "employee_code", "invalid_value"), "{report:?}");
    let (s, _, _) = call_raw(&app, Method::GET, "/v1/files/import-errors/not-a-token", None, &[], vec![]).await;
    assert_eq!(s, StatusCode::GONE);

    // Same file again (AC-EM-15.5): recognised, applied rows skipped, no second Asha.
    let (s, b) = upload(&app, admin, "imp-second-01", csv.as_bytes()).await;
    assert_eq!((s, b["duplicate_of"].as_str()), (StatusCode::ACCEPTED, Some(first.as_str())), "{b}");
    let again = finished(&app, admin, b["import_id"].as_str().unwrap()).await;
    assert_eq!((again["applied_count"].clone(), again["error_count"].clone(), again["skipped_count"].clone()), (json!(0), json!(3), json!(2)), "{again}");
    let ashas: i64 = sqlx::query_scalar("SELECT count(*) FROM app.employees WHERE org_id = $1 AND full_name = $2")
        .bind(w.org).bind(format!("Asha {n}")).fetch_one(&w.pool).await.unwrap();
    assert_eq!(ashas, 1);

    // J5 on a finished import changes nothing.
    let (s, _, b) = call_app(&app, Method::POST, "/internal/jobs/bulk-import", None, &[("authorization", &format!("Bearer {jobs_token}"))],
        Some(json!({ "org_id": w.org, "import_id": first }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("completed")), "{b}");

    // HR limited to Bengaluru: a Chennai employee reads as unknown (edge case 6).
    let (s, b) = upload(&app, hr, "imp-hr-000001", format!("employee_code,full_name\n{kiran},Kiran Kumar\n").as_bytes()).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{b}");
    let hr_run = finished(&app, hr, b["import_id"].as_str().unwrap()).await;
    assert_eq!(hr_run["error_count"], 1, "{hr_run}");
    let code: String = sqlx::query_scalar("SELECT error_message FROM app.bulk_import_rows WHERE import_id = $1::uuid")
        .bind(b["import_id"].as_str().unwrap()).fetch_one(&w.pool).await.unwrap();
    assert!(code.contains("not_found"), "{code}");

    // B5 and the audit trail: each upload, and each applied row as the uploader.
    let (s, _, b) = call_app(&app, Method::GET, "/v1/employee-imports", Some(admin), &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(3)), "{b}");
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND action = 'employee.bulk_import'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(audits, 3);
    let created: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log a JOIN app.employees e ON e.id = a.target_id
                                           WHERE a.org_id = $1 AND a.action = 'employee.create' AND e.full_name = $2 AND a.actor_kind = 'user'")
        .bind(w.org).bind(format!("Asha {n}")).fetch_one(&w.pool).await.unwrap();
    assert_eq!(created, 1);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn probation_reminders_and_cleanup_end_to_end() {
    let w = world().await;
    let jobs_token = "jobs-token-jobs-token-jobs-token-07";
    let bearer = format!("Bearer {jobs_token}");
    let auth = [("authorization", bearer.as_str())];
    let dir = std::env::temp_dir().join(format!("payroll-files-{}", Uuid::new_v4().simple()));
    let mailbox = std::sync::Arc::new(std::sync::Mutex::new(Vec::<api::mail::Email>::new()));
    let app = api::router(
        w.state.clone().with_jobs_token(jobs_token).with_files_dir(&dir).with_mailer(api::mail::Mailer::Memory(mailbox.clone()), "https://app.test"),
    );
    let admin = Some(w.admin.as_str());
    let today = api::employees::today();
    let day = |n: i64| (today + chrono::Duration::days(n)).to_string();
    let n = Uuid::new_v4().as_u128() % 10_000_000;
    let org_body = json!({ "org_id": w.org });
    let email_of = |prefix: &'static str| {
        let pool = w.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT email FROM app.users WHERE org_id = $1 AND email LIKE $2")
                .bind(w.org).bind(format!("{prefix}-%")).fetch_one(&pool).await.unwrap()
        }
    };
    let (admin_email, hr_email) = (email_of("admin").await, email_of("hr").await);
    let sent_to = |to: &str| mailbox.lock().unwrap().iter().filter(|e| e.to == to).cloned().collect::<Vec<_>>();
    let drain = |app: axum::Router| async move {
        for _ in 0..30 {
            let (s, _, b) = call_app(&app, Method::POST, "/internal/outbox/dispatch", None, &auth, None).await;
            assert_eq!(s, StatusCode::OK, "{b}");
            if b["dispatched"] == 0 && b["failed"] == 0 {
                return;
            }
        }
        panic!("outbox never drained");
    };

    // Probation ending in 10 days (Chennai and Bengaluru), and one in 40 days (not yet due; EM-04 = 15).
    let mut ids = Vec::new();
    for (i, (name, branch, end)) in [("Chen", w.chn, 10), ("Bela", w.blr, 10), ("Later", w.chn, 40)].into_iter().enumerate() {
        let mut body = new_employee(&w, &format!("{name} {n}"), &format!("+9195{:08}", n * 10 + i as u128));
        body["joining_date"] = json!(day(-30));
        body["probation_end_date"] = json!(day(end));
        body["job"]["master_branch_id"] = json!(branch);
        let (s, _, b) = call_app(&app, Method::POST, "/v1/employees", admin, &[("idempotency-key", &format!("j3-emp-{i}-{n}"))], Some(body)).await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
        ids.push((b["id"].as_str().unwrap().to_string(), b["version"].clone()));
    }

    let (s, _, _) = call_app(&app, Method::POST, "/internal/jobs/probation-reminders", None, &[], Some(org_body.clone())).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = call_app(&app, Method::POST, "/internal/jobs/probation-reminders", None, &auth, Some(org_body.clone())).await;
    assert_eq!((s, b["reminded"].clone()), (StatusCode::OK, json!(2)), "{b}");
    let (_, _, b) = call_app(&app, Method::POST, "/internal/jobs/probation-reminders", None, &auth, Some(org_body.clone())).await;
    assert_eq!(b["reminded"], 0, "a second run the same day sends nothing");

    // Each HR user hears only about employees in their scope.
    drain(app.clone()).await;
    let to_admin = sent_to(&admin_email);
    assert_eq!(to_admin.len(), 1);
    assert!(to_admin[0].html.contains(&format!("Chen {n}")) && to_admin[0].html.contains(&format!("Bela {n}")) && !to_admin[0].html.contains("Later"), "{}", to_admin[0].html);
    let to_hr = sent_to(&hr_email);
    assert_eq!(to_hr.len(), 1);
    assert!(to_hr[0].html.contains(&format!("Bela {n}")) && !to_hr[0].html.contains(&format!("Chen {n}")), "{}", to_hr[0].html);

    // Extending probation moves the reminder (AC-EM-12.4).
    let (id, version) = &ids[0];
    let (s, _, b) = call_app(&app, Method::PATCH, &format!("/v1/employees/{id}/probation"), admin, &[("if-match", &format!("\"{version}\""))],
        Some(json!({ "probation_end_date": day(12), "reason": "More time" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, _, b) = call_app(&app, Method::POST, "/internal/jobs/probation-reminders", None, &auth, Some(org_body)).await;
    assert_eq!(b["reminded"], 1, "{b}");
    drain(app.clone()).await;
    assert_eq!(sent_to(&admin_email).len(), 2);

    // J7: an idempotency key over 24 h old, and an import finished 31 days ago with its upload on disk.
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM app.users WHERE org_id = $1 AND email = $2").bind(w.org).bind(&admin_email).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.idempotency_keys (org_id, user_id, idempotency_key, request_hash, created_at) VALUES ($1, $2, 'old-key-0001', 'x', now() - interval '2 days')")
        .bind(w.org).bind(admin_id).execute(&w.pool).await.unwrap();
    let import = Uuid::new_v4();
    let file_ref = format!("{}/imports/old/{import}.csv", w.org);
    std::fs::create_dir_all(dir.join(&file_ref).parent().unwrap()).unwrap();
    std::fs::write(dir.join(&file_ref), "employee_code\n").unwrap();
    sqlx::query("INSERT INTO app.bulk_imports (id, org_id, file_ref, file_checksum, uploaded_by, state, total_rows, completed_at)
                 VALUES ($1, $2, $3, repeat('a', 64), $4, 'completed', 0, now() - interval '31 days')")
        .bind(import).bind(w.org).bind(&file_ref).bind(admin_id).execute(&w.pool).await.unwrap();

    let (s, _, b) = call_app(&app, Method::POST, "/internal/jobs/cleanup", None, &auth, None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b["idempotency_keys_deleted"].as_u64().unwrap() >= 1 && b["files_deleted"].as_u64().unwrap() >= 1, "{b}");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM app.idempotency_keys WHERE org_id = $1 AND idempotency_key = 'old-key-0001'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(left, 0);
    assert!(!dir.join(&file_ref).exists());
    let (s, _, b) = call_app(&app, Method::GET, &format!("/v1/employee-imports/{import}/errors"), admin, &[], None).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::GONE, Some("link_expired")), "{b}");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn self_service_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let n = Uuid::new_v4().as_u128() % 10_000_000;
    let mobile = |i: u128| format!("+9194{:08}", n * 10 + i);
    let mut people = Vec::new();
    for (i, name) in ["Ravi", "Sita"].into_iter().enumerate() {
        let (s, _, b) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", &format!("me-emp-{i}-{n}"))],
            Some(new_employee(&w, &format!("{name} {n}"), &mobile(i as u128)))).await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
        people.push(b["id"].as_str().unwrap().to_string());
    }
    let (ravi, sita) = (&people[0], &people[1]);

    // Ravi gets portal access (E13) and has set his PIN; sign him in.
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{ravi}/portal-access"), admin, &[("idempotency-key", &format!("me-grant-{n}"))], Some(json!({ "send_email": false }))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let user: Uuid = sqlx::query_scalar("SELECT id FROM app.users WHERE employee_id = $1::uuid").bind(ravi).fetch_one(&w.pool).await.unwrap();
    sqlx::query("UPDATE app.user_credentials SET state = 'active' WHERE user_id = $1").bind(user).execute(&w.pool).await.unwrap();
    let token = issue_access_token(SECRET, user, w.org, session(&w.pool, w.org, user).await, "employee");
    let me = Some(token.as_str());

    // M1: own record, five sections only, version in the ETag.
    let (s, h, b) = call(&w, Method::GET, "/v1/me/profile", me, &[], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let mut keys: Vec<&String> = b.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["custom_fields", "employment", "job", "personal", "status"]);
    assert_eq!(b["personal"]["full_name"], format!("Ravi {n}"));
    let etag = h["etag"].to_str().unwrap().to_string();
    // AC-EM-16.2: nobody else's record, and no HR edit route.
    let (s, _, _) = call(&w, Method::GET, &format!("/v1/employees/{sita}"), me, &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = call(&w, Method::PATCH, &format!("/v1/employees/{ravi}/personal"), me, &[("if-match", &etag)], Some(json!({ "address": "x" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Admins have no employee record of their own.
    let (s, _, _) = call(&w, Method::GET, "/v1/me/profile", admin, &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // M2 refusals: no version, non-contact field, another current employee's mobile (AC-EM-17.4).
    let (s, _, _) = call(&w, Method::PATCH, "/v1/me/contact", me, &[], Some(json!({ "address": "1 Main St" }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
    let (s, _, b) = call(&w, Method::PATCH, "/v1/me/contact", me, &[("if-match", &etag)], Some(json!({ "full_name": "Someone" }))).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_field")), "{b}");
    let (s, _, b) = call(&w, Method::PATCH, "/v1/me/contact", me, &[("if-match", &etag)], Some(json!({ "mobile": mobile(1) }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("mobile_in_use")), "{b}");

    // M2 applies at once (AC-EM-17.1) and is audited as the employee's own edit (AC-EM-17.2).
    let (s, h, b) = call(&w, Method::PATCH, "/v1/me/contact", me, &[("if-match", &etag)],
        Some(json!({ "email": "ravi@example.test", "address": "1 Main St", "mobile": mobile(5) }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!((b["personal"]["email"].as_str(), b["personal"]["mobile"].as_str()), (Some("ravi@example.test"), Some(mobile(5).as_str())));
    assert_ne!(h["etag"].to_str().unwrap(), etag);
    let (s, _, _) = call(&w, Method::PATCH, "/v1/me/contact", me, &[("if-match", &etag)], Some(json!({ "address": "2 Main St" }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM app.audit_log WHERE org_id = $1 AND action = 'employee.profile.edit_own' AND actor_id = $2 AND target_id = $3::uuid")
        .bind(w.org).bind(user).bind(ravi).fetch_one(&w.pool).await.unwrap();
    assert_eq!(audited, 1);

    // EM-03 off: read-only (AC-EM-17.3).
    sqlx::query("INSERT INTO app.org_settings_versions (org_id, settings_group, valid_from, setting_values) VALUES ($1, 'EM', '2000-01-01', '{\"self_edit_contact\": false}')")
        .bind(w.org).execute(&w.pool).await.unwrap();
    let (_, h, _) = call(&w, Method::GET, "/v1/me/profile", me, &[], None).await;
    let (s, _, b) = call(&w, Method::PATCH, "/v1/me/contact", me, &[("if-match", h["etag"].to_str().unwrap())], Some(json!({ "address": "3 Main St" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("self_edit_disabled")), "{b}");
}

async fn read_xlsx(bytes: Vec<u8>) -> Vec<Vec<String>> {
    use calamine::Reader;
    let mut book: calamine::Xlsx<_> = calamine::open_workbook_from_rs(std::io::Cursor::new(bytes)).unwrap();
    book.worksheet_range_at(0).unwrap().unwrap().rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn reports_exports_audit_end_to_end() {
    let w = world().await;
    let dir = std::env::temp_dir().join(format!("payroll-files-{}", Uuid::new_v4().simple()));
    let jobs_token = "jobs-token-jobs-token-jobs-token-06";
    let app = api::router(w.state.clone().with_files_dir(&dir).with_jobs_token(jobs_token));
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let get = |uri: String, token: Option<&'static str>| {
        let app = app.clone();
        async move { call_app(&app, Method::GET, &uri, token, &[], None).await }
    };
    let admin_s: &'static str = Box::leak(w.admin.clone().into_boxed_str());
    let hr_s: &'static str = Box::leak(w.hr_blr.clone().into_boxed_str());
    let n = Uuid::new_v4().as_u128() % 10_000_000;

    // Asha (Chennai, OPS), Bala (Chennai, OPS, left yesterday), Chitra (Bengaluru, HR).
    let mut ids = Vec::new();
    for (i, (name, branch, dept)) in [("Asha", w.chn, w.dept), ("Bala", w.chn, w.dept), ("Chitra", w.blr, w.hr_dept)].into_iter().enumerate() {
        let mut body = new_employee(&w, &format!("{name} {n}"), &format!("+9193{:08}", n * 10 + i as u128));
        body["job"]["master_branch_id"] = json!(branch);
        body["job"]["department_id"] = json!(dept);
        let (s, _, b) = call_app(&app, Method::POST, "/v1/employees", admin, &[("idempotency-key", &format!("rep-emp-{i}-{n}"))], Some(body)).await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
        ids.push(b["id"].as_str().unwrap().parse::<Uuid>().unwrap());
    }
    let today = api::employees::today();
    let period: Uuid = sqlx::query_scalar("UPDATE app.employment_periods SET state = 'closed', last_working_day = $2, exit_reason_id = $3 WHERE employee_id = $1 RETURNING id")
        .bind(ids[1]).bind(today - chrono::Duration::days(1)).bind(w.resigned).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.employee_status_changes (org_id, employee_id, employment_period_id, status, effective_from) VALUES ($1, $2, $3, 'exited', $4)")
        .bind(w.org).bind(ids[1]).bind(period).bind(today).execute(&w.pool).await.unwrap();

    // P1 R10: headcount today leaves out the leaver; HR sees Bengaluru only; nobody before joining.
    let (s, _, b) = get("/v1/reports/r10?group_by=branch".into(), Some(admin_s)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let count = |b: &Value, code: &str| b["rows"].as_array().unwrap().iter().find(|r| r["group"]["code"] == code).map(|r| r["count"].clone());
    assert_eq!((count(&b, "CHN"), count(&b, "BLR"), b["total"].clone()), (Some(json!(1)), Some(json!(1)), json!(2)), "{b}");
    let (_, _, b) = get("/v1/reports/r10?group_by=department".into(), Some(admin_s)).await;
    assert_eq!((count(&b, "OPS"), count(&b, "HR")), (Some(json!(1)), Some(json!(1))), "{b}");
    let (_, _, b) = get("/v1/reports/r10".into(), Some(hr_s)).await;
    assert_eq!((count(&b, "BLR"), b["total"].clone()), (Some(json!(1)), json!(1)), "{b}");
    let (_, _, b) = get("/v1/reports/r10?as_of=2026-03-01".into(), Some(admin_s)).await;
    assert_eq!(b["total"], 0, "{b}");
    let (s, _, _) = get("/v1/reports/r10?group_by=colour".into(), Some(admin_s)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // P2 R11: three joiners and one leaver with the reason; HR only Bengaluru.
    let r11 = format!("/v1/reports/r11?from=2026-04-01&to={today}");
    let (s, _, b) = get(r11.clone(), Some(admin_s)).await;
    assert_eq!((s, b["joiners"].as_array().map(Vec::len), b["leavers"].as_array().map(Vec::len)), (StatusCode::OK, Some(3), Some(1)), "{b}");
    assert_eq!(b["leavers"][0]["exit_reason"]["code"], "RESIGNED");
    let (_, _, b) = get(r11, Some(hr_s)).await;
    assert_eq!((b["joiners"].as_array().map(Vec::len), b["leavers"].as_array().map(Vec::len)), (Some(1), Some(0)), "{b}");
    let (s, _, _) = get("/v1/reports/r11?to=2026-05-01".into(), Some(admin_s)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // P3: small exports come back at once with a 10-minute link.
    let (s, _, b) = call_app(&app, Method::POST, "/v1/exports", admin, &[("idempotency-key", &format!("exp-emp-{n}"))],
        Some(json!({ "report_code": "employees", "parameters": {} }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("ready")), "{b}");
    let admin_export = b["export_id"].as_str().unwrap().to_string();
    let (s, _, bytes) = call_raw(&app, Method::GET, b["download_url"].as_str().unwrap(), None, &[], vec![]).await;
    assert_eq!(s, StatusCode::OK);
    let sheet = read_xlsx(bytes).await;
    assert_eq!((sheet[0][0].as_str(), sheet.len()), ("employee_code", 4), "{sheet:?}");
    let (s, _, b) = call_app(&app, Method::POST, "/v1/exports", hr, &[("idempotency-key", &format!("exp-r10-{n}"))],
        Some(json!({ "report_code": "r10", "parameters": { "group_by": "department" } }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, _, bytes) = call_raw(&app, Method::GET, b["download_url"].as_str().unwrap(), None, &[], vec![]).await;
    let sheet = read_xlsx(bytes).await;
    assert_eq!(sheet.last().unwrap()[1..], ["Total", "1"], "{sheet:?}");
    let (s, _, b) = call_app(&app, Method::POST, "/v1/exports", admin, &[("idempotency-key", &format!("exp-bad-{n}"))],
        Some(json!({ "report_code": "payslips" }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    let (s, _, _) = call_raw(&app, Method::GET, "/v1/files/exports/not-a-token", None, &[], vec![]).await;
    assert_eq!(s, StatusCode::GONE);

    // P4: only the requester sees an export.
    let (s, _, b) = get(format!("/v1/exports/{admin_export}"), Some(admin_s)).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("ready")), "{b}");
    let (s, _, _) = get(format!("/v1/exports/{admin_export}"), Some(hr_s)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // J6: a queued (large) export is built as its requester; a second run does nothing.
    let admin_id: Uuid = sqlx::query_scalar("SELECT requested_by FROM app.export_jobs WHERE id = $1::uuid").bind(&admin_export).fetch_one(&w.pool).await.unwrap();
    let queued: Uuid = sqlx::query_scalar("INSERT INTO app.export_jobs (org_id, requested_by, report_code, parameters) VALUES ($1, $2, 'r11', $3) RETURNING id")
        .bind(w.org).bind(admin_id).bind(json!({ "from": "2026-04-01", "to": today.to_string() })).fetch_one(&w.pool).await.unwrap();
    let auth = format!("Bearer {jobs_token}");
    let (s, _, b) = call_app(&app, Method::POST, "/internal/jobs/export", None, &[("authorization", &auth)], Some(json!({ "org_id": w.org, "export_id": queued }))).await;
    assert_eq!((s, b["state"].as_str(), b["rows"].clone()), (StatusCode::OK, Some("ready"), json!(4)), "{b}");
    let (_, _, b) = call_app(&app, Method::POST, "/internal/jobs/export", None, &[("authorization", &auth)], Some(json!({ "org_id": w.org, "export_id": queued }))).await;
    assert!(b["note"].is_string(), "{b}");
    let (_, _, b) = get(format!("/v1/exports/{queued}"), Some(admin_s)).await;
    assert!(b["download_url"].is_string(), "{b}");

    // P5: audit entries, scoped for employee targets, paged newest first.
    let (s, _, b) = get(format!("/v1/audit?target_id={}", ids[0]), Some(admin_s)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b["items"].as_array().unwrap().iter().any(|e| e["action"] == "employee.create"), "{b}");
    let (_, _, b) = get(format!("/v1/audit?target_id={}", ids[0]), Some(hr_s)).await;
    assert_eq!(b["items"].as_array().map(Vec::len), Some(0), "Chennai employee is outside HR's scope: {b}");
    let (_, _, b) = get(format!("/v1/audit?target_id={}", ids[2]), Some(hr_s)).await;
    assert!(!b["items"].as_array().unwrap().is_empty(), "{b}");
    let (_, _, b) = get("/v1/audit?action=report.export".into(), Some(admin_s)).await;
    assert_eq!(b["items"].as_array().map(Vec::len), Some(2), "{b}");
    let (_, _, p1) = get("/v1/audit?limit=1".into(), Some(admin_s)).await;
    let next = p1["next_before"].as_i64().expect("more pages");
    let (_, _, p2) = get(format!("/v1/audit?limit=1&before={next}"), Some(admin_s)).await;
    assert!(p2["items"][0]["id"].as_i64().unwrap() < p1["items"][0]["id"].as_i64().unwrap());
    let (s, _, _) = get("/v1/audit?colour=red".into(), Some(admin_s)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn sign_in_rate_limits() {
    let w = world().await;
    let from = |ip: String| [("x-forwarded-for", format!("203.0.113.9, {ip}"))];
    let send = |uri: &'static str, ip: String, body: Value| {
        let app = w.app.clone();
        async move {
            let h = from(ip);
            call_app(&app, Method::POST, uri, None, &[(h[0].0, h[0].1.as_str())], Some(body)).await
        }
    };
    let pin = |code: String| json!({ "org_short_code": w.org_code, "employee_code": code, "pin": "000000", "device": { "id": "d", "platform": "web" } });

    // Per IP: 30 a minute on one route; other IPs unaffected. Bad bodies keep each call quick (no database),
    // since the IP is counted before the body is read.
    for i in 0..30 {
        let (s, _, b) = send("/v1/auth/employee/sign-in", "10.0.0.1".into(), json!({})).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "attempt {i}: {b}");
    }
    let (s, h, b) = send("/v1/auth/employee/sign-in", "10.0.0.1".into(), pin("NOPE99".into())).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited")));
    let retry: u64 = h["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry) && b["meta"]["retry_after"] == retry, "{retry} {b}");
    let (s, _, _) = send("/v1/auth/employee/sign-in", "10.0.0.2".into(), pin("NOPE99".into())).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Per account: 20 per 15 minutes whatever the IP; the code is matched case-insensitively.
    for i in 0..20 {
        let (s, _, _) = send("/v1/auth/employee/sign-in", format!("10.1.0.{i}"), pin(if i % 2 == 0 { "acct1".into() } else { "ACCT1".into() })).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    let (s, _, _) = send("/v1/auth/employee/sign-in", "10.1.1.1".into(), pin("ACCT1".into())).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    // One-time codes: 3 per 10 minutes per account.
    let otp = json!({ "org_short_code": w.org_code, "employee_code": "OTP1" });
    for i in 0..3 {
        let (s, _, b) = send("/v1/auth/employee/otp/request", format!("10.2.0.{i}"), otp.clone()).await;
        assert!(s.is_success(), "attempt {i}: {s} {b}");
    }
    let (s, _, _) = send("/v1/auth/employee/otp/request", "10.2.1.1".into(), otp).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    // Vendor sign-in: per email, whatever its case.
    let vendor = json!({ "email": "nobody@example.test", "password": "wrong-password-123" });
    for i in 0..20 {
        let (s, _, _) = send("/v1/vendor/auth/sign-in", format!("10.3.0.{i}"), vendor.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    let (s, _, _) = send("/v1/vendor/auth/sign-in", "10.3.1.1".into(), json!({ "email": "NOBODY@example.test", "password": "x" })).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

/// J1 walks every active organisation in the database, so it would remind or apply status changes in
/// other tests' organisations while they run: run it on its own (`-- --ignored fan_out --exact`).
#[tokio::test]
#[ignore = "runs jobs for every organisation in DATABASE_URL; run alone"]
async fn fan_out() {
    let w = world().await;
    let jobs_token = "jobs-token-jobs-token-jobs-token-j1";
    let app = api::router(w.state.clone().with_jobs_token(jobs_token));
    let bearer = format!("Bearer {jobs_token}");
    let n = Uuid::new_v4().as_u128() % 10_000_000;
    let today = api::employees::today();
    let mut body = new_employee(&w, &format!("Due {n}"), &format!("+9196{:08}", n));
    body["joining_date"] = json!((today - chrono::Duration::days(30)).to_string());
    body["probation_end_date"] = json!((today + chrono::Duration::days(10)).to_string());
    let (s, _, b) = call_app(&app, Method::POST, "/v1/employees", Some(&w.admin), &[("idempotency-key", &format!("j1-{n}"))], Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let id: Uuid = b["id"].as_str().unwrap().parse().unwrap();

    let (s, _, _) = call_app(&app, Method::POST, "/internal/jobs/fan-out", None, &[], None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, b) = call_app(&app, Method::POST, "/internal/jobs/fan-out", None, &[("authorization", &bearer)], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b["organisations"].as_u64().unwrap() >= 1 && b["reminded"].as_u64().unwrap() >= 1, "{b}");
    // Other organisations on a shared database may hold requests that can no longer apply; this one has none.
    assert!(!b["failed"].as_array().unwrap().iter().any(|f| f["org_id"] == json!(w.org)), "{b}");
    let sent: Option<chrono::NaiveDate> = sqlx::query_scalar("SELECT probation_reminder_sent_for FROM app.employment_periods WHERE employee_id = $1")
        .bind(id).fetch_one(&w.pool).await.unwrap();
    assert!(sent.is_some(), "this organisation's employee was reminded");
    let (_, _, b) = call_app(&app, Method::POST, "/internal/jobs/fan-out", None, &[("authorization", &bearer)], None).await;
    assert_eq!(b["reminded"], 0, "a second tick the same day reminds nobody: {b}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn locked_period_refuses_dated_changes() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let (_, _, meena) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-lk-meena1")], Some(new_employee(&w, "Meena", "+919800000061"))).await;
    let id = meena["id"].as_str().unwrap().to_string();
    let ravi = hire_at_blr(&w, "Ravi", "+919800000062", "key-lk-ravi01").await;

    // October locked for Chennai only (lock unit = payroll group × branch × period, DEC-49). Period routes come in S9.
    let unit: Uuid = sqlx::query_scalar(
        "WITH p AS (INSERT INTO app.attendance_periods (org_id, payroll_group_id, period)
                    VALUES ($1, $2, '[2026-10-01,2026-11-01)') RETURNING id)
         INSERT INTO app.lock_units (org_id, period_id, branch_id, state) SELECT $1, id, $3, 'locked' FROM p RETURNING id",
    )
    .bind(w.org).bind(w.group).bind(w.chn).fetch_one(&w.pool).await.unwrap();

    // E6 into the locked month is refused and names the unit; after it, allowed.
    let change = |d: &str| json!({ "effective_from": d, "changes": { "department_id": w.hr_dept }, "reason": "Moves to HR" });
    let uri = format!("/v1/employees/{id}/job-changes");
    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"1\"")], Some(change("2026-10-15"))).await;
    assert_eq!((s, b["code"].as_str(), b["meta"]["state"].as_str()), (StatusCode::CONFLICT, Some("date_in_locked_period"), Some("locked")), "{b}");
    assert_eq!(b["meta"]["lock_unit_id"], json!(unit));
    // Bengaluru's unit for October is still open.
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{ravi}/job-changes"), admin, &[("if-match", "\"1\"")], Some(change("2026-10-15"))).await;
    assert_eq!(s, StatusCode::OK, "{b}");

    // S1 dated in the locked month is refused.
    let confirm = json!({ "to_status": "active", "effective_date": "2026-10-10", "reason": "Confirmed" });
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{id}/status-requests"), admin, &[("idempotency-key", "key-lk-conf01")], Some(confirm)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("date_in_locked_period")), "{b}");

    // O15 skips the locked employee and says why.
    let assign = json!({ "effective_from": "2026-10-20", "employee_ids": [id], "reason": "Regroup" });
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/org/payroll-groups/{}/assign", w.group), admin, &[("idempotency-key", "key-lk-asg001")], Some(assign)).await;
    assert_eq!((s, b["changed"].as_i64(), b["skipped"][0]["code"].as_str()), (StatusCode::OK, Some(0), Some("date_in_locked_period")), "{b}");

    let (s, _, b) = call(&w, Method::POST, &uri, admin, &[("if-match", "\"1\"")], Some(change("2026-11-05"))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}
