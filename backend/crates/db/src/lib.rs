//! Database access: embedded migrations and the tenant transaction (DATA_MODEL §2).
//! Queries use runtime `sqlx::query`, not the `query!` macros, so the build never needs a database.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub use sqlx::migrate::Migrator;

/// Migrations 0001–0008, compiled into the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Starts a transaction scoped to one organisation. Row-level security reads `app.org_id`
/// (and the audit trigger `app.user_id`); `is_local = true` clears both at commit or rollback,
/// so a pooled connection never carries one tenant into the next request.
/// It also switches to the `app_api` role for the transaction, so row-level security and the
/// column grants apply even when the login role owns the tables (as on Supabase dev).
pub async fn begin_tenant(
    pool: &PgPool,
    org_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id', $1, true), set_config('app.user_id', $2, true)")
        .bind(org_id.to_string())
        .bind(user_id.map(|u| u.to_string()).unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL ROLE app_api").execute(&mut *tx).await?;
    Ok(tx)
}
