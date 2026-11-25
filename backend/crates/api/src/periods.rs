//! Attendance periods and lock units (Package 2, C12). S0: the lock check (PeriodLockQuery, BR-AT-17)
//! that Package 1 left as a stub. The period routes (L1–L9) join this module in S9.

use chrono::NaiveDate;
use serde_json::json;
use shared::AppError;
use uuid::Uuid;

use crate::employees::Tx;
use crate::error::ApiResult;

/// The lock unit that freezes `employee` on `date`, if any: the unit of the employee's payroll group and
/// master branch on that date (DEC-49), for the period containing it, in any state but open.
pub(crate) async fn lock_on(tx: &mut Tx, employee: Uuid, date: NaiveDate) -> ApiResult<Option<(Uuid, String)>> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT lu.id, lu.state::text
         FROM app.employee_job_versions v
         JOIN app.attendance_periods p ON p.payroll_group_id = v.payroll_group_id AND p.period @> $2::date
         JOIN app.lock_units lu ON lu.period_id = p.id AND lu.branch_id = v.master_branch_id
         WHERE v.employee_id = $1 AND v.state = 'active' AND v.valid @> $2::date AND lu.state <> 'open'",
    )
    .bind(employee)
    .bind(date)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

/// Refuses a change dated inside a locked period (BR-EM-06): 409 `date_in_locked_period`.
pub(crate) async fn refuse_if_locked(tx: &mut Tx, employee: Uuid, date: NaiveDate) -> ApiResult<()> {
    match lock_on(tx, employee, date).await? {
        Some((unit, state)) => Err(AppError::conflict(
            "date_in_locked_period",
            "Date is in a locked period",
            format!("{date} is in a period that is {state}; step it back first"),
        )
        .with_meta(json!({ "lock_unit_id": unit, "state": state }))
        .into()),
        None => Ok(()),
    }
}

/// The earliest lock unit, not open, whose period ends after `from`: for one branch, or any branch when None.
/// Used by organisation-wide schedule changes dated from `from` (shift versions), which touch every later date.
pub(crate) async fn locked_from(tx: &mut Tx, branch: Option<Uuid>, from: NaiveDate) -> ApiResult<Option<(Uuid, String)>> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT lu.id, lu.state::text FROM app.lock_units lu JOIN app.attendance_periods p ON p.id = lu.period_id
         WHERE lu.state <> 'open' AND upper(p.period) > $1 AND ($2::uuid IS NULL OR lu.branch_id = $2)
         ORDER BY lower(p.period) LIMIT 1",
    )
    .bind(from)
    .bind(branch)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

/// Package 2 refusal for schedules and settings on a locked date (API_CONTRACTS P2 §1.3): 409 `date_locked`.
pub(crate) fn date_locked((unit, state): (Uuid, String), date: NaiveDate) -> crate::error::ApiError {
    AppError::conflict("date_locked", "Date is locked", format!("{date} is in a period that is {state}; step it back first"))
        .with_meta(json!({ "lock_unit_id": unit, "state": state }))
        .into()
}
