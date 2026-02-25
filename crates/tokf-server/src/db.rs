use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Creates a connection pool with sensible production defaults.
///
/// # Errors
///
/// Returns an error if the database URL is invalid or a connection cannot be
/// established within the acquire timeout.
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
        .context("failed to connect to database")
}

/// Applies all pending migrations to the pool.
///
/// # Errors
///
/// Returns an error if the migrations table cannot be created or a migration
/// fails to apply.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    let mut migrator = sqlx::migrate!("./migrations");
    // CockroachDB does not support pg_advisory_lock(), so disable locking.
    // Fly's release_command runs on a single machine, so there is no
    // concurrency risk.
    migrator.set_locking(false);
    migrator
        .run(pool)
        .await
        .context("failed to apply database migrations")?;
    tracing::info!("database migrations applied");
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[tokio::test]
    async fn create_pool_fails_on_invalid_url() {
        let result = create_pool("not-a-valid-url").await;
        assert!(result.is_err(), "expected error for invalid URL");
    }

    #[tokio::test]
    async fn run_migrations_fails_when_db_unreachable() {
        // nonexistent-host.invalid (RFC 2606) guarantees NXDOMAIN, so DNS fails
        // immediately. The short acquire_timeout caps any unexpected delay.
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy("postgres://tokf:tokf@nonexistent-host.invalid:5432/tokf")
            .expect("lazy pool creation should not fail");
        let result = run_migrations(&pool).await;
        assert!(result.is_err(), "expected error when DB is unreachable");
    }
}
