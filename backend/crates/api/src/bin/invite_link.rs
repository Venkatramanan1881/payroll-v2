//! Dev only: prints the invitation link for an invited admin, for when email is off (no ZEPTOMAIL_TOKEN).
//! Needs DATABASE_URL, JWT_SECRET and TOTP_ENCRYPTION_KEY (backend/.env).
//!
//!   cargo run -p api --bin invite_link -- <admin email> [web base url, default http://localhost:3000]

use anyhow::Context;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let email = args.first().context("usage: invite_link <admin email> [base url]")?;
    let base = args.get(1).map(String::as_str).unwrap_or("http://localhost:3000");
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?).await?;
    let jwt = std::env::var("JWT_SECRET").context("JWT_SECRET is not set")?;
    let totp_key = std::env::var("TOTP_ENCRYPTION_KEY").context("TOTP_ENCRYPTION_KEY is not set")?;
    let org: Uuid = sqlx::query_scalar::<_, Option<Uuid>>("SELECT app.org_id_for_admin_email($1)")
        .bind(email)
        .fetch_one(&pool)
        .await?
        .context("no admin with that email")?;
    let mut tx = db::begin_tenant(&pool, org, None).await?;
    let user: Uuid = sqlx::query_scalar("SELECT id FROM app.users WHERE kind = 'admin' AND state = 'invited' AND lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(&mut *tx)
        .await?
        .context("that admin is not in the invited state")?;
    tx.commit().await?;
    let state = api::AppState::new(pool, &jwt, &totp_key);
    println!("{}/accept-invite?token={}", base.trim_end_matches('/'), api::auth::issue_invite_token(&state, user, org));
    Ok(())
}
