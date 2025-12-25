//! Database pool configuration and migrations

use anyhow::Result;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::migrate::Migrator;
use std::path::Path;
use std::time::Duration;
use tracing::{info, warn};

/// Create an optimized SQLite connection pool
pub async fn create_optimized_pool(database_url: &str) -> Result<SqlitePool> {
    SqlitePoolOptions::new()
        // SQLite is single-writer, but can have multiple readers
        .max_connections(10)
        // Keep some connections ready
        .min_connections(2)
        // Don't wait too long for a connection
        .acquire_timeout(Duration::from_secs(10))
        // Recycle connections periodically
        .max_lifetime(Duration::from_secs(1800)) // 30 minutes
        // Close idle connections after a while
        .idle_timeout(Duration::from_secs(600)) // 10 minutes
        .connect(database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))
}

/// Run database migrations from a directory
///
/// Applies any pending migrations from the specified directory.
/// Uses SQLite's `_sqlx_migrations` table to track applied migrations.
pub async fn run_migrations(pool: &SqlitePool, migrations_path: &Path) -> Result<()> {
    if !migrations_path.exists() {
        warn!("Migrations directory not found: {}", migrations_path.display());
        return Ok(());
    }

    let migrator = Migrator::new(migrations_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load migrations: {}", e))?;

    let pending = migrator.migrations.iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .count();

    if pending > 0 {
        info!("Running {} pending migrations...", pending);
    }

    migrator
        .run(pool)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {}", e))?;

    info!("Migrations complete");
    Ok(())
}

/// Get current schema version (number of applied migrations)
pub async fn get_schema_version(pool: &SqlitePool) -> Result<i64> {
    let result: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1"
    )
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|(c,)| c).unwrap_or(0))
}
