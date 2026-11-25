//! Types shared by every crate. Error codes match docs/api/ERROR_CATALOG.md exactly.

use serde::Serialize;

/// One field-level problem inside a `validation_failed` (or single-rule) error.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

impl FieldError {
    pub fn new(field: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self { field: field.into(), code, message: message.into(), meta: None }
    }
}

/// An error the API can return. `code` is the stable catalogue code; `status` its HTTP status.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct AppError {
    pub status: u16,
    pub code: &'static str,
    pub title: &'static str,
    pub detail: String,
    pub errors: Vec<FieldError>,
    pub meta: Option<serde_json::Value>,
}

impl AppError {
    pub fn new(status: u16, code: &'static str, title: &'static str, detail: impl Into<String>) -> Self {
        Self { status, code, title, detail: detail.into(), errors: Vec::new(), meta: None }
    }

    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self.meta = Some(meta);
        self
    }

    /// 422 with every field problem listed (AC-EM-01.2: each missing field is named).
    pub fn validation(errors: Vec<FieldError>) -> Self {
        let detail = format!("{} field(s) need attention", errors.len());
        Self { errors, ..Self::new(422, "validation_failed", "Some fields are not valid", detail) }
    }

    pub fn not_found() -> Self {
        Self::new(404, "not_found", "Not found", "No such record")
    }

    pub fn forbidden() -> Self {
        Self::new(403, "forbidden", "Not allowed", "You don't have permission for this action")
    }

    pub fn conflict(code: &'static str, title: &'static str, detail: impl Into<String>) -> Self {
        Self::new(409, code, title, detail)
    }

    pub fn unprocessable(code: &'static str, title: &'static str, detail: impl Into<String>) -> Self {
        Self::new(422, code, title, detail)
    }

    pub fn internal() -> Self {
        Self::new(500, "internal_error", "Something went wrong", "Nothing was changed")
    }
}

pub type AppResult<T> = Result<T, AppError>;
