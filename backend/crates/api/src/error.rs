//! AppError → RFC 9457 problem+json (API_CONTRACTS §1.4).

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use shared::AppError;

tokio::task_local! {
    /// Set per request by the correlation middleware so error bodies can carry it.
    pub static CORRELATION_ID: String;
}

/// Wrapper so this crate can implement axum's IntoResponse for the shared error type.
#[derive(Debug)]
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<sqlx::Error> for ApiError {
    /// A constraint the API also checks up front can still fire under a race; name it properly.
    fn from(e: sqlx::Error) -> Self {
        let constraint = e.as_database_error().and_then(|d| d.constraint()).unwrap_or_default();
        let taken = "Another current employee already has this value";
        let mapped = match constraint {
            "employees_mobile_current_uq" => Some(("mobile_in_use", "Mobile number in use", "mobile", taken)),
            "employees_org_id_employee_code_key" => Some(("employee_code_in_use", "Employee code in use", "employee_code", taken)),
            "job_versions_enrolment_unique" => Some(("enrolment_id_in_use", "Enrolment ID in use", "job.enrolment_id", taken)),
            "organisations_short_code_key" => Some(("short_code_in_use", "Short code in use", "short_code", "Another organisation has this short code")),
            "branches_org_id_short_code_key" => Some(("short_code_in_use", "Short code in use", "short_code", "Branch short code already exists")),
            "departments_code_uq" | "designations_code_uq" | "categories_code_uq" | "employee_types_code_uq"
            | "exit_reasons_code_uq" | "contractors_code_uq" | "shifts_code_uq" => Some(("code_in_use", "Code in use", "code", "This code is already used")),
            "payroll_groups_name_uq" | "roles_org_id_name_key" | "holiday_calendars_name_uq" | "policy_sets_name_uq" => Some(("name_in_use", "Name in use", "name", "This name is already used")),
            "policy_assignments_no_overlap" => Some(("assignment_overlaps", "Assignment overlaps", "valid_from", "This group already has a policy set for these dates")),
            "holidays_date_uq" => Some(("holiday_exists_on_date", "Holiday exists on date", "date", "The calendar already has a holiday on that date")),
            "optional_holiday_picks_live_uq" => Some(("already_picked", "Already picked", "holiday_id", "You have already picked this holiday")),
            "users_admin_email_uq" => Some(("email_in_use", "Email in use", "email", "This email already has an admin account")),
            "custom_field_definitions_label_uq" => Some(("label_in_use", "Label in use", "label", "This label is already used")),
            _ => None,
        };
        if let Some((code, title, field, detail)) = mapped {
            let mut err = AppError::conflict(code, title, detail);
            err.errors.push(shared::FieldError::new(field, code, title));
            return Self(err);
        }
        tracing::error!(error = %e, "database error");
        Self(AppError::internal())
    }
}

/// JSON request body. Unknown fields are refused (§1.1) and every problem is a problem+json reply.
/// Kept as a `Value` so idempotency can hash it before it is parsed into the route's type.
pub struct Body(pub serde_json::Value);

impl<S: Send + Sync> axum::extract::FromRequest<S> for Body {
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, ApiError> {
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|_| AppError::unprocessable("invalid_value", "Body not readable", "The request body could not be read"))?;
        let value = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::unprocessable("invalid_value", "Body is not valid JSON", e.to_string()))?;
        Ok(Body(value))
    }
}

/// Parses a body into the route's request type; unknown fields → `unknown_field`.
pub fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> ApiResult<T> {
    serde_json::from_value(v).map_err(|e| {
        let msg = e.to_string();
        let code = if msg.starts_with("unknown field") { "unknown_field" } else { "invalid_value" };
        AppError::unprocessable(code, "Request not valid", msg).into()
    })
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let e = self.0;
        let mut body = serde_json::json!({
            "type": format!("https://errors.payroll.app/{}", e.code),
            "title": e.title,
            "status": e.status,
            "code": e.code,
            "detail": e.detail,
        });
        if !e.errors.is_empty() {
            body["errors"] = serde_json::to_value(&e.errors).unwrap_or_default();
        }
        if let Some(meta) = e.meta {
            body["meta"] = meta;
        }
        if let Ok(id) = CORRELATION_ID.try_with(Clone::clone) {
            body["correlation_id"] = id.into();
        }
        let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut res = (status, body.to_string()).into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/problem+json"));
        if let Some(secs) = body["meta"]["retry_after"].as_u64() {
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        res
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use shared::FieldError;

    #[tokio::test]
    async fn validation_error_has_problem_shape() {
        let err = AppError::validation(vec![FieldError::new("department_id", "required", "Department is required")]);
        let res = CORRELATION_ID.scope("abc".into(), async { ApiError(err).into_response() }).await;
        assert_eq!(res.status(), 422);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/problem+json");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["type"], "https://errors.payroll.app/validation_failed");
        assert_eq!(v["code"], "validation_failed");
        assert_eq!(v["errors"][0]["field"], "department_id");
        assert_eq!(v["correlation_id"], "abc");
    }
}
