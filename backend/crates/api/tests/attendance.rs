//! End-to-end checks of the attendance routes (Package 2) against a real database (DATABASE_URL from backend/.env).
//! Ignored by default because they write to the database; each test creates its own organisation.
//!
//!   cargo test -p api --test attendance -- --ignored

use api::ctx::issue_access_token;
use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

mod common;
use common::*;

async fn shift(w: &World, key: &str, body: Value) -> String {
    let (s, _, b) = call(w, Method::POST, "/v1/attendance/shifts", Some(&w.admin), &[("idempotency-key", key)], Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    b["id"].as_str().unwrap().to_string()
}

async fn roster(w: &World, token: &str, key: &str, entries: Value) -> Value {
    let (s, _, b) = call(w, Method::PUT, "/v1/attendance/roster", Some(token), &[("idempotency-key", key)], Some(json!({ "entries": entries }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b
}

async fn day(w: &World, emp: &str, date: &str) -> Value {
    let uri = format!("/v1/attendance/roster?from={date}&to={date}&employee_id={emp}");
    let (s, _, b) = call(w, Method::GET, &uri, Some(&w.admin), &[], None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b["rows"][0]["entries"][date].clone()
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn shifts_and_roster_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());

    // SH2: defaults filled (AC-AT-01.2); night flag required when the shift crosses midnight (DEC-23); codes unique.
    let gen = shift(&w, "key-at-sh-gen1", json!({ "code": "GEN", "name": "General", "valid_from": "2026-01-01" })).await;
    let (_, _, list) = call(&w, Method::GET, "/v1/attendance/shifts", admin, &[], None).await;
    let g = &list["items"][0];
    assert_eq!((g["current"]["start_time"].as_str(), g["current"]["required_minutes"].as_i64(), g["current"]["state"].as_str()), (Some("09:00:00"), Some(480), Some("active")), "{list}");
    let night = json!({ "code": "NGT", "name": "Night", "valid_from": "2026-01-01", "start_time": "22:00", "end_time": "06:00" });
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/shifts", admin, &[("idempotency-key", "key-at-sh-ngt1")], Some(night.clone())).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("night_flag_required")), "{b}");
    let mut night = night;
    night["is_night"] = json!(true);
    shift(&w, "key-at-sh-ngt2", night).await;
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/shifts", admin, &[("idempotency-key", "key-at-sh-dup1")], Some(json!({ "code": "gen", "name": "Again", "valid_from": "2026-01-01" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("code_in_use")), "{b}");
    let mor = shift(&w, "key-at-sh-mor1", json!({ "code": "MOR", "name": "Morning", "valid_from": "2026-01-01", "start_time": "06:00", "end_time": "14:00" })).await;
    let eve = shift(&w, "key-at-sh-eve1", json!({ "code": "EVE", "name": "Evening", "valid_from": "2026-01-01", "start_time": "14:00", "end_time": "22:00" })).await;
    let blr_only = shift(&w, "key-at-sh-blr1", json!({ "code": "BLRX", "name": "Bengaluru only", "valid_from": "2026-01-01", "branch_id": w.blr })).await;

    // SH4: a later version is scheduled; it can be discarded (twice: same answer); earlier dates keep the old one (BR-05).
    let vuri = format!("/v1/attendance/shifts/{gen}/versions");
    let (s, _, v) = call(&w, Method::POST, &vuri, admin, &[("idempotency-key", "key-at-sv-0001")], Some(json!({ "valid_from": "2027-01-01", "start_time": "10:00", "end_time": "19:00", "reason": "New hours" }))).await;
    assert_eq!((s, v["state"].as_str(), v["start_time"].as_str()), (StatusCode::CREATED, Some("scheduled"), Some("10:00:00")), "{v}");
    let (_, _, vs) = call(&w, Method::GET, &vuri, admin, &[], None).await;
    let states: Vec<&str> = vs["items"].as_array().unwrap().iter().map(|x| x["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["scheduled", "active"]);
    let discard = format!("{vuri}/{}/discard", v["id"].as_str().unwrap());
    for _ in 0..2 {
        let (s, _, b) = call(&w, Method::POST, &discard, admin, &[], Some(json!({ "reason": "Not needed" }))).await;
        assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("discarded")), "{b}");
    }

    // SH3: name changes with If-Match; the code is not editable.
    let suri = format!("/v1/attendance/shifts/{gen}");
    let (s, _, b) = call(&w, Method::PATCH, &suri, admin, &[("if-match", "\"1\"")], Some(json!({ "code": "GN2" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_field")));
    let (s, h, b) = call(&w, Method::PATCH, &suri, admin, &[("if-match", "\"1\"")], Some(json!({ "name": "General day" }))).await;
    assert_eq!((s, h["etag"].to_str().unwrap(), b["name"].as_str()), (StatusCode::OK, "\"2\"", Some("General day")), "{b}");

    // R7: default and allowed shifts are a dated job change.
    let (_, _, meena) = call(&w, Method::POST, "/v1/employees", admin, &[("idempotency-key", "key-at-meena01")], Some(new_employee(&w, "Meena", "+919800000071"))).await;
    let id = meena["id"].as_str().unwrap().to_string();
    let r7 = format!("/v1/attendance/employees/{id}/shifts");
    let body = json!({ "effective_from": "2026-11-01", "default_shift_id": gen, "allowed_shift_ids": [eve, mor], "reason": "Assign shifts" });
    let (s, h, b) = call(&w, Method::PUT, &r7, admin, &[("if-match", "\"1\"")], Some(body)).await;
    assert_eq!((s, h["etag"].to_str().unwrap()), (StatusCode::OK, "\"2\""), "{b}");
    let (s, _, b) = call(&w, Method::PUT, &r7, admin, &[("if-match", "\"2\"")], Some(json!({ "effective_from": "2026-11-01", "default_shift_id": blr_only, "reason": "x" }))).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("shift_not_allowed_at_branch")), "{b}");

    // A later Package 1 job change keeps the shift fields (they ride along with every new version).
    let change = json!({ "effective_from": "2026-11-15", "changes": { "department_id": w.hr_dept }, "reason": "Moves to HR" });
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/employees/{id}/job-changes"), admin, &[("if-match", "\"2\"")], Some(change)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (default, allowed): (Option<Uuid>, i64) = sqlx::query_as(
        "SELECT v.default_shift_id, (SELECT count(*) FROM app.job_version_allowed_shifts a WHERE a.job_version_id = v.id)
         FROM app.employee_job_versions v WHERE v.employee_id = $1 AND v.state = 'active' AND v.valid @> DATE '2026-11-20'",
    )
    .bind(id.parse::<Uuid>().unwrap())
    .fetch_one(&w.pool)
    .await
    .unwrap();
    assert_eq!((default.map(|u| u.to_string()), allowed), (Some(gen.clone()), 2));

    // Lock October for Chennai (period routes come in S9).
    sqlx::query(
        "WITH p AS (INSERT INTO app.attendance_periods (org_id, payroll_group_id, period) VALUES ($1, $2, '[2026-10-01,2026-11-01)') RETURNING id)
         INSERT INTO app.lock_units (org_id, period_id, branch_id, state) SELECT $1, id, $3, 'locked' FROM p",
    )
    .bind(w.org).bind(w.group).bind(w.chn).execute(&w.pool).await.unwrap();
    let (s, _, b) = call(&w, Method::POST, &vuri, admin, &[("idempotency-key", "key-at-sv-0002")], Some(json!({ "valid_from": "2026-10-15" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("date_locked")), "{b}");

    // R2: valid entries apply; locked, outside employment and other-branch shifts are reported (AC-AT-02.2, 02.6).
    let entries = json!([
        { "employee_id": id, "work_date": "2026-11-16", "shift_id": mor },
        { "employee_id": id, "work_date": "2026-11-17", "is_weekly_off": true },
        { "employee_id": id, "work_date": "2026-10-10", "shift_id": mor },
        { "employee_id": id, "work_date": "2026-03-01", "shift_id": mor },
        { "employee_id": id, "work_date": "2026-11-18", "shift_id": blr_only },
    ]);
    let b = roster(&w, &w.admin, "key-at-ro-0001", entries.clone()).await;
    let codes: Vec<&str> = b["failed"].as_array().unwrap().iter().map(|f| f["code"].as_str().unwrap()).collect();
    assert_eq!((b["applied"].as_i64(), codes), (Some(2), vec!["date_locked", "outside_employment", "shift_not_allowed_at_branch"]), "{b}");
    let b = roster(&w, &w.admin, "key-at-ro-0002", entries).await;
    assert_eq!((b["applied"].as_i64(), b["unchanged"].as_i64()), (Some(0), Some(2)));
    // The Bengaluru-only HR Manager can't touch a Chennai employee.
    let b = roster(&w, &w.hr_blr, "key-at-ro-0003", json!([{ "employee_id": id, "work_date": "2026-11-19", "shift_id": mor }])).await;
    assert_eq!(b["failed"][0]["code"], "not_found");

    // R1: roster entry beats the default shift.
    assert_eq!((day(&w, &id, "2026-11-15").await["source"].as_str(), day(&w, &id, "2026-11-15").await["code"].as_str()), (Some("default"), Some("GEN")));
    assert_eq!(day(&w, &id, "2026-11-16").await["code"], "MOR");
    assert_eq!(day(&w, &id, "2026-11-17").await["is_weekly_off"], true);

    // R4: copy a range forward.
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/roster/copy", admin, &[("idempotency-key", "key-at-cp-0001")],
        Some(json!({ "from": "2026-11-16", "to": "2026-11-17", "to_start": "2026-11-23", "employee_ids": [id] }))).await;
    assert_eq!((s, b["applied"].as_i64()), (StatusCode::OK, Some(2)), "{b}");
    assert_eq!((day(&w, &id, "2026-11-23").await["code"].as_str(), day(&w, &id, "2026-11-23").await["source"].as_str()), (Some("MOR"), Some("copy")));

    // R5: two-week rotation from Monday 30 November, Sundays off (AC-AT-02.5).
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/roster/rotation", admin, &[("idempotency-key", "key-at-rt-0001")],
        Some(json!({ "employee_ids": [id], "start": "2026-11-30", "weeks": 2, "pattern": [mor, eve], "weekly_off_days": [7] }))).await;
    assert_eq!((s, b["applied"].as_i64()), (StatusCode::OK, Some(14)), "{b}");
    assert_eq!(day(&w, &id, "2026-12-06").await["is_weekly_off"], true);
    assert_eq!(day(&w, &id, "2026-12-07").await["code"], "EVE");

    // R3: clear back to the default shift.
    let (s, _, b) = call(&w, Method::DELETE, "/v1/attendance/roster/entries", admin, &[],
        Some(json!({ "employee_id": id, "from": "2026-11-30", "to": "2026-12-13", "reason": "Rotation cancelled" }))).await;
    assert_eq!((s, b["removed"].as_i64()), (StatusCode::OK, Some(14)), "{b}");
    assert_eq!(day(&w, &id, "2026-12-07").await["source"], "default");

    // Every roster change is announced for recalculation.
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE aggregate_id = $1 AND event_type = 'ScheduleChanged'")
        .bind(id.parse::<Uuid>().unwrap())
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert!(events >= 5, "{events}");
}

async fn post(w: &World, uri: &str, key: &str, body: Value) -> (StatusCode, Value) {
    let (s, _, b) = call(w, Method::POST, uri, Some(&w.admin), &[("idempotency-key", key)], Some(body)).await;
    (s, b)
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn holidays_and_optional_picks_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let cals = "/v1/attendance/holiday-calendars";

    // H2 + H4: a calendar for 2026 with dates checked against its year and one holiday per date.
    let (s, cal) = post(&w, cals, "key-hc-0001", json!({ "name": "Chennai 2026", "year": 2026 })).await;
    assert_eq!(s, StatusCode::CREATED, "{cal}");
    let cid = cal["id"].as_str().unwrap().to_string();
    let (s, b) = post(&w, cals, "key-hc-0002", json!({ "name": "chennai 2026", "year": 2026 })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("name_in_use")), "{b}");
    let huri = format!("{cals}/{cid}/holidays");
    let mut ids = Vec::new();
    for (n, (date, name, kind)) in [("2026-10-02", "Gandhi Jayanti", "national"), ("2026-12-25", "Christmas", "festival"),
                                    ("2026-12-04", "Local fair", "optional"), ("2026-12-11", "Temple day", "optional")].into_iter().enumerate() {
        let (s, b) = post(&w, &huri, &format!("key-hd-000{n}"), json!({ "date": date, "name": name, "kind": kind })).await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
        ids.push(b["id"].as_str().unwrap().to_string());
    }
    let (s, b) = post(&w, &huri, "key-hd-0105", json!({ "date": "2027-01-01", "name": "New Year", "kind": "festival" })).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("date_outside_year")), "{b}");
    let (s, b) = post(&w, &huri, "key-hd-0106", json!({ "date": "2026-12-25", "name": "Again", "kind": "festival" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("holiday_exists_on_date")), "{b}");

    // H5: October is locked for Chennai, so linking a calendar with an October holiday is refused.
    sqlx::query(
        "WITH p AS (INSERT INTO app.attendance_periods (org_id, payroll_group_id, period) VALUES ($1, $2, '[2026-10-01,2026-11-01)') RETURNING id)
         INSERT INTO app.lock_units (org_id, period_id, branch_id, state) SELECT $1, id, $3, 'locked' FROM p",
    )
    .bind(w.org).bind(w.group).bind(w.chn).execute(&w.pool).await.unwrap();
    let link = format!("/v1/attendance/branches/{}/holiday-calendars/2026", w.chn);
    let (s, _, b) = call(&w, Method::PUT, &link, admin, &[], Some(json!({ "calendar_id": cid }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("date_locked")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("{huri}/{}/remove", ids[0]), admin, &[("if-match", "\"5\"")], Some(json!({ "reason": "Entered by mistake" }))).await;
    assert_eq!((s, b["removed_at"].is_null()), (StatusCode::OK, false), "{b}");
    let (s, _, b) = call(&w, Method::PUT, &link, admin, &[], Some(json!({ "calendar_id": cid }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    // Now linked, a locked date can't be added.
    let (s, b) = post(&w, &huri, "key-hd-0107", json!({ "date": "2026-10-20", "name": "Late", "kind": "festival" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("date_locked")), "{b}");

    // H4 PATCH: a change replaces the holiday (new id); H3 hides removed ones unless asked.
    let (s, _, b) = call(&w, Method::PATCH, &format!("{huri}/{}", ids[1]), admin, &[("if-match", "\"6\"")], Some(json!({ "name": "Christmas Day" }))).await;
    assert_eq!((s, b["name"].as_str()), (StatusCode::OK, Some("Christmas Day")), "{b}");
    assert_ne!(b["id"].as_str(), Some(ids[1].as_str()));
    let curi = format!("{cals}/{cid}");
    let (_, _, c) = call(&w, Method::GET, &curi, admin, &[], None).await;
    let chn = w.chn.to_string();
    assert_eq!((c["holidays"].as_array().unwrap().len(), c["holidays_count"].as_i64(), c["branches"][0]["id"].as_str()), (3, Some(3), Some(chn.as_str())), "{c}");
    let (_, _, c) = call(&w, Method::GET, &format!("{curi}?include_removed=true"), admin, &[], None).await;
    assert_eq!(c["holidays"].as_array().unwrap().len(), 5);
    let (_, _, l) = call(&w, Method::GET, &format!("{cals}?year=2026"), admin, &[], None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);

    // H2 copy: same days in 2027.
    let (s, b) = post(&w, cals, "key-hc-0003", json!({ "name": "Chennai 2027", "year": 2027, "copy_from_calendar_id": cid })).await;
    assert_eq!((s, b["holidays_count"].as_i64()), (StatusCode::CREATED, Some(3)), "{b}");

    // R1 shows the holiday for a Chennai employee.
    let (_, meena) = post(&w, "/v1/employees", "key-hd-meena1", new_employee(&w, "Meena", "+919800000072")).await;
    let emp = meena["id"].as_str().unwrap().to_string();
    assert_eq!(day(&w, &emp, "2026-12-25").await["holiday"]["name"], "Christmas Day");
    assert!(day(&w, &emp, "2026-12-04").await["holiday"].is_null());

    // Meena signs in as an employee.
    let e: Uuid = emp.parse().unwrap();
    sqlx::query("UPDATE app.employees SET portal_access = 'employee_portal', email = 'meena@example.test' WHERE id = $1").bind(e).execute(&w.pool).await.unwrap();
    let user: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, employee_id, display_name, state) VALUES ($1, 'employee', $2, 'Meena', 'active') RETURNING id")
        .bind(w.org).bind(e).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'employee'")
        .bind(w.org).bind(user).execute(&w.pool).await.unwrap();
    let token = issue_access_token(SECRET, user, w.org, session(&w.pool, w.org, user).await, "employee");
    let me = Some(token.as_str());
    let picks = "/v1/attendance/optional-holiday-picks";
    let pick = |key: &'static str, hid: String| {
        let w = &w;
        async move { call(w, Method::POST, picks, me, &[("idempotency-key", key)], Some(json!({ "holiday_id": hid }))).await }
    };

    // H7: off while HD-01 = 0 (the default); H6 still shows the optional holidays (AC-AT-08.4).
    let (s, _, b) = pick("key-pk-0001", ids[2].clone()).await;
    assert_eq!((s, b["code"].as_str(), b["meta"]["setting"].as_str()), (StatusCode::FORBIDDEN, Some("feature_disabled"), Some("HD-01")), "{b}");
    let (_, _, mine) = call(&w, Method::GET, "/v1/attendance/me/optional-holidays?year=2026", me, &[], None).await;
    assert_eq!((mine["allowed"].as_i64(), mine["holidays"].as_array().map(Vec::len)), (Some(0), Some(2)), "{mine}");

    // HD-01 = 1 in the default policy set (policy routes come in S3).
    sqlx::query("INSERT INTO app.policy_set_versions (org_id, policy_set_id, valid_from, state, settings)
                 SELECT $1, id, DATE '2026-01-01', 'approved', '{\"optional_holidays_per_year\": 1}' FROM app.policy_sets WHERE org_id = $1 AND is_default")
        .bind(w.org).execute(&w.pool).await.unwrap();
    let (s, _, b) = pick("key-pk-0002", ids[1].clone()).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{b}"); // the replaced Christmas row is removed
    let (_, _, c) = call(&w, Method::GET, &curi, admin, &[], None).await;
    let xmas = c["holidays"].as_array().unwrap().iter().find(|h| h["kind"] == "festival").unwrap()["id"].as_str().unwrap().to_string();
    let (s, _, b) = pick("key-pk-0003", xmas).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("holiday_not_optional")), "{b}");
    let (s, _, p) = pick("key-pk-0004", ids[2].clone()).await;
    assert_eq!((s, p["state"].as_str()), (StatusCode::CREATED, Some("pending")), "{p}");
    let pid = p["id"].as_str().unwrap().to_string();
    let (s, _, b) = pick("key-pk-0005", ids[2].clone()).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("already_picked")), "{b}");
    let (s, _, b) = pick("key-pk-0006", ids[3].clone()).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("pick_limit_reached")), "{b}");
    let (_, _, mine) = call(&w, Method::GET, "/v1/attendance/me/optional-holidays", me, &[], None).await;
    assert_eq!((mine["allowed"].as_i64(), mine["used"].as_i64(), mine["holidays"][0]["pick"]["state"].as_str()), (Some(1), Some(1), Some("pending")), "{mine}");

    // H8 + H9: the employee sees their own and can't approve; the admin sees it awaiting them and approves.
    let (_, _, own) = call(&w, Method::GET, picks, me, &[], None).await;
    assert_eq!(own["items"].as_array().unwrap().len(), 1, "{own}");
    let act = |id: &str, what: &str| format!("{picks}/{id}/{what}");
    let (s, _, _) = call(&w, Method::POST, &act(&pid, "approve"), me, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, _, waiting) = call(&w, Method::GET, &format!("{picks}?awaiting_me=true"), admin, &[], None).await;
    assert_eq!(waiting["items"][0]["id"].as_str(), Some(pid.as_str()), "{waiting}");
    let (s, _, b) = call(&w, Method::POST, &act(&pid, "approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("approved")), "{b}");
    assert_eq!(day(&w, &emp, "2026-12-04").await["holiday"]["kind"], "optional");
    let (s, _, b) = call(&w, Method::POST, &act(&pid, "reject"), admin, &[("if-match", "\"2\"")], Some(json!({ "reason": "x" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_pending")), "{b}");

    // H10: cancel before the date frees the pick; a new pick can be withdrawn; a rejection needs a reason.
    let (s, _, b) = call(&w, Method::POST, &act(&pid, "cancel"), me, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("cancelled")), "{b}");
    assert!(day(&w, &emp, "2026-12-04").await["holiday"].is_null());
    let (s, _, p2) = pick("key-pk-0007", ids[3].clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{p2}");
    let p2 = p2["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &act(&p2, "reject"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("required")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act(&p2, "withdraw"), me, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("withdrawn")), "{b}");

    // Calendar and pick changes are announced for recalculation.
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND event_type = 'ScheduleChanged'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert!(events >= 4, "{events}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn policy_sets_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let sets = "/v1/attendance/policy-sets";
    let v_uri = |set: &str, rest: &str| format!("{sets}/{set}/versions{rest}");
    let step = |set: String, vid: String, what: &'static str, token: Option<&'static str>, version: &'static str, body: Value| {
        let w = &w;
        async move {
            let token = Some(if token == Some("hr") { w.hr_blr.as_str() } else { w.admin.as_str() });
            call(w, Method::POST, &format!("{sets}/{set}/versions/{vid}/{what}"), token, &[("if-match", version)], Some(body)).await
        }
    };

    // PS5 + PS1: the schema carries BRD defaults; every organisation starts with the default set (AC-AT-21.1).
    let (_, _, schema) = call(&w, Method::GET, "/v1/attendance/policy-settings/schema", admin, &[], None).await;
    assert_eq!(schema["properties"]["late_grace_minutes"]["default"], 10, "{schema}");
    let (_, _, list) = call(&w, Method::GET, sets, admin, &[], None).await;
    assert_eq!((list["items"][0]["is_default"].as_bool(), list["items"][0]["active_version"]["valid_from"].as_str()), (Some(true), Some("2000-01-01")), "{list}");
    let default_id = list["items"][0]["id"].as_str().unwrap().to_string();

    // PS2: values are checked together (AC-AT-21.5): grace 15 needs the first slab at 16.
    let (s, b) = post(&w, sets, "key-ps-0001", json!({ "name": "Night staff", "valid_from": "2026-12-01", "values": { "late_grace_minutes": 15 } })).await;
    assert_eq!((s, b["errors"][0]["field"].as_str(), b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("values.late_slabs"), Some("invalid_setting")), "{b}");
    let values = json!({ "late_grace_minutes": 15, "late_slabs": [{ "from": 16, "to": null, "action": "mark" }] });
    let (s, night) = post(&w, sets, "key-ps-0002", json!({ "name": "Night staff", "valid_from": "2026-12-01", "values": values })).await;
    assert_eq!((s, night["draft_version"]["state"].as_str()), (StatusCode::CREATED, Some("draft")), "{night}");
    let (s, b) = post(&w, sets, "key-ps-0003", json!({ "name": "night STAFF", "valid_from": "2026-12-01" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("name_in_use")), "{b}");
    let (nid, nv) = (night["id"].as_str().unwrap().to_string(), night["draft_version"]["id"].as_str().unwrap().to_string());

    // PS4: a draft is edited, then submitted; with AC-04 off it is approved at once (scheduled: it starts later).
    let mut values = values;
    values["optional_holidays_per_year"] = json!(2);
    let (s, _, b) = call(&w, Method::PATCH, &v_uri(&nid, &format!("/{nv}")), admin, &[("if-match", "\"1\"")], Some(json!({ "values": values }))).await;
    assert_eq!((s, b["version"].as_i64()), (StatusCode::OK, Some(2)), "{b}");
    let (s, _, b) = step(nid.clone(), nv.clone(), "submit", None, "\"2\"", json!({})).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("scheduled")), "{b}");
    let (_, _, b) = call(&w, Method::GET, &v_uri(&nid, &format!("/{nv}?with_defaults=true")), admin, &[], None).await;
    assert_eq!((b["values"]["late_grace_minutes"].as_i64(), b["values"]["break_minutes"].as_i64()), (Some(15), Some(60)), "{b}");
    let (s, _, b) = call(&w, Method::PATCH, &v_uri(&nid, &format!("/{nv}")), admin, &[("if-match", "\"3\"")], Some(json!({ "values": {} }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_draft")), "{b}");

    // A date in a locked period is refused (AC-AT-21.3).
    sqlx::query(
        "WITH p AS (INSERT INTO app.attendance_periods (org_id, payroll_group_id, period) VALUES ($1, $2, '[2026-10-01,2026-11-01)') RETURNING id)
         INSERT INTO app.lock_units (org_id, period_id, branch_id, state) SELECT $1, id, $3, 'locked' FROM p",
    )
    .bind(w.org).bind(w.group).bind(w.chn).execute(&w.pool).await.unwrap();
    let (s, _, b) = call(&w, Method::POST, &v_uri(&nid, ""), admin, &[("idempotency-key", "key-ps-0004")], Some(json!({ "valid_from": "2026-10-15" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("date_locked")), "{b}");

    // PS6 + PS8: assigned to Chennai from 1 December; the most specific source wins (AC-AT-21.2).
    let assign = json!({ "policy_set_id": nid, "level": "branch", "target_id": w.chn, "valid_from": "2026-12-01", "reason": "Night work" });
    let (s, a) = post(&w, "/v1/attendance/policy-assignments", "key-pa-0001", assign.clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{a}");
    let mut again = assign;
    again["valid_from"] = json!("2027-01-01");
    let (s, b) = post(&w, "/v1/attendance/policy-assignments", "key-pa-0002", again).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("assignment_overlaps")), "{b}");
    let (_, meena) = post(&w, "/v1/employees", "key-ps-meena1", new_employee(&w, "Meena", "+919800000073")).await;
    let emp = meena["id"].as_str().unwrap().to_string();
    let resolved = |on: &'static str| {
        let (w, emp) = (&w, emp.clone());
        async move { call(w, Method::GET, &format!("/v1/attendance/employees/{emp}/policy-set?as_of={on}"), Some(&w.admin), &[], None).await.2 }
    };
    let r = resolved("2026-11-30").await;
    assert_eq!((r["source"].as_str(), r["policy_set"]["id"].as_str()), (Some("default"), Some(default_id.as_str())), "{r}");
    let r = resolved("2026-12-05").await;
    assert_eq!((r["source"].as_str(), r["policy_set"]["name"].as_str()), (Some("branch"), Some("Night staff")), "{r}");

    // PS7: an employee override is a dated job change and beats the branch; E2 shows it.
    let (_, special) = post(&w, sets, "key-ps-0005", json!({ "name": "Special", "valid_from": "2026-11-01" })).await;
    let (sid, sv) = (special["id"].as_str().unwrap().to_string(), special["draft_version"]["id"].as_str().unwrap().to_string());
    let (s, _, b) = step(sid.clone(), sv, "submit", None, "\"1\"", json!({})).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("active")), "{b}");
    let body = json!({ "effective_from": "2026-12-10", "policy_set_id": sid, "reason": "Personal arrangement" });
    let (s, _, b) = call(&w, Method::PUT, &format!("/v1/attendance/employees/{emp}/policy-set"), admin, &[("if-match", "\"1\"")], Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(resolved("2026-12-15").await["source"], "employee");
    let (_, _, e2) = call(&w, Method::GET, &format!("/v1/employees/{emp}?as_of=2026-12-15"), admin, &[], None).await;
    assert_eq!(e2["policy_set"]["policy_set"]["name"], "Special", "{e2}");

    // PS6 end: the assignment stops after 31 December.
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/policy-assignments/{}/end", a["id"].as_str().unwrap()), admin, &[],
        Some(json!({ "valid_to": "2026-12-31", "reason": "Season over" }))).await;
    assert_eq!((s, b["valid_to"].as_str()), (StatusCode::OK, Some("2026-12-31")), "{b}");

    // PS3: the default can't be switched off; a set still assigned can't either; rename needs If-Match.
    let (s, _, b) = call(&w, Method::POST, &format!("{sets}/{default_id}/deactivate"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("cannot_deactivate_default")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("{sets}/{nid}/deactivate"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("policy_set_in_use")), "{b}");
    let (s, h, b) = call(&w, Method::PATCH, &format!("{sets}/{nid}"), admin, &[("if-match", "\"1\"")], Some(json!({ "name": "Night shift staff" }))).await;
    assert_eq!((s, h["etag"].to_str().unwrap()), (StatusCode::OK, "\"2\""), "{b}");

    // AC-04 on: a change waits for an Organisation Admin who is not its author (AC-AT-21.4).
    let ac = json!({ "valid_from": "2026-01-01", "values": { "settings_change_approval": true }, "reason": "Maker-checker" });
    let (s, _, b) = call(&w, Method::PUT, "/v1/org/settings/AC", admin, &[("idempotency-key", "key-ac-0001")], Some(ac)).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let (s, _, d) = call(&w, Method::POST, &v_uri(&nid, ""), hr, &[("idempotency-key", "key-ps-0006")], Some(json!({ "valid_from": "2027-01-01", "values": { "rounding": true } }))).await;
    assert_eq!(s, StatusCode::CREATED, "{d}");
    let dv = d["id"].as_str().unwrap().to_string();
    let (s, _, b) = step(nid.clone(), dv.clone(), "submit", Some("hr"), "\"1\"", json!({})).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("pending_approval")), "{b}");
    let (s, _, _) = step(nid.clone(), dv.clone(), "approve", Some("hr"), "\"2\"", json!({})).await;
    assert_eq!(s, StatusCode::FORBIDDEN); // HR Manager has no approve permission
    let (s, _, b) = step(nid.clone(), dv.clone(), "reject", None, "\"2\"", json!({})).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("reason")), "{b}");
    let (s, _, b) = step(nid.clone(), dv.clone(), "approve", None, "\"2\"", json!({ "reason": "Looks right" })).await;
    assert_eq!((s, b["state"].as_str(), b["decided_by"].is_null()), (StatusCode::OK, Some("scheduled"), false), "{b}");
    // The admin's own draft can't be approved by the admin.
    let (_, _, d2) = call(&w, Method::POST, &v_uri(&nid, ""), admin, &[("idempotency-key", "key-ps-0007")], Some(json!({ "valid_from": "2027-02-01" }))).await;
    let d2 = d2["id"].as_str().unwrap().to_string();
    step(nid.clone(), d2.clone(), "submit", None, "\"1\"", json!({})).await;
    let (s, _, b) = step(nid.clone(), d2.clone(), "approve", None, "\"2\"", json!({})).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("cannot_decide_own_request")), "{b}");
    let (s, _, b) = step(nid.clone(), d2, "discard", None, "\"2\"", json!({ "reason": "Not needed" })).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("discarded")), "{b}");

    let (_, _, vs) = call(&w, Method::GET, &v_uri(&nid, ""), admin, &[], None).await;
    let states: Vec<&str> = vs["items"].as_array().unwrap().iter().map(|v| v["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["discarded", "scheduled", "scheduled"], "{vs}");
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND event_type = 'PolicyChanged'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert!(events >= 5, "{events}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn fences_punches_and_reviews_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let fences = format!("/v1/attendance/branches/{}/fences", w.chn);

    // F2: a new fence is a draft; HR Manager for Bangalore can't add one in Chennai; F4 measures; F5 switches it on.
    let place = json!({ "name": "Chennai gate", "latitude": 13.08, "longitude": 80.27 });
    let (s, _, b) = call(&w, Method::POST, &fences, hr, &[("idempotency-key", "key-gf-0000")], Some(place.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    let (s, f) = post(&w, &fences, "key-gf-0001", place).await;
    assert_eq!((s, f["state"].as_str(), f["radius_m"].as_i64()), (StatusCode::CREATED, Some("draft"), Some(200)), "{f}");
    let fid = f["id"].as_str().unwrap().to_string();
    let (s, b) = post(&w, &format!("/v1/attendance/fences/{fid}/test"), "-", json!({ "latitude": 13.0841, "longitude": 80.27 })).await;
    assert_eq!((s, b["inside"].as_bool()), (StatusCode::OK, Some(false)), "{b}");
    assert!((b["distance_m"].as_f64().unwrap() - 456.0).abs() < 5.0, "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/fences/{fid}/activate"), admin, &[("if-match", "\"1\"")], None).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("active")), "{b}");
    let (s, _, b) = call(&w, Method::PATCH, &format!("/v1/attendance/fences/{fid}"), admin, &[("if-match", "\"1\"")], Some(json!({ "radius_m": 250 }))).await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED, "{b}");
    let (s, _, b) = call(&w, Method::PATCH, &format!("/v1/attendance/fences/{fid}"), admin, &[("if-match", "\"2\"")], Some(json!({ "radius_m": 250 }))).await;
    assert_eq!((s, b["radius_m"].as_i64()), (StatusCode::OK, Some(250)), "{b}");
    let (_, _, l) = call(&w, Method::GET, &fences, admin, &[], None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1, "{l}");

    // Meena at Chennai on the general shift today, so a punch is not an off-day punch.
    let gen = shift(&w, "key-pu-gen1", json!({ "code": "GEN", "name": "General", "valid_from": "2026-01-01" })).await;
    let (_, meena) = post(&w, "/v1/employees", "key-pu-meena1", new_employee(&w, "Meena", "+919800000074")).await;
    let emp = meena["id"].as_str().unwrap().to_string();
    let today: String = sqlx::query_scalar("SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date::text").fetch_one(&w.pool).await.unwrap();
    roster(&w, &w.admin, "key-pu-ro1", json!([{ "employee_id": emp, "work_date": today, "shift_id": gen }])).await;

    // The app sign-in binds the first phone (DEC-20).
    let code = meena["employee_code"].as_str().unwrap().to_string();
    let (s, b) = post(&w, &format!("/v1/employees/{emp}/portal-access"), "key-pu-pa1", json!({ "send_email": false })).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let temp = b["temporary_pin"].as_str().unwrap().to_string();
    let sign_in = |pin: String, phone: &'static str| {
        let (w, code) = (&w, code.clone());
        async move {
            call(w, Method::POST, "/v1/auth/employee/sign-in", None, &[],
                 Some(json!({ "org_short_code": w.org_code, "employee_code": code, "pin": pin, "device": { "id": phone, "platform": "android" } }))).await.2
        }
    };
    let b = sign_in(temp.clone(), "phone-1").await;
    assert_eq!(b["device_bound"].as_bool(), Some(true), "{b}");
    let (s, _, _) = call(&w, Method::POST, "/v1/auth/employee/pin", b["access_token"].as_str(), &[], Some(json!({ "current_pin": temp, "new_pin": "482913" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let b = sign_in("482913".into(), "phone-2").await;
    assert_eq!(b["device_bound"].as_bool(), Some(false), "{b}");
    let tok = b["access_token"].as_str().unwrap().to_string();
    let me = Some(tok.as_str());

    // P1: only the bound phone; inside the fence → clean; a repeat of the same key returns the stored punch.
    // The phone time from the database clock: fence versions are stamped with it, and this machine's clock may lag.
    let now: String = sqlx::query_scalar("SELECT to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')").fetch_one(&w.pool).await.unwrap();
    let punch = |dir: &str, key: &str, phone: &str, lat: f64| json!({ "direction": dir, "client_time": now, "queued": false, "client_key": key,
        "location": { "latitude": lat, "longitude": 80.27, "accuracy_m": 15.0 }, "device": { "id": phone, "platform": "android" } });
    let uri = "/v1/attendance/me/punches";
    let (s, _, b) = call(&w, Method::POST, uri, me, &[], Some(punch("in", "k1", "phone-2", 13.08))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("device_not_bound")), "{b}");
    let (s, _, p1) = call(&w, Method::POST, uri, me, &[], Some(punch("in", "k1", "phone-1", 13.08))).await;
    assert_eq!((s, p1["review_opened"].as_bool(), p1["flags"].as_array().map(Vec::len)), (StatusCode::CREATED, Some(false), Some(0)), "{p1}");
    assert_eq!(p1["punch"]["source"], "app");
    let (s, _, b) = call(&w, Method::POST, uri, me, &[], Some(punch("in", "k1", "phone-1", 13.08))).await;
    assert_eq!((s, b["punch"]["id"].clone()), (StatusCode::OK, p1["punch"]["id"].clone()), "{b}");
    // About 450 m out: accepted, flagged, sent for review (PC-04 flag, DEC-18).
    let (s, _, p2) = call(&w, Method::POST, uri, me, &[], Some(punch("out", "k2", "phone-1", 13.0841))).await;
    assert_eq!((s, p2["flags"].clone(), p2["review_opened"].as_bool()), (StatusCode::CREATED, json!(["outside_fence"]), Some(true)), "{p2}");
    let pid2 = p2["punch"]["id"].as_str().unwrap().to_string();
    // Web without location is refused by default (PC-13).
    let (s, _, b) = call(&w, Method::POST, uri, me, &[], Some(json!({ "direction": "in", "client_key": "k3" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("location_required")), "{b}");

    // P2: today's punches and the next direction.
    let (s, _, t) = call(&w, Method::GET, "/v1/attendance/me/punches/today", me, &[], None).await;
    assert_eq!((s, t["punches"].as_array().map(Vec::len), t["next_direction"].as_str(), t["shift"]["code"].as_str()),
               (StatusCode::OK, Some(2), Some("in"), Some("GEN")), "{t}");

    // P4: Org Admin sees both; the Bangalore HR Manager sees none of Chennai's.
    let list = format!("/v1/attendance/punches?from={today}&to={today}");
    let (s, _, l) = call(&w, Method::GET, &list, admin, &[], None).await;
    assert_eq!((s, l["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(2)), "{l}");
    let (_, _, l) = call(&w, Method::GET, &format!("{list}&flag=outside_fence"), admin, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(1), "{l}");
    let (_, _, l) = call(&w, Method::GET, &list, hr, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(0), "{l}");

    // P5/P6: the review queue; reject needs a reason and excludes the punch; a decided review can't be approved.
    let (s, _, q) = call(&w, Method::GET, "/v1/attendance/punch-reviews?state=pending", admin, &[], None).await;
    assert_eq!((s, q["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(1)), "{q}");
    let rid = q["items"][0]["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, &format!("/v1/attendance/punch-reviews/{rid}/approve"), me, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/punch-reviews/{rid}/reject"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("reason")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/punch-reviews/{rid}/reject"), admin, &[("if-match", "\"1\"")], Some(json!({ "reason": "Not at work" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("rejected")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/punch-reviews/{rid}/approve"), admin, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_pending")), "{b}");

    // P7: include it again; including twice changes nothing; move_date needs the date.
    let adj = format!("/v1/attendance/punches/{pid2}/adjudications");
    let (s, b) = post(&w, &adj, "key-pu-adj1", json!({ "kind": "include", "reason": "Was at the client site" })).await;
    assert_eq!((s, b["counted"].as_bool(), b["adjudications"].as_array().map(Vec::len)), (StatusCode::CREATED, Some(true), Some(2)), "{b}");
    let (s, b) = post(&w, &adj, "key-pu-adj2", json!({ "kind": "include", "reason": "Again" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_adjudication")), "{b}");
    let (s, b) = post(&w, &adj, "key-pu-adj3", json!({ "kind": "move_date", "reason": "Night" })).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("new_work_date")), "{b}");

    // P3: reset the binding; the old phone is refused; the next app sign-in binds the new one.
    let reset = format!("/v1/attendance/employees/{emp}/device-binding/reset");
    let (s, _, b) = call(&w, Method::POST, &reset, admin, &[], Some(json!({ "reason": "Lost phone" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{b}");
    let (s, _, b) = call(&w, Method::POST, &reset, admin, &[], Some(json!({ "reason": "Again" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("no_binding")), "{b}");
    let (s, _, b) = call(&w, Method::POST, uri, me, &[], Some(punch("in", "k4", "phone-1", 13.08))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("device_not_bound")), "{b}");
    assert_eq!(sign_in("482913".into(), "phone-2").await["device_bound"].as_bool(), Some(true));
    let (s, _, b) = call(&w, Method::POST, uri, me, &[], Some(punch("in", "k4", "phone-2", 13.08))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");

    // P8: manual attendance for a device failure; Bangalore's HR can't enter Chennai's; the sheet shows it; P9 voids.
    let yesterday: String = sqlx::query_scalar("SELECT ((now() AT TIME ZONE 'Asia/Kolkata')::date - 1)::text").fetch_one(&w.pool).await.unwrap();
    let batch = json!({ "branch_id": w.chn, "work_date": yesterday, "reason": "Device down", "entries": [
        { "employee_id": emp, "in_time": "09:00:00", "out_time": "18:00:00" }, { "employee_id": emp } ] });
    let (s, _, _) = call(&w, Method::POST, "/v1/attendance/manual-attendance", hr, &[("idempotency-key", "key-pu-man0")], Some(batch.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/manual-attendance", admin, &[("idempotency-key", "key-pu-man1")], Some(batch)).await;
    assert_eq!((s, b["created"].as_i64(), b["failed"][0]["code"].as_str()), (StatusCode::OK, Some(1), Some("invalid_value")), "{b}");
    let (_, _, sh) = call(&w, Method::GET, &format!("/v1/attendance/manual-attendance/sheet?branch_id={}&work_date={yesterday}", w.chn), admin, &[], None).await;
    let entries = sh["items"].as_array().unwrap().iter().find(|i| i["employee"]["id"] == json!(emp)).unwrap()["entries"].clone();
    assert_eq!(entries.as_array().map(Vec::len), Some(2), "{sh}");
    let mid = entries[0]["id"].as_str().unwrap().to_string();
    let void = format!("/v1/attendance/manual-punches/{mid}/void");
    let (s, _, b) = call(&w, Method::POST, &void, admin, &[], Some(json!({ "reason": "Wrong time" }))).await;
    assert_eq!((s, b["voided"].as_bool()), (StatusCode::OK, Some(true)), "{b}");
    let (s, _, b) = call(&w, Method::POST, &void, admin, &[], Some(json!({ "reason": "Again" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("already_voided")), "{b}");

    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND event_type = 'PunchRecorded'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert!(events >= 6, "{events}");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn day_results_and_recalculation_end_to_end() {
    let w = world().await;
    let (admin, hr) = (Some(w.admin.as_str()), Some(w.hr_blr.as_str()));
    let jobs_token = "jobs-token-jobs-token-jobs-token-07";
    let bearer = format!("Bearer {jobs_token}");
    let auth = [("authorization", bearer.as_str())];
    let jobs = api::router(w.state.clone().with_jobs_token(jobs_token));
    let job = |uri: &'static str, body: Value| {
        let (jobs, auth) = (jobs.clone(), auth);
        async move { call_app(&jobs, Method::POST, uri, None, &auth, Some(body)).await }
    };
    // Runs the outbox until this organisation has nothing waiting (dev holds other organisations' events too).
    let drain = || async {
        for _ in 0..30 {
            let left: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND dispatched_at IS NULL")
                .bind(w.org).fetch_one(&w.pool).await.unwrap();
            if left == 0 {
                return;
            }
            let (s, _, b) = call_app(&jobs, Method::POST, "/internal/outbox/dispatch", None, &auth, None).await;
            assert_eq!(s, StatusCode::OK, "{b}");
        }
        panic!("outbox not drained");
    };

    // Three recent working days (not Sunday, the default weekly off), rostered on the general shift.
    let days: Vec<String> = sqlx::query_scalar(
        "SELECT d::date::text FROM generate_series((now() AT TIME ZONE 'Asia/Kolkata')::date - 6, (now() AT TIME ZONE 'Asia/Kolkata')::date - 1, interval '1 day') d
         WHERE extract(isodow FROM d) <> 7 ORDER BY d LIMIT 3",
    ).fetch_all(&w.pool).await.unwrap();
    let (d1, d2, d3) = (days[0].clone(), days[1].clone(), days[2].clone());
    let gen = shift(&w, "key-dy-gen1", json!({ "code": "GEN", "name": "General", "valid_from": "2026-01-01" })).await;
    let (_, meena) = post(&w, "/v1/employees", "key-dy-meena1", new_employee(&w, "Meena", "+919800000075")).await;
    let emp = meena["id"].as_str().unwrap().to_string();
    let entries: Vec<Value> = days.iter().map(|d| json!({ "employee_id": emp, "work_date": d, "shift_id": gen })).collect();
    roster(&w, &w.admin, "key-dy-ro1", json!(entries)).await;

    // Manual attendance (device down): d1 in 09:25 out 18:00; d2 in only; d3 a full day on time.
    for (n, (d, i, o)) in [(&d1, "09:25:00", Some("18:00:00")), (&d2, "09:00:00", None), (&d3, "08:58:00", Some("18:02:00"))].into_iter().enumerate() {
        let mut e = json!({ "employee_id": emp, "in_time": i });
        if let Some(o) = o {
            e["out_time"] = json!(o);
        }
        let (s, _, b) = call(&w, Method::POST, "/v1/attendance/manual-attendance", admin, &[("idempotency-key", &format!("key-dy-man{n}"))],
            Some(json!({ "branch_id": w.chn, "work_date": d, "reason": "Device down", "entries": [e] }))).await;
        assert_eq!((s, b["created"].as_i64()), (StatusCode::OK, Some(1)), "{b}");
    }
    drain().await;

    // DY2: the stored result with its explanation (E2: 25 minutes late, a late mark).
    let day_uri = |d: &str| format!("/v1/attendance/employees/{emp}/days/{d}");
    let (s, _, r1) = call(&w, Method::GET, &day_uri(&d1), admin, &[], None).await;
    assert_eq!((s, r1["status"].as_str(), r1["late_minutes"].as_i64(), r1["late_mark"].as_bool(), r1["version"].as_i64()),
               (StatusCode::OK, Some("present"), Some(25), Some(true), Some(1)), "{r1}");
    assert_eq!((r1["net_minutes"].as_i64(), r1["explanation"]["punches_used"].as_array().map(Vec::len), r1["shift"]["code"].as_str()), (Some(455), Some(2), Some("GEN")), "{r1}");
    assert_eq!(r1["pending_recalculation"], false, "{r1}");
    // E13: in only → missed punch, waiting for regularisation.
    let (_, _, r2) = call(&w, Method::GET, &day_uri(&d2), admin, &[], None).await;
    assert_eq!((r2["status"].as_str(), r2["flags"].clone()), (Some("absent_pending"), json!(["missed_punch"])), "{r2}");

    // J-AT2 again: nothing changed, nothing stored.
    let (s, _, b) = job("/internal/jobs/recalculate", json!({ "org_id": w.org, "employee_id": emp, "from": d1, "to": d3, "reason": "retry" })).await;
    assert_eq!((s, b["changed"].as_i64()), (StatusCode::OK, Some(0)), "{b}");

    // P9 void of d1's in-punch → a new version: single punch. ?known_on returns the earlier one.
    let first_calc = r1["calculated_at"].as_str().unwrap().to_string();
    let (_, _, sh) = call(&w, Method::GET, &format!("/v1/attendance/manual-attendance/sheet?branch_id={}&work_date={d1}", w.chn), admin, &[], None).await;
    let mine = sh["items"].as_array().unwrap().iter().find(|i| i["employee"]["id"] == json!(emp)).unwrap()["entries"].clone();
    let in_id = mine.as_array().unwrap().iter().find(|m| m["direction"] == "in").unwrap()["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/manual-punches/{in_id}/void"), admin, &[], Some(json!({ "reason": "Wrong entry" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, _, b) = call(&w, Method::GET, &day_uri(&d1), admin, &[], None).await;
    assert_eq!(b["pending_recalculation"], true, "{b}");
    drain().await;
    let (_, _, b) = call(&w, Method::GET, &day_uri(&d1), admin, &[], None).await;
    assert_eq!((b["version"].as_i64(), b["status"].as_str(), b["versions"].as_array().map(Vec::len)), (Some(2), Some("absent_pending"), Some(2)), "{b}");
    let known = format!("{}?known_on={}", day_uri(&d1), first_calc.replace('+', "%2B"));
    let (_, _, b) = call(&w, Method::GET, &known, admin, &[], None).await;
    assert_eq!((b["version"].as_i64(), b["status"].as_str()), (Some(1), Some("present")), "{b}");

    // DY1: scoped list with filters; the Bangalore HR Manager sees none of Chennai's days.
    let list = format!("/v1/attendance/days?from={d1}&to={d3}&employee_id={emp}");
    let (s, _, l) = call(&w, Method::GET, &list, admin, &[], None).await;
    assert_eq!((s, l["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(3)), "{l}");
    let (_, _, l) = call(&w, Method::GET, &format!("{list}&flag=missed_punch"), admin, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(2), "{l}");
    let (_, _, l) = call(&w, Method::GET, &list, hr, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(0), "{l}");

    // J-AT1: the full day is past its window + 1 h and clear → finalised (so is the absent day before d1, calculated
    // because a punch also recalculates the day before); the missed-punch days stay provisional.
    let (s, _, b) = job("/internal/jobs/finalise", json!({ "org_id": w.org })).await;
    assert_eq!((s, b["finalised"].as_i64()), (StatusCode::OK, Some(2)), "{b}");
    let (_, _, b) = call(&w, Method::GET, &day_uri(&d3), admin, &[], None).await;
    assert_eq!((b["state"].as_str(), b["status"].as_str()), (Some("finalised"), Some("present")), "{b}");
    let (_, _, b) = call(&w, Method::GET, &day_uri(&d2), admin, &[], None).await;
    assert_eq!(b["state"], "provisional");

    // DY3: manual retry needs a reason and edit scope; queued through the outbox.
    let re = format!("/v1/attendance/employees/{emp}/days/recalculate");
    let (s, _, _) = call(&w, Method::POST, &re, hr, &[], Some(json!({ "from": d1, "to": d3, "reason": "Check" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, b) = call(&w, Method::POST, &re, admin, &[], Some(json!({ "from": d1, "to": d3 }))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("reason")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &re, admin, &[], Some(json!({ "from": d1, "to": d3, "reason": "Check" }))).await;
    assert_eq!((s, b["queued"].as_i64()), (StatusCode::ACCEPTED, Some(3)), "{b}");
    drain().await;

    // J-AT3: the sweep fills the days without punches (absent, pending, or weekly off).
    let (s, _, b) = job("/internal/jobs/attendance-sweep", json!({ "org_id": w.org })).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b["changed"].as_i64().unwrap() >= 5, "{b}");
    let (_, _, b) = job("/internal/jobs/attendance-sweep", json!({ "org_id": w.org })).await;
    assert_eq!(b["changed"].as_i64(), Some(0), "{b}");

    // M1–M3 for Meena herself.
    let e: Uuid = emp.parse().unwrap();
    let user: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, employee_id, display_name, state) VALUES ($1, 'employee', $2, 'Meena', 'active') RETURNING id")
        .bind(w.org).bind(e).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'employee'")
        .bind(w.org).bind(user).execute(&w.pool).await.unwrap();
    let token = issue_access_token(SECRET, user, w.org, session(&w.pool, w.org, user).await, "employee");
    let me = Some(token.as_str());
    let (s, _, f) = call(&w, Method::GET, "/v1/attendance/me/attendance/features", me, &[], None).await;
    assert_eq!((s, f["punch"]["web"].as_bool(), f["regularisation"]["window_days"].as_i64()), (StatusCode::OK, Some(true), Some(7)), "{f}");
    let (s, _, b) = call(&w, Method::GET, &format!("/v1/attendance/me/days/{d3}"), me, &[], None).await;
    assert_eq!((s, b["status"].as_str()), (StatusCode::OK, Some("present")), "{b}");
    let (s, _, b) = call(&w, Method::GET, &format!("/v1/attendance/me/days?from={d1}&to={d3}"), me, &[], None).await;
    assert_eq!((s, b["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(3)), "{b}");
    let (s, _, b) = call(&w, Method::GET, &format!("/v1/attendance/me/roster?from={d1}&to={d3}"), me, &[], None).await;
    assert_eq!((s, b["entries"][&d1]["code"].as_str()), (StatusCode::OK, Some("GEN")), "{b}");
    let (s, _, _) = call(&w, Method::GET, &format!("/v1/attendance/employees/{emp}/days/{d3}"), me, &[], None).await;
    assert_eq!(s, StatusCode::OK); // own day through DY2 too

    let changed: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND event_type = 'DayResultChanged'")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert!(changed >= 4, "{changed}");
}

/// Raw request for multipart uploads and .xlsx downloads.
async fn call_raw(app: &axum::Router, method: Method, uri: &str, token: Option<&str>, headers: &[(&str, &str)], body: Vec<u8>) -> (StatusCode, Vec<u8>) {
    use tower::ServiceExt;
    let mut req = axum::http::Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = app.clone().oneshot(req.body(axum::body::Body::from(body)).unwrap()).await.unwrap();
    let status = res.status();
    (status, axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec())
}

/// Waits for a background import to finish and returns it.
async fn import_done(w: &World, uri: &str) -> Value {
    for _ in 0..120 {
        let (s, _, b) = call(w, Method::GET, uri, Some(&w.admin), &[], None).await;
        assert_eq!(s, StatusCode::OK, "{b}");
        if b["state"] == "completed" || b["state"] == "failed" {
            return b;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("import {uri} did not finish");
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn imports_notifications_and_reports_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let jobs_token = "jobs-token-jobs-token-jobs-token-10";
    let bearer = format!("Bearer {jobs_token}");
    let auth = [("authorization", bearer.as_str())];
    let jobs = api::router(w.state.clone().with_jobs_token(jobs_token));
    let drain = || async {
        for _ in 0..30 {
            let left: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND dispatched_at IS NULL")
                .bind(w.org).fetch_one(&w.pool).await.unwrap();
            if left == 0 {
                return;
            }
            let (s, _, b) = call_app(&jobs, Method::POST, "/internal/outbox/dispatch", None, &auth, None).await;
            assert_eq!(s, StatusCode::OK, "{b}");
        }
        panic!("outbox not drained");
    };
    let dates: Vec<String> = sqlx::query_scalar(
        "SELECT (t + i)::text FROM (SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date AS t) x, unnest(ARRAY[-2, -1, 3]) i ORDER BY i",
    ).fetch_all(&w.pool).await.unwrap();
    let (d1, d2, d3) = (dates[0].clone(), dates[1].clone(), dates[2].clone());
    shift(&w, "key-im-gen1", json!({ "code": "GEN", "name": "General", "valid_from": "2026-01-01" })).await;
    let (_, ravi) = post(&w, "/v1/employees", "key-im-ravi1", new_employee(&w, "Ravi", "+919800000081")).await;
    let (emp, code) = (ravi["id"].as_str().unwrap().to_string(), ravi["employee_code"].as_str().unwrap().to_string());

    // R6: roster file — GEN on d1 and d2, weekly off on d3, one unknown shift.
    let (s, b) = call_raw(&w.app, Method::GET, "/v1/attendance/roster-imports/template", admin, &[], vec![]).await;
    assert!(s == StatusCode::OK && b.starts_with(b"PK"));
    let csv = format!("employee_code,work_date,shift_code,weekly_off\n{code},{d1},GEN,\n{code},{d2},gen,\n{code},{d3},,yes\n{code},{d3},NOPE,\n");
    let mut body = b"--XB\r\nContent-Disposition: form-data; name=\"file\"; filename=\"roster.csv\"\r\nContent-Type: text/csv\r\n\r\n".to_vec();
    body.extend_from_slice(csv.as_bytes());
    body.extend_from_slice(b"\r\n--XB--\r\n");
    let (s, b) = call_raw(&w.app, Method::POST, "/v1/attendance/roster-imports", admin, &[("content-type", "multipart/form-data; boundary=XB"), ("idempotency-key", "key-im-ro1")], body).await;
    let b: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(s, StatusCode::ACCEPTED, "{b}");
    let ro = import_done(&w, &format!("/v1/attendance/roster-imports/{}", b["import_id"].as_str().unwrap())).await;
    assert_eq!((ro["applied_count"].as_i64(), ro["error_count"].as_i64()), (Some(3), Some(1)), "{ro}");
    assert_eq!(day(&w, &emp, &d3).await["is_weekly_off"], true);
    let (s, _, l) = call(&w, Method::GET, "/v1/attendance/roster-imports", admin, &[], None).await;
    assert_eq!((s, l["items"].as_array().map(Vec::len)), (StatusCode::OK, Some(1)), "{l}");

    // P10 JSON: d1 in and out, d2 in only (a missed punch), a row without a time, and x1 again in the same batch.
    let rows = json!({ "rows": [
        { "external_id": "x1", "punch_time": format!("{d1}T09:00:00+05:30"), "employee_code": code, "direction": "in" },
        { "external_id": "x2", "punch_time": format!("{d1}T18:05:00+05:30"), "employee_code": code, "direction": "out" },
        { "external_id": "x3", "punch_time": format!("{d2}T09:10:00+05:30"), "employee_code": code, "direction": "in", "latitude": 13.08, "longitude": 80.27 },
        { "external_id": "x4", "employee_code": code },
        { "external_id": "x1", "punch_time": format!("{d1}T09:01:00+05:30"), "employee_code": code, "direction": "in" },
    ] });
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/punch-imports/json", admin, &[("idempotency-key", "key-im-p1")], Some(rows.clone())).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{b}");
    let p1 = format!("/v1/attendance/punch-imports/{}", b["import_id"].as_str().unwrap());
    let im = import_done(&w, &p1).await;
    assert_eq!((im["applied_count"].as_i64(), im["error_count"].as_i64(), im["skipped_count"].as_i64()), (Some(3), Some(1), Some(1)), "{im}");
    let (s, _, link) = call(&w, Method::GET, &format!("{p1}/errors"), admin, &[], None).await;
    assert_eq!(s, StatusCode::OK, "{link}");
    let (s, b) = call_raw(&w.app, Method::GET, link["url"].as_str().unwrap(), None, &[], vec![]).await;
    assert!(s == StatusCode::OK && b.starts_with(b"PK"));
    // The same rows again: applied rows skipped (same file); a new file repeating x1 is ignored (AC-AT-17.2).
    let (_, _, b) = call(&w, Method::POST, "/v1/attendance/punch-imports/json", admin, &[("idempotency-key", "key-im-p2")], Some(rows)).await;
    let im = import_done(&w, &format!("/v1/attendance/punch-imports/{}", b["import_id"].as_str().unwrap())).await;
    assert_eq!((im["applied_count"].as_i64(), im["skipped_count"].as_i64()), (Some(0), Some(4)), "{im}");
    let again = json!({ "rows": [{ "external_id": "x1", "punch_time": format!("{d1}T09:30:00+05:30"), "employee_code": code }] });
    let (_, _, b) = call(&w, Method::POST, "/v1/attendance/punch-imports/json", admin, &[("idempotency-key", "key-im-p3")], Some(again)).await;
    let im = import_done(&w, &format!("/v1/attendance/punch-imports/{}", b["import_id"].as_str().unwrap())).await;
    assert_eq!((im["applied_count"].as_i64(), im["skipped_count"].as_i64()), (Some(0), Some(1)), "{im}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app.raw_punches WHERE org_id = $1 AND source = 'import'").bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(n, 3);
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/punch-imports/json", admin, &[], Some(json!({ "rows": [{ "colour": "red" }] }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    drain().await;

    // RP1–RP7 and RP8.
    let w = &w;
    let get = |uri: String| async move { call(w, Method::GET, &uri, admin, &[], None).await };
    let (s, _, r) = get(format!("/v1/attendance/reports/r1?date={d1}")).await;
    let row = r["rows"].as_array().unwrap().iter().find(|x| x["employee_code"] == json!(code)).cloned().unwrap_or_default();
    assert_eq!((s, row["status"].as_str(), row["shift"].as_str()), (StatusCode::OK, Some("present"), Some("GEN")), "{r}");
    let (_, _, r) = get(format!("/v1/attendance/reports/r2?from={d1}&to={d2}")).await;
    let row = r["rows"].as_array().unwrap().iter().find(|x| x["employee_code"] == json!(code)).cloned().unwrap_or_default();
    assert_eq!((row[&d1].as_str(), row[&d2].as_str(), r["columns"].as_array().map(Vec::len)), (Some("P"), Some("A?"), Some(4)), "{r}");
    let (_, _, r) = get(format!("/v1/attendance/reports/r4?from={d1}&to={d2}")).await;
    assert_eq!(r["rows"].as_array().map(Vec::len), Some(1), "{r}");
    for rp in ["r3", "r5", "r23", "r26"] {
        let (s, _, r) = get(format!("/v1/attendance/reports/{rp}?from={d1}&to={d2}")).await;
        assert_eq!(s, StatusCode::OK, "{rp} {r}");
    }
    let (s, _, r) = get("/v1/attendance/reports/r1?colour=red".into()).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{r}");
    let (s, _, x) = call(w, Method::POST, "/v1/exports", admin, &[("idempotency-key", "key-im-x1")], Some(json!({ "report_code": "r2", "parameters": { "from": d1, "to": d2 } }))).await;
    assert_eq!((s, x["state"].as_str()), (StatusCode::OK, Some("ready")), "{x}");
    let (s, b) = call_raw(&w.app, Method::GET, x["download_url"].as_str().unwrap(), None, &[], vec![]).await;
    assert!(s == StatusCode::OK && b.starts_with(b"PK"));

    // DB1.
    let (s, _, b) = get("/v1/attendance/dashboard/attendance".into()).await;
    assert_eq!((s, b["branches"].is_array(), b["pending_approvals"]["punch_reviews"].as_i64()), (StatusCode::OK, true, Some(0)), "{b}");

    // N1: switch the HR digest off; no row means on.
    let ns = "/v1/attendance/notification-settings";
    let (s, _, b) = call(w, Method::PUT, ns, admin, &[], Some(json!({ "rows": [{ "event": "ApprovalsPending", "channel": "in_app", "recipient": "hr", "enabled": false }] }))).await;
    assert_eq!((s, b["rows"][0]["recipient"].as_str()), (StatusCode::OK, Some("hr_manager")), "{b}");
    let (s, _, b) = call(w, Method::PUT, ns, admin, &[], Some(json!({ "rows": [{ "event": "x-y", "channel": "sms", "recipient": "boss", "enabled": true }] }))).await;
    assert_eq!((s, b["errors"].as_array().map(Vec::len)), (StatusCode::UNPROCESSABLE_ENTITY, Some(3)), "{b}");

    // J-AT8: Ravi's missed punch on d2 is reminded once per kind; N2 reads it.
    let e: Uuid = emp.parse().unwrap();
    let user: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, employee_id, display_name, state) VALUES ($1, 'employee', $2, 'Ravi', 'active') RETURNING id")
        .bind(w.org).bind(e).fetch_one(&w.pool).await.unwrap();
    sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = 'employee'")
        .bind(w.org).bind(user).execute(&w.pool).await.unwrap();
    let (s, _, b) = call_app(&jobs, Method::POST, "/internal/jobs/attendance-reminders", None, &auth, Some(json!({ "org_id": w.org }))).await;
    let sent = b["missed_punch_reminders"].as_i64().unwrap_or(0);
    assert!(s == StatusCode::OK && sent >= 1, "{b}");
    let (_, _, b) = call_app(&jobs, Method::POST, "/internal/jobs/attendance-reminders", None, &auth, Some(json!({ "org_id": w.org }))).await;
    assert_eq!(b["missed_punch_reminders"].as_i64(), Some(0), "{b}");
    let token = issue_access_token(SECRET, user, w.org, session(&w.pool, w.org, user).await, "employee");
    let me = Some(token.as_str());
    let (s, _, b) = call(w, Method::GET, "/v1/attendance/me/notifications?unread=true", me, &[], None).await;
    assert_eq!((s, b["unread_count"].as_i64(), b["items"][0]["event"].as_str()), (StatusCode::OK, Some(sent), Some("MissedPunch")), "{b}");
    let first = b["items"][0]["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(w, Method::POST, &format!("/v1/attendance/me/notifications/{first}/read"), me, &[], None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = call(w, Method::POST, &format!("/v1/attendance/me/notifications/{first}/read"), admin, &[], None).await;
    assert_eq!(s, StatusCode::NOT_FOUND); // someone else's notification
    let (_, _, b) = call(w, Method::POST, "/v1/attendance/me/notifications/read-all", me, &[], None).await;
    assert_eq!(b["marked"].as_i64(), Some(sent - 1), "{b}");
    let (s, _, _) = call(w, Method::GET, "/v1/attendance/reports/r1", me, &[], None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // J-AT9.
    let (s, _, b) = call_app(&jobs, Method::POST, "/internal/jobs/attendance-cleanup", None, &auth, None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

/// A signed-in user for an employee, with the given built-in roles.
async fn employee_user(w: &World, employee: &str, name: &str, roles: &[&str]) -> String {
    let e: Uuid = employee.parse().unwrap();
    let user: Uuid = sqlx::query_scalar("INSERT INTO app.users (org_id, kind, employee_id, display_name, state) VALUES ($1, 'employee', $2, $3, 'active') RETURNING id")
        .bind(w.org).bind(e).bind(name).fetch_one(&w.pool).await.unwrap();
    for r in roles {
        sqlx::query("INSERT INTO app.user_roles (org_id, user_id, role_id) SELECT $1, $2, id FROM app.roles WHERE org_id = $1 AND builtin_key = $3")
            .bind(w.org).bind(user).bind(*r).execute(&w.pool).await.unwrap();
    }
    issue_access_token(SECRET, user, w.org, session(&w.pool, w.org, user).await, "employee")
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn regularisation_and_corrections_end_to_end() {
    let w = world().await;
    let admin = Some(w.admin.as_str());
    let jobs_token = "jobs-token-jobs-token-jobs-token-08";
    let bearer = format!("Bearer {jobs_token}");
    let auth = [("authorization", bearer.as_str())];
    let jobs = api::router(w.state.clone().with_jobs_token(jobs_token));
    let drain = || async {
        for _ in 0..30 {
            let left: i64 = sqlx::query_scalar("SELECT count(*) FROM app.outbox_events WHERE org_id = $1 AND dispatched_at IS NULL")
                .bind(w.org).fetch_one(&w.pool).await.unwrap();
            if left == 0 {
                return;
            }
            let (s, _, b) = call_app(&jobs, Method::POST, "/internal/outbox/dispatch", None, &auth, None).await;
            assert_eq!(s, StatusCode::OK, "{b}");
        }
        panic!("outbox not drained");
    };
    let dates: Vec<String> = sqlx::query_scalar(
        "SELECT (t + i)::text FROM (SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date AS t) x, unnest(ARRAY[-2, -1]) i ORDER BY i",
    ).fetch_all(&w.pool).await.unwrap();
    let (d1, d2) = (dates[0].clone(), dates[1].clone());

    // Two approval levels (RG-05) for Chennai.
    let (s, set) = post(&w, "/v1/attendance/policy-sets", "key-rg-ps1", json!({ "name": "Two levels", "valid_from": "2026-01-01", "values": { "rg_levels": 2 } })).await;
    assert_eq!(s, StatusCode::CREATED, "{set}");
    let (sid, sv) = (set["id"].as_str().unwrap().to_string(), set["draft_version"]["id"].as_str().unwrap().to_string());
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/policy-sets/{sid}/versions/{sv}/submit"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = post(&w, "/v1/attendance/policy-assignments", "key-rg-pa1",
        json!({ "policy_set_id": sid, "level": "branch", "target_id": w.chn, "valid_from": "2026-01-01", "reason": "Two levels" })).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");

    // Kavi manages Asha; both rostered on GEN.
    let gen = shift(&w, "key-rg-gen1", json!({ "code": "GEN", "name": "General", "valid_from": "2026-01-01" })).await;
    let (_, kavi) = post(&w, "/v1/employees", "key-rg-kavi1", new_employee(&w, "Kavi", "+919800000091")).await;
    let kavi_id = kavi["id"].as_str().unwrap().to_string();
    let mut body = new_employee(&w, "Asha", "+919800000092");
    body["job"]["reporting_manager_id"] = json!(kavi_id);
    let (s, asha) = post(&w, "/v1/employees", "key-rg-asha1", body).await;
    assert_eq!(s, StatusCode::CREATED, "{asha}");
    let emp = asha["id"].as_str().unwrap().to_string();
    roster(&w, &w.admin, "key-rg-ro1", json!([{ "employee_id": emp, "work_date": d1, "shift_id": gen }, { "employee_id": emp, "work_date": d2, "shift_id": gen }])).await;
    let asha_t = employee_user(&w, &emp, "Asha", &["employee"]).await;
    let kavi_t = employee_user(&w, &kavi_id, "Kavi", &["employee", "manager"]).await;
    let (asha_t, kavi_t) = (Some(asha_t.as_str()), Some(kavi_t.as_str()));

    // d1: in only (missed punch). d2: 40 minutes late.
    for (n, (d, e)) in [(&d1, json!({ "employee_id": emp, "in_time": "09:00:00" })), (&d2, json!({ "employee_id": emp, "in_time": "09:40:00", "out_time": "18:00:00" }))].into_iter().enumerate() {
        let (s, _, b) = call(&w, Method::POST, "/v1/attendance/manual-attendance", admin, &[("idempotency-key", &format!("key-rg-man{n}"))],
            Some(json!({ "branch_id": w.chn, "work_date": d, "reason": "Device down", "entries": [e] }))).await;
        assert_eq!(s, StatusCode::OK, "{b}");
    }
    drain().await;
    let day_of = |d: String| {
        let (w, emp) = (&w, emp.clone());
        async move { call(w, Method::GET, &format!("/v1/attendance/employees/{emp}/days/{d}"), Some(&w.admin), &[], None).await.2 }
    };
    assert_eq!(day_of(d1.clone()).await["status"], "absent_pending");

    // RG1: Asha asks for the missing out; the manager is told.
    let regs = "/v1/attendance/regularisations";
    let ask = json!({ "work_date": d1, "kind": "missed_punch", "requested": { "out_time": "18:00:00" }, "reason": "Forgot to punch out" });
    let (s, _, r) = call(&w, Method::POST, regs, asha_t, &[("idempotency-key", "key-rg-r1")], Some(ask.clone())).await;
    assert_eq!((s, r["state"].as_str(), r["levels"].as_i64(), r["current_approver"].as_str()), (StatusCode::CREATED, Some("pending"), Some(2), Some("manager")), "{r}");
    let rid = r["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, regs, asha_t, &[("idempotency-key", "key-rg-r2")], Some(ask)).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("request_already_pending")), "{b}");
    let (s, _, b) = call(&w, Method::POST, regs, asha_t, &[("idempotency-key", "key-rg-rx")], Some(json!({ "work_date": d1, "kind": "late", "requested": { "out_time": "18:00:00" }, "reason": "x" }))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("requested")), "{b}");
    let (_, _, n) = call(&w, Method::GET, "/v1/attendance/me/notifications", kavi_t, &[], None).await;
    assert_eq!(n["items"][0]["event"], "RegularisationSubmitted", "{n}");
    let (_, _, l) = call(&w, Method::GET, &format!("{regs}?awaiting_me=true"), kavi_t, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(1), "{l}");
    let (_, _, l) = call(&w, Method::GET, &format!("{regs}?awaiting_me=true"), admin, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(0), "{l}");

    // RG4: HR can't skip the manager; the manager approves level 1, then HR the last level.
    let act = |what: &str| format!("{regs}/{rid}/{what}");
    let (s, _, b) = call(&w, Method::POST, &act("approve"), admin, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("wrong_approver")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act("approve"), kavi_t, &[("if-match", "\"1\"")], Some(json!({ "note": "OK" }))).await;
    assert_eq!((s, b["state"].as_str(), b["level"].as_i64(), b["current_approver"].as_str()), (StatusCode::OK, Some("pending"), Some(2), Some("hr")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act("approve"), kavi_t, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::FORBIDDEN, Some("wrong_approver")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act("approve"), admin, &[("if-match", "\"2\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str(), b["decisions"].as_array().map(Vec::len)), (StatusCode::OK, Some("approved"), Some(2)), "{b}");
    drain().await;
    let day = day_of(d1.clone()).await;
    assert_eq!((day["status"].as_str(), day["requests"].as_array().map(Vec::len)), (Some("present"), Some(1)), "{day}");

    // RG6: cancelling voids the punch it wrote; the day goes back.
    let (s, _, b) = call(&w, Method::POST, &act("cancel"), admin, &[("if-match", "\"3\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    let (s, _, b) = call(&w, Method::POST, &act("cancel"), admin, &[("if-match", "\"3\"")], Some(json!({ "reason": "Raised in error" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("cancelled")), "{b}");
    drain().await;
    assert_eq!(day_of(d1.clone()).await["status"], "absent_pending");

    // RG-07: a late request left two days is escalated to HR by the sweep; HR rejects with a reason.
    let (s, _, r) = call(&w, Method::POST, regs, asha_t, &[("idempotency-key", "key-rg-r3")],
        Some(json!({ "work_date": d2, "kind": "late", "requested": { "in_time": "09:00:00" }, "reason": "Bus late" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let late = r["id"].as_str().unwrap().to_string();
    sqlx::query("UPDATE app.regularisation_requests SET created_at = now() - interval '3 days' WHERE id = $1").bind(late.parse::<Uuid>().unwrap()).execute(&w.pool).await.unwrap();
    let (s, _, b) = call_app(&jobs, Method::POST, "/internal/jobs/attendance-sweep", None, &auth, Some(json!({ "org_id": w.org }))).await;
    assert_eq!((s, b["requests_actioned"].as_i64()), (StatusCode::OK, Some(1)), "{b}");
    let (_, _, b) = call(&w, Method::GET, &format!("{regs}/{late}"), asha_t, &[], None).await;
    assert_eq!((b["state"].as_str(), b["current_approver"].as_str()), (Some("escalated"), Some("hr")), "{b}");
    let v = format!("\"{}\"", b["version"]);
    let (s, _, b) = call(&w, Method::POST, &format!("{regs}/{late}/reject"), admin, &[("if-match", &v)], Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("{regs}/{late}/reject"), admin, &[("if-match", &v)], Some(json!({ "reason": "No proof" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("rejected")), "{b}");

    // RG5: only the raiser withdraws.
    let (_, _, r) = call(&w, Method::POST, regs, asha_t, &[("idempotency-key", "key-rg-r4")],
        Some(json!({ "work_date": d2, "kind": "early", "requested": { "out_time": "18:30:00" }, "reason": "Stayed late" }))).await;
    let early = r["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(&w, Method::POST, &format!("{regs}/{early}/withdraw"), kavi_t, &[("if-match", "\"1\"")], None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, b) = call(&w, Method::POST, &format!("{regs}/{early}/withdraw"), asha_t, &[("if-match", "\"1\"")], None).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("withdrawn")), "{b}");
    let (_, _, l) = call(&w, Method::GET, regs, asha_t, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(3), "{l}");

    // C1 / C2: HR sets d2 present without a late mark, then voids it.
    let ov = format!("/v1/attendance/employees/{emp}/days/{d2}/overrides");
    let (s, _, b) = call(&w, Method::POST, &ov, admin, &[("idempotency-key", "key-rg-o0")], Some(json!({ "fields": { "status": "here" }, "reason": "x" }))).await;
    assert_eq!((s, b["errors"][0]["field"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("fields.status")), "{b}");
    let before = day_of(d2.clone()).await;
    assert_eq!(before["late_mark"], true, "{before}");
    let (s, _, b) = call(&w, Method::POST, &ov, admin, &[("idempotency-key", "key-rg-o1")], Some(json!({ "fields": { "late_mark": false }, "reason": "Approved by HOD" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let oid = b["override"]["id"].as_str().unwrap().to_string();
    drain().await;
    assert_eq!(day_of(d2.clone()).await["late_mark"], false);
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/day-overrides/{oid}/void"), admin, &[], Some(json!({ "reason": "Wrong day" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/day-overrides/{oid}/void"), admin, &[], Some(json!({ "reason": "Again" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("already_voided")), "{b}");
    drain().await;
    assert_eq!(day_of(d2.clone()).await["late_mark"], true);

    // W1: a late waiver for Chennai on d2 clears the mark; withdrawn, it comes back.
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/waivers", admin, &[("idempotency-key", "key-rg-w0")], Some(json!({ "kind": "late", "work_date": d2, "reason": "Rain" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("target_required")), "{b}");
    let (s, _, wv) = call(&w, Method::POST, "/v1/attendance/waivers", admin, &[("idempotency-key", "key-rg-w1")], Some(json!({ "kind": "late", "work_date": d2, "branch_id": w.chn, "reason": "Rain" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{wv}");
    drain().await;
    assert_eq!(day_of(d2.clone()).await["late_mark"], false);
    let (_, _, l) = call(&w, Method::GET, "/v1/attendance/waivers", admin, &[], None).await;
    assert_eq!(l["items"].as_array().map(Vec::len), Some(1), "{l}");
    let (s, _, b) = call(&w, Method::POST, &format!("/v1/attendance/waivers/{}/withdraw", wv["id"].as_str().unwrap()), admin, &[], Some(json!({ "reason": "Sent early" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("withdrawn")), "{b}");
    drain().await;
    assert_eq!(day_of(d2.clone()).await["late_mark"], true);

    // O1: the outage fills d1's missing out at shift end; d2 already has both.
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/outage-regularisations", admin, &[("idempotency-key", "key-rg-x0")],
        Some(json!({ "branch_id": w.chn, "from": "2026-01-01", "to": d2, "reason": "Device down" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("range_too_long")), "{b}");
    let (s, _, b) = call(&w, Method::POST, "/v1/attendance/outage-regularisations", admin, &[("idempotency-key", "key-rg-x1")],
        Some(json!({ "branch_id": w.chn, "from": d1, "to": d2, "reason": "Device down" }))).await;
    assert_eq!((s, b["affected_employees"].as_i64(), b["punches_added"].as_i64()), (StatusCode::ACCEPTED, Some(1), Some(1)), "{b}");
    drain().await;
    assert_eq!(day_of(d1.clone()).await["status"], "present");
}

async fn ask(w: &World, token: Option<&str>, key: &str, body: Value) -> (StatusCode, axum::http::HeaderMap, Value) {
    call(w, Method::POST, "/v1/attendance/shift-requests", token, &[("idempotency-key", key)], Some(body)).await
}

#[tokio::test]
#[ignore = "writes to the database in DATABASE_URL"]
async fn shift_change_and_swap_requests_end_to_end() {
    let w = world().await;
    let reqs = "/v1/attendance/shift-requests";
    let d: String = sqlx::query_scalar("SELECT ((now() AT TIME ZONE 'Asia/Kolkata')::date + 3)::text").fetch_one(&w.pool).await.unwrap();
    let past: String = sqlx::query_scalar("SELECT ((now() AT TIME ZONE 'Asia/Kolkata')::date)::text").fetch_one(&w.pool).await.unwrap();

    // Kavi manages Asha (morning) and Ravi (evening), all at Chennai.
    let mor = shift(&w, "key-sw-mor1", json!({ "code": "MOR", "name": "Morning", "valid_from": "2026-01-01", "start_time": "06:00", "end_time": "14:00" })).await;
    let eve = shift(&w, "key-sw-eve1", json!({ "code": "EVE", "name": "Evening", "valid_from": "2026-01-01", "start_time": "14:00", "end_time": "22:00" })).await;
    let (_, kavi) = post(&w, "/v1/employees", "key-sw-kavi1", new_employee(&w, "Kavi", "+919800000191")).await;
    let kavi_id = kavi["id"].as_str().unwrap().to_string();
    let mut ids = Vec::new();
    for (n, (name, mobile)) in [("Asha", "+919800000192"), ("Ravi", "+919800000193")].into_iter().enumerate() {
        let mut body = new_employee(&w, name, mobile);
        body["job"]["reporting_manager_id"] = json!(kavi_id);
        let (s, e) = post(&w, "/v1/employees", &format!("key-sw-emp{n}"), body).await;
        assert_eq!(s, StatusCode::CREATED, "{e}");
        ids.push(e["id"].as_str().unwrap().to_string());
    }
    let (asha, ravi) = (ids[0].clone(), ids[1].clone());
    roster(&w, &w.admin, "key-sw-ro1", json!([{ "employee_id": asha, "work_date": d, "shift_id": mor }, { "employee_id": ravi, "work_date": d, "shift_id": eve }])).await;
    let asha_t = employee_user(&w, &asha, "Asha", &["employee"]).await;
    let ravi_t = employee_user(&w, &ravi, "Ravi", &["employee"]).await;
    let kavi_t = employee_user(&w, &kavi_id, "Kavi", &["employee", "manager"]).await;
    let (asha_t, ravi_t, kavi_t) = (Some(asha_t.as_str()), Some(ravi_t.as_str()), Some(kavi_t.as_str()));
    let act = |id: &str, what: &str| format!("{reqs}/{id}/{what}");
    let swap = json!({ "work_date": d, "kind": "swap", "colleague_employee_id": ravi, "reason": "Family function" });

    // SH-12 is off by default (AC-AT-05.3).
    let (s, _, b) = ask(&w, asha_t, "key-sw-0001", swap.clone()).await;
    assert_eq!((s, b["code"].as_str(), b["meta"]["setting"].as_str()), (StatusCode::FORBIDDEN, Some("feature_disabled"), Some("SH-12")), "{b}");
    sqlx::query("INSERT INTO app.policy_set_versions (org_id, policy_set_id, valid_from, state, settings)
                 SELECT $1, id, DATE '2026-01-01', 'approved', '{\"shift_requests_enabled\": true}' FROM app.policy_sets WHERE org_id = $1 AND is_default")
        .bind(w.org).execute(&w.pool).await.unwrap();

    // Only future dates; a change to the shift you already have is refused.
    let (s, _, b) = ask(&w, asha_t, "key-sw-0002", json!({ "work_date": past, "kind": "change", "shift_id": eve, "reason": "x" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("date_not_future")), "{b}");
    let (s, _, b) = ask(&w, asha_t, "key-sw-0003", json!({ "work_date": d, "kind": "change", "shift_id": mor, "reason": "x" })).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("shift_not_allowed")), "{b}");

    // The form's options: Asha (MOR) may change to EVE or swap with Ravi (EVE).
    let (s, _, o) = call(&w, Method::GET, &format!("/v1/attendance/me/shift-options?date={d}"), asha_t, &[], None).await;
    assert_eq!((s, o["current"]["code"].as_str(), o["shifts"][0]["code"].as_str(), o["colleagues"][0]["full_name"].as_str()), (StatusCode::OK, Some("MOR"), Some("EVE"), Some("Ravi")), "{o}");

    // SW1 swap: pending, Ravi (the colleague) is told and sees it.
    let (s, _, r) = ask(&w, asha_t, "key-sw-0004", swap.clone()).await;
    assert_eq!((s, r["state"].as_str(), r["wanted_shift"]["code"].as_str(), r["colleague_shift"]["code"].as_str()), (StatusCode::CREATED, Some("pending"), Some("EVE"), Some("MOR")), "{r}");
    let rid = r["id"].as_str().unwrap().to_string();
    let told: i64 = sqlx::query_scalar("SELECT count(*) FROM app.in_app_notifications WHERE org_id = $1 AND event IN ('ShiftSwapProposed', 'ShiftRequestSubmitted')")
        .bind(w.org).fetch_one(&w.pool).await.unwrap();
    assert_eq!(told, 2);
    let (_, _, seen) = call(&w, Method::GET, reqs, ravi_t, &[], None).await;
    assert_eq!(seen["items"][0]["id"].as_str(), Some(rid.as_str()), "{seen}");

    // SW3: the requester can't approve; the manager sees it awaiting them, and one approval writes both entries (ATQ-12).
    let (s, _, _) = call(&w, Method::POST, &act(&rid, "approve"), asha_t, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, _, waiting) = call(&w, Method::GET, &format!("{reqs}?awaiting_me=true"), kavi_t, &[], None).await;
    assert_eq!(waiting["items"].as_array().map(Vec::len), Some(1), "{waiting}");
    let (s, _, b) = call(&w, Method::POST, &act(&rid, "approve"), kavi_t, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("approved")), "{b}");
    let (a, r2) = (day(&w, &asha, &d).await, day(&w, &ravi, &d).await);
    assert_eq!((a["code"].as_str(), a["source"].as_str(), r2["code"].as_str()), (Some("EVE"), Some("swap"), Some("MOR")), "{a} {r2}");
    let (s, _, b) = call(&w, Method::POST, &act(&rid, "reject"), kavi_t, &[("if-match", "\"2\"")], Some(json!({ "reason": "x" }))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("not_pending")), "{b}");

    // A change: rejection needs a reason; another one is withdrawn by the requester.
    let (s, _, c) = ask(&w, asha_t, "key-sw-0005", json!({ "work_date": d, "kind": "change", "shift_id": mor, "reason": "Back to mornings" })).await;
    assert_eq!(s, StatusCode::CREATED, "{c}");
    let cid = c["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &act(&cid, "reject"), kavi_t, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["errors"][0]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("required")), "{b}");
    let (s, _, b) = call(&w, Method::POST, &act(&cid, "reject"), kavi_t, &[("if-match", "\"1\"")], Some(json!({ "reason": "Short of evening staff" }))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("rejected")), "{b}");
    let (_, _, c2) = ask(&w, asha_t, "key-sw-0006", json!({ "work_date": d, "kind": "change", "shift_id": mor, "reason": "Again" })).await;
    let c2 = c2["id"].as_str().unwrap().to_string();
    let (s, _, b) = call(&w, Method::POST, &act(&c2, "withdraw"), asha_t, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["state"].as_str()), (StatusCode::OK, Some("withdrawn")), "{b}");

    // A swap whose roster changed before the decision can't be approved.
    let (s, _, r3) = ask(&w, asha_t, "key-sw-0007", swap).await;
    assert_eq!(s, StatusCode::CREATED, "{r3}");
    roster(&w, &w.admin, "key-sw-ro2", json!([{ "employee_id": ravi, "work_date": d, "shift_id": eve }])).await;
    let (s, _, b) = call(&w, Method::POST, &act(r3["id"].as_str().unwrap(), "approve"), kavi_t, &[("if-match", "\"1\"")], Some(json!({}))).await;
    assert_eq!((s, b["code"].as_str()), (StatusCode::CONFLICT, Some("roster_changed")), "{b}");
}
