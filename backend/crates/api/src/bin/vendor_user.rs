//! Adds a vendor console user (there is no route for it). Reads the password from stdin, prints the
//! authenticator setup once. Needs DATABASE_URL, JWT_SECRET and TOTP_ENCRYPTION_KEY (backend/.env).
//!
//!   cargo run -p api --bin vendor_user -- <email> "<display name>"

use anyhow::Context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [email, name] = args.as_slice() else {
        anyhow::bail!("usage: vendor_user <email> <display name>");
    };
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?).await?;
    let jwt = std::env::var("JWT_SECRET").context("JWT_SECRET is not set")?;
    let totp_key = std::env::var("TOTP_ENCRYPTION_KEY").context("TOTP_ENCRYPTION_KEY is not set")?;
    let state = api::AppState::new(pool, &jwt, &totp_key);
    eprint!("Password (12+ characters): ");
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    let (id, secret, uri) = api::vendor::create_vendor_user(&state, email, name, password.trim_end_matches(['\r', '\n']).to_string())
        .await
        .map_err(|e| anyhow::anyhow!("{}: {}", e.0.code, e.0.detail))?;
    println!("Vendor user {id} created.\nAdd to an authenticator app:\n  secret: {secret}\n  {uri}");
    Ok(())
}
