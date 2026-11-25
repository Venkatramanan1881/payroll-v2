//! HTTP API (Axum). Routes from docs/api/API_CONTRACTS are added module by module.

pub mod access;
pub mod at_imports;
pub mod at_reports;
pub mod auth;
pub mod ctx;
pub mod days;
pub mod employee_details;
pub mod employees;
pub mod error;
pub mod explorer;
pub mod fences;
pub mod holidays;
pub mod imports;
pub mod job_changes;
pub mod jobs;
pub mod mail;
pub mod notify;
pub mod org;
pub mod periods;
pub mod policies;
pub mod portal;
pub mod punches;
pub mod rate;
pub mod regularise;
pub mod reports;
pub mod roster;
pub mod shifts;
pub mod status;
pub mod vendor;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::HeaderValue;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use error::{ApiResult, CORRELATION_ID};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// HS256 key for access tokens (JWT_SECRET, at least 32 bytes).
    pub jwt_secret: std::sync::Arc<[u8]>,
    /// AES-256 key for admins' authenticator secrets: SHA-256 of TOTP_ENCRYPTION_KEY.
    pub totp_key: std::sync::Arc<[u8]>,
    /// SHA-256 of JOBS_TOKEN, the bearer token internal job calls must carry. None = job routes refuse everyone.
    pub jobs_token_hash: Option<std::sync::Arc<[u8]>>,
    pub mailer: mail::Mailer,
    /// Web app address for links in emails (APP_BASE_URL), without a trailing slash.
    pub app_url: std::sync::Arc<str>,
    /// Where uploaded files are kept (FILE_DIR). shortcut: local disk until Cloud Storage is set up.
    pub files_dir: std::sync::Arc<std::path::Path>,
    /// Sign-in attempt counters (rate::Limiter), shared by every request this process serves.
    pub limiter: std::sync::Arc<rate::Limiter>,
}

impl AppState {
    /// Derives the 32-byte TOTP key from any secret string of at least 32 characters.
    pub fn new(pool: PgPool, jwt_secret: &str, totp_secret: &str) -> Self {
        use sha2::Digest;
        let totp_key = sha2::Sha256::digest(totp_secret.as_bytes()).to_vec();
        Self {
            pool,
            jwt_secret: jwt_secret.as_bytes().into(),
            totp_key: totp_key.into(),
            jobs_token_hash: None,
            mailer: mail::Mailer::Off,
            app_url: "".into(),
            files_dir: std::path::Path::new("files").into(),
            limiter: Default::default(),
        }
    }

    pub fn with_mailer(mut self, mailer: mail::Mailer, app_url: &str) -> Self {
        self.mailer = mailer;
        self.app_url = app_url.trim_end_matches('/').into();
        self
    }

    pub fn with_files_dir(mut self, dir: impl AsRef<std::path::Path>) -> Self {
        self.files_dir = dir.as_ref().into();
        self
    }

    pub fn with_jobs_token(mut self, token: &str) -> Self {
        use sha2::Digest;
        self.jobs_token_hash = Some(sha2::Sha256::digest(token.as_bytes()).to_vec().into());
        self
    }
}

pub fn router(state: AppState) -> Router {
    // Sign-in routes: rate-limited per client IP (TSD §15).
    let sign_in = Router::new()
        .route("/v1/auth/employee/sign-in", post(auth::employee_sign_in))
        .route("/v1/auth/employee/otp/request", post(auth::otp_request))
        .route("/v1/auth/employee/otp/verify", post(auth::otp_verify))
        .route("/v1/auth/admin/sign-in", post(auth::admin_sign_in))
        .route("/v1/auth/admin/mfa", post(auth::admin_mfa))
        .route("/v1/auth/admin/accept-invite", post(auth::accept_invite))
        .route("/v1/auth/refresh", post(auth::refresh))
        .route("/v1/vendor/auth/sign-in", post(vendor::sign_in))
        .route("/v1/vendor/auth/mfa", post(vendor::mfa))
        .route_layer(middleware::from_fn_with_state(state.clone(), rate::per_ip));
    Router::new()
        .merge(sign_in)
        .route("/health", get(|| async { Json(serde_json::json!({ "status": "ok" })) }))
        .route("/health/ready", get(ready))
        .route("/v1/auth/employee/pin", post(auth::set_pin))
        .route("/v1/auth/sign-out", post(auth::sign_out))
        .route("/v1/me/access", get(auth::me_access))
        .route("/v1/me/profile", get(employees::me_profile))
        .route("/v1/me/contact", patch(employees::me_contact))
        .route("/internal/jobs/fan-out", post(jobs::fan_out))
        .route("/internal/jobs/status-due", post(jobs::status_due))
        .route("/internal/outbox/dispatch", post(jobs::outbox_dispatch))
        .route("/internal/jobs/bulk-import", post(jobs::bulk_import))
        .route("/internal/jobs/probation-reminders", post(jobs::probation_reminders))
        .route("/internal/jobs/cleanup", post(jobs::cleanup))
        .route("/internal/jobs/export", post(jobs::export))
        .route("/internal/jobs/recalculate", post(jobs::recalculate))
        .route("/internal/jobs/finalise", post(jobs::finalise))
        .route("/internal/jobs/attendance-sweep", post(jobs::attendance_sweep))
        .route("/internal/jobs/punch-import", post(jobs::attendance_import))
        .route("/internal/jobs/attendance-reminders", post(jobs::attendance_reminders))
        .route("/internal/jobs/attendance-cleanup", post(jobs::attendance_cleanup))
        .route("/internal/jobs/roster-import", post(jobs::attendance_import))
        .route("/v1/reports/r10", get(reports::r10))
        .route("/v1/reports/r11", get(reports::r11))
        .route("/v1/exports", post(reports::create_export))
        .route("/v1/exports/{id}", get(reports::get_export))
        .route("/v1/files/exports/{token}", get(reports::export_file))
        .route("/v1/audit", get(reports::audit))
        .route("/v1/employee-imports/template", get(imports::template))
        .route(
            "/v1/employee-imports",
            get(imports::list).post(imports::upload).layer(DefaultBodyLimit::max(imports::MAX_BYTES + 64 * 1024)),
        )
        .route("/v1/employee-imports/{id}", get(imports::get))
        .route("/v1/employee-imports/{id}/errors", get(imports::errors))
        .route("/v1/files/import-errors/{token}", get(imports::errors_file))
        .route("/v1/vendor/organisations", get(vendor::list).post(vendor::create))
        .route("/v1/vendor/organisations/{id}", patch(vendor::patch))
        .route("/v1/vendor/organisations/{id}/suspend", post(vendor::suspend))
        .route("/v1/vendor/organisations/{id}/reactivate", post(vendor::reactivate))
        .route("/v1/access/permissions", get(access::permissions))
        .route("/v1/access/roles", get(access::roles).post(access::role_create))
        .route("/v1/access/roles/{id}", get(access::role))
        .route("/v1/access/roles/{id}/permissions", put(access::role_permissions))
        .route("/v1/access/users", get(access::users).post(access::user_create))
        .route("/v1/access/users/{id}/roles", put(access::user_roles))
        .route("/v1/access/users/{id}/suspend", post(access::user_suspend))
        .route("/v1/access/users/{id}/reactivate", post(access::user_reactivate))
        .route("/v1/attendance/shifts", get(shifts::list).post(shifts::create))
        .route("/v1/attendance/shifts/{id}", patch(shifts::patch))
        .route("/v1/attendance/shifts/{id}/versions", get(shifts::versions).post(shifts::version_create))
        .route("/v1/attendance/shifts/{id}/versions/{vid}/discard", post(shifts::version_discard))
        .route("/v1/attendance/shifts/{id}/deactivate", post(shifts::deactivate))
        .route("/v1/attendance/shifts/{id}/activate", post(shifts::activate))
        .route("/v1/attendance/employees/{id}/shifts", put(shifts::employee_shifts))
        .route("/v1/attendance/roster", get(roster::view).put(roster::put))
        .route("/v1/attendance/roster/entries", delete(roster::clear))
        .route("/v1/attendance/roster/copy", post(roster::copy))
        .route("/v1/attendance/roster/rotation", post(roster::rotate))
        .route("/v1/attendance/holiday-calendars", get(holidays::list).post(holidays::create))
        .route("/v1/attendance/holiday-calendars/{id}", get(holidays::get))
        .route("/v1/attendance/holiday-calendars/{id}/holidays", post(holidays::holiday_add))
        .route("/v1/attendance/holiday-calendars/{id}/holidays/{hid}", patch(holidays::holiday_patch))
        .route("/v1/attendance/holiday-calendars/{id}/holidays/{hid}/remove", post(holidays::holiday_remove))
        .route("/v1/attendance/branches/{id}/holiday-calendars/{year}", put(holidays::branch_link))
        .route("/v1/attendance/me/optional-holidays", get(holidays::my_optional))
        .route("/v1/attendance/optional-holiday-picks", get(holidays::picks).post(holidays::pick))
        .route("/v1/attendance/optional-holiday-picks/{id}/approve", post(holidays::approve))
        .route("/v1/attendance/optional-holiday-picks/{id}/reject", post(holidays::reject))
        .route("/v1/attendance/optional-holiday-picks/{id}/withdraw", post(holidays::withdraw))
        .route("/v1/attendance/optional-holiday-picks/{id}/cancel", post(holidays::cancel))
        .route("/v1/attendance/policy-sets", get(policies::list).post(policies::create))
        .route("/v1/attendance/policy-sets/{id}", patch(policies::patch))
        .route("/v1/attendance/policy-sets/{id}/deactivate", post(policies::deactivate))
        .route("/v1/attendance/policy-sets/{id}/versions", get(policies::versions).post(policies::version_create))
        .route("/v1/attendance/policy-sets/{id}/versions/{vid}", get(policies::version_get).patch(policies::version_patch))
        .route("/v1/attendance/policy-sets/{id}/versions/{vid}/submit", post(policies::submit))
        .route("/v1/attendance/policy-sets/{id}/versions/{vid}/approve", post(policies::approve))
        .route("/v1/attendance/policy-sets/{id}/versions/{vid}/reject", post(policies::reject))
        .route("/v1/attendance/policy-sets/{id}/versions/{vid}/discard", post(policies::discard))
        .route("/v1/attendance/policy-settings/schema", get(policies::schema))
        .route("/v1/attendance/policy-assignments", get(policies::assignments).post(policies::assign))
        .route("/v1/attendance/policy-assignments/{id}/end", post(policies::assignment_end))
        .route("/v1/attendance/employees/{id}/policy-set", get(policies::employee_resolved).put(policies::employee_set))
        .route("/v1/attendance/branches/{id}/fences", get(fences::list).post(fences::create))
        .route("/v1/attendance/fences/{id}", patch(fences::patch))
        .route("/v1/attendance/fences/{id}/test", post(fences::test))
        .route("/v1/attendance/fences/{id}/activate", post(fences::activate))
        .route("/v1/attendance/fences/{id}/deactivate", post(fences::deactivate))
        .route("/v1/attendance/employees/{id}/device-binding/reset", post(fences::binding_reset))
        .route("/v1/attendance/me/punches", post(punches::punch))
        .route("/v1/attendance/me/punches/today", get(punches::today_status))
        .route("/v1/attendance/punches", get(punches::list))
        .route("/v1/attendance/punches/{id}/adjudications", post(punches::adjudicate))
        .route("/v1/attendance/punch-reviews", get(punches::reviews))
        .route("/v1/attendance/punch-reviews/{id}/approve", post(punches::approve))
        .route("/v1/attendance/punch-reviews/{id}/reject", post(punches::reject))
        .route("/v1/attendance/manual-attendance", post(punches::manual))
        .route("/v1/attendance/manual-attendance/sheet", get(punches::sheet))
        .route("/v1/attendance/manual-punches/{id}/void", post(punches::void))
        .route("/v1/attendance/days", get(days::list))
        .route("/v1/attendance/employees/{id}/days/recalculate", post(days::recalculate))
        .route("/v1/attendance/employees/{id}/days/{date}", get(days::get))
        .route("/v1/attendance/me/days", get(days::my_list))
        .route("/v1/attendance/me/days/{date}", get(days::my_get))
        .route("/v1/attendance/me/roster", get(days::my_roster))
        .route("/v1/attendance/me/attendance/features", get(days::features))
        .route(
            "/v1/attendance/punch-imports",
            get(at_imports::punch_list).post(at_imports::punch_upload).layer(DefaultBodyLimit::max(imports::MAX_BYTES + 64 * 1024)),
        )
        .route("/v1/attendance/punch-imports/json", post(at_imports::punch_json).layer(DefaultBodyLimit::max(imports::MAX_BYTES)))
        .route("/v1/attendance/punch-imports/{id}", get(at_imports::punch_get))
        .route("/v1/attendance/punch-imports/{id}/errors", get(at_imports::punch_errors))
        .route("/v1/attendance/roster-imports/template", get(at_imports::roster_template))
        .route(
            "/v1/attendance/roster-imports",
            get(at_imports::roster_list).post(at_imports::roster_upload).layer(DefaultBodyLimit::max(imports::MAX_BYTES + 64 * 1024)),
        )
        .route("/v1/attendance/roster-imports/{id}", get(at_imports::roster_get))
        .route("/v1/attendance/roster-imports/{id}/errors", get(at_imports::roster_errors))
        .route("/v1/files/attendance-import-errors/{token}", get(at_imports::errors_file))
        .route("/v1/attendance/reports/r1", get(at_reports::r1))
        .route("/v1/attendance/reports/r2", get(at_reports::r2))
        .route("/v1/attendance/reports/r3", get(at_reports::r3))
        .route("/v1/attendance/reports/r4", get(at_reports::r4))
        .route("/v1/attendance/reports/r5", get(at_reports::r5))
        .route("/v1/attendance/reports/r23", get(at_reports::r23))
        .route("/v1/attendance/reports/r26", get(at_reports::r26))
        .route("/v1/attendance/dashboard/attendance", get(at_reports::dashboard))
        .route("/v1/attendance/regularisations", get(regularise::list).post(regularise::raise))
        .route("/v1/attendance/regularisations/{id}", get(regularise::get))
        .route("/v1/attendance/regularisations/{id}/approve", post(regularise::approve))
        .route("/v1/attendance/regularisations/{id}/reject", post(regularise::reject))
        .route("/v1/attendance/regularisations/{id}/withdraw", post(regularise::withdraw))
        .route("/v1/attendance/regularisations/{id}/cancel", post(regularise::cancel))
        .route("/v1/attendance/employees/{id}/days/{date}/overrides", post(regularise::override_create))
        .route("/v1/attendance/day-overrides/{id}/void", post(regularise::override_void))
        .route("/v1/attendance/waivers", get(regularise::waivers).post(regularise::waiver_create))
        .route("/v1/attendance/waivers/{id}/withdraw", post(regularise::waiver_withdraw))
        .route("/v1/attendance/outage-regularisations", post(regularise::outage))
        .route("/v1/attendance/notification-settings", get(notify::settings).put(notify::settings_put))
        .route("/v1/attendance/me/notifications", get(notify::mine))
        .route("/v1/attendance/me/notifications/read-all", post(notify::read_all))
        .route("/v1/attendance/me/notifications/{id}/read", post(notify::read))
        .route("/v1/org/branches", get(org::branches).post(org::branch_create))
        .route("/v1/org/branches/{id}", patch(org::branch_patch))
        .route("/v1/org/branches/{id}/deactivate", post(org::branch_deactivate))
        .route("/v1/org/branches/{id}/activate", post(org::branch_activate))
        .route("/v1/org/lists/apply-template", post(org::apply_template))
        .route("/v1/org/lists/{list}", get(org::lists).post(org::list_create))
        .route("/v1/org/lists/{list}/{id}", patch(org::list_patch))
        .route("/v1/org/lists/{list}/{id}/deactivate", post(org::list_deactivate))
        .route("/v1/org/lists/{list}/{id}/activate", post(org::list_activate))
        .route("/v1/org/contractors", get(org::contractors).post(org::contractor_create))
        .route("/v1/org/contractors/{id}", patch(org::contractor_patch))
        .route("/v1/org/contractors/{id}/deactivate", post(org::contractor_deactivate))
        .route("/v1/org/contractors/{id}/activate", post(org::contractor_activate))
        .route("/v1/org/contractors/{id}/workers", get(org::contractor_workers))
        .route("/v1/org/payroll-groups", get(org::groups).post(org::group_create))
        .route("/v1/org/payroll-groups/{id}", patch(org::group_patch).delete(org::group_delete))
        .route("/v1/org/payroll-groups/{id}/assign", post(org::group_assign))
        .route("/v1/org/settings", get(org::settings))
        .route("/v1/org/settings/employee-code/preview", get(org::code_preview))
        .route("/v1/org/settings/{group}", put(org::settings_put))
        .route("/v1/org/custom-fields", get(org::custom_fields).post(org::custom_field_create))
        .route("/v1/org/custom-fields/{id}", patch(org::custom_field_patch))
        .route("/v1/org/custom-fields/{id}/deactivate", post(org::custom_field_deactivate))
        .route("/v1/employees", get(employees::list).post(employees::create))
        .route("/v1/employees/{id}", get(employees::get))
        .route("/v1/employees/{id}/personal", patch(employees::patch_personal))
        .route("/v1/employees/{id}/history", get(employee_details::history))
        .route("/v1/employees/{id}/devices", get(portal::devices))
        .route("/v1/employees/{id}/portal-access", post(portal::grant).delete(portal::remove))
        .route("/v1/employees/{id}/pin-reset", post(portal::pin_reset))
        .route("/v1/employees/{id}/unlock", post(portal::unlock))
        .route("/v1/employees/{id}/probation", patch(employee_details::probation))
        .route("/v1/employees/{id}/custom-fields", put(employee_details::custom_fields))
        .route("/v1/employees/{id}/enrolment-status", patch(employee_details::enrolment_status))
        .route("/v1/employees/{id}/rejoin", post(employee_details::rejoin))
        .route("/v1/employees/{id}/job-changes", post(job_changes::change))
        .route("/v1/employees/{id}/job-changes/{version_id}/cancel", post(job_changes::cancel))
        .route("/v1/employees/{id}/status-requests", post(status::raise))
        .route("/v1/status-requests", get(status::list))
        .route("/v1/status-requests/{id}", get(status::get))
        .route("/v1/status-requests/{id}/approve", post(status::approve))
        .route("/v1/status-requests/{id}/reject", post(status::reject))
        .route("/v1/status-requests/{id}/withdraw", post(status::withdraw))
        .route("/v1/status-requests/{id}/cancel", post(status::cancel))
        .layer(middleware::from_fn(correlation))
        .with_state(state)
}

/// Readiness: the database answers.
async fn ready(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query("SELECT 1").execute(&s.pool).await?;
    Ok(Json(serde_json::json!({ "status": "ready" })))
}

/// Every response carries X-Correlation-Id (§1.4); a caller-supplied one is kept if it looks sane.
async fn correlation(req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get("x-correlation-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| (8..=100).contains(&v.len()) && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut res = CORRELATION_ID.scope(id.clone(), next.run(req)).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-correlation-id", v);
    }
    res
}
