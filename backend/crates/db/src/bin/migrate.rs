//! Safe migrator. Refuses to touch a database whose name is not the one you expect, and only
//! changes anything with `--apply`.
//!
//!   cargo run -p db --bin migrate -- --expect-db postgres            # show status only
//!   cargo run -p db --bin migrate -- --expect-db postgres --apply    # apply pending migrations
//!
//! Reads DATABASE_URL from the environment or backend/.env. The URL is never printed.

use anyhow::{bail, Context};
use sqlx::{postgres::PgPoolOptions, Row};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let expect_db = args
        .iter()
        .position(|a| a == "--expect-db")
        .and_then(|i| args.get(i + 1))
        .context("--expect-db <database name> is required")?;
    let apply = args.iter().any(|a| a == "--apply");

    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set (put it in backend/.env)")?;
    let pool = PgPoolOptions::new().max_connections(1).connect(&url).await.context("could not connect")?;

    let row = sqlx::query(
        "SELECT current_database() AS db, current_user::text AS usr, \
         inet_server_addr()::text AS host, version() AS ver",
    )
    .fetch_one(&pool)
    .await?;
    let db: String = row.get("db");
    let usr: String = row.get("usr");
    let host: Option<String> = row.get("host");
    println!("Connected: database={db} user={usr} host={}", host.unwrap_or_else(|| "?".into()));
    println!("Server:    {}", row.get::<String, _>("ver"));
    if &db != expect_db {
        bail!("refusing: connected to database '{db}', expected '{expect_db}'");
    }

    let applied: HashSet<i64> = match sqlx::query("SELECT version FROM _sqlx_migrations WHERE success")
        .fetch_all(&pool)
        .await
    {
        Ok(rows) => rows.iter().map(|r| r.get("version")).collect(),
        Err(_) => HashSet::new(), // table not created yet: nothing applied
    };
    for m in db::MIGRATOR.iter().filter(|m| m.migration_type.is_up_migration()) {
        let state = if applied.contains(&m.version) { "applied" } else { "pending" };
        println!("  {:04} {:<24} {state}", m.version, m.description);
    }

    if !apply {
        println!("Status only. Re-run with --apply to apply pending migrations.");
        return Ok(());
    }
    db::MIGRATOR.run(&pool).await.context("migration failed")?;
    println!("Done: all migrations applied.");
    Ok(())
}
