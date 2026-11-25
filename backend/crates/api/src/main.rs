use anyhow::Context;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    // Lazy: the server starts even if the database is briefly unreachable; /health/ready reports it.
    let pool = PgPoolOptions::new().max_connections(10).connect_lazy(&url)?;
    let jwt_secret = std::env::var("JWT_SECRET").context("JWT_SECRET is not set")?;
    anyhow::ensure!(jwt_secret.len() >= 32, "JWT_SECRET must be at least 32 characters");
    let totp_secret = std::env::var("TOTP_ENCRYPTION_KEY").context("TOTP_ENCRYPTION_KEY is not set")?;
    anyhow::ensure!(totp_secret.len() >= 32, "TOTP_ENCRYPTION_KEY must be at least 32 characters");
    let mut state = api::AppState::new(pool, &jwt_secret, &totp_secret);
    // Optional: without it the /internal job routes refuse every call.
    if let Ok(t) = std::env::var("JOBS_TOKEN") {
        anyhow::ensure!(t.len() >= 32, "JOBS_TOKEN must be at least 32 characters");
        state = state.with_jobs_token(&t);
    }
    // Optional: without ZEPTOMAIL_TOKEN nothing is emailed and mail events wait in the outbox.
    if let Ok(token) = std::env::var("ZEPTOMAIL_TOKEN") {
        let from = std::env::var("MAIL_FROM").context("MAIL_FROM is required with ZEPTOMAIL_TOKEN")?;
        let app_url = std::env::var("APP_BASE_URL").context("APP_BASE_URL is required with ZEPTOMAIL_TOKEN")?;
        let url = std::env::var("ZEPTOMAIL_URL").unwrap_or_else(|_| api::mail::ZEPTOMAIL_URL.into());
        state = state.with_mailer(api::mail::Mailer::zepto(&url, &token, &from), &app_url);
    }
    // Uploaded files (bulk imports). Defaults to ./files.
    if let Ok(dir) = std::env::var("FILE_DIR") {
        state = state.with_files_dir(dir);
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("listening on {port}");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
