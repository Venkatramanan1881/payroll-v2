//! Pure business rules for Package 1. No database or network calls (TSD §3.3), so the API,
//! jobs and tests all run the same code.

pub mod day;
pub mod employee_code;
pub mod pin;
pub mod policy;
pub mod punches;
pub mod reporting;
pub mod settings;
pub mod shifts;
pub mod status;
pub mod templates;
