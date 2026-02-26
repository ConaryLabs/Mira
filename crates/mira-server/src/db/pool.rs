// db/pool.rs
// Async connection pool using deadpool-sqlite
//
// # Async Database Access Patterns
//
// ## Preferred Pattern: pool.run()
// Use `pool.run()` for MCP tool handlers — it handles error conversion to
// `MiraError` automatically:
// ```ignore
// let result = ctx.pool()
//     .run(move |conn| some_function(conn))
//     .await?;
// ```
//
// ## Lower-Level: pool.interact()
// Use `pool.interact()` when you need `anyhow::Result` instead of `MiraError`,
// e.g. in migrations, background tasks, or internal helpers:
// ```ignore
// let result = ctx.pool().interact(move |conn| {
//     some_sync_function(conn, arg1, arg2)
// }).await?;
// ```
//
// ## Common Pitfalls
//
// 1. **Don't block the async runtime**: Always use `pool.run()` or
//    `pool.interact()` for database access.
//
// 2. **Type inference**: Rust needs help inferring types for closures. If you get
//    "type annotations needed", add explicit types to the return value:
//    `Ok::<_, rusqlite::Error>(result)`
//
// 3. **Capturing variables**: Move semantics can be tricky. Clone `Arc` values
//    before the closure to avoid lifetime issues.
//
// 4. **In-memory testing**: Use shared cache URI (`file:memdb_xxx?mode=memory&cache=shared`)
//    so multiple pool connections share the same database state.

use crate::error::MiraError;
use crate::utils::path_to_string;
use anyhow::{Context, Result};
use deadpool_sqlite::{Config, Hook, Pool, Runtime};
use rusqlite::Connection;
use sqlite_vec::sqlite3_vec_init;
use std::path::{Path, PathBuf};
use std::sync::Once;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Registers sqlite-vec extension globally (once per process).
/// Must be called before any SQLite connections are opened.
static SQLITE_VEC_INIT: Once = Once::new();

#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn ensure_sqlite_vec_registered() {
    SQLITE_VEC_INIT.call_once(|| {
        // SAFETY: sqlite3_vec_init has the signature expected by sqlite3_auto_extension
        // (i.e. fn(*mut sqlite3, *mut *const c_char, *const sqlite3_api_routines) -> c_int).
        // The transmute converts the fn-pointer to the Option<extern "C" fn()> that
        // sqlite3_auto_extension accepts. This is the standard pattern for registering
        // SQLite extensions via the Rust FFI; the function pointer remains valid for the
        // lifetime of the process since it points to a statically-linked symbol.
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }
        tracing::debug!("sqlite-vec extension registered globally");
    });
}

/// Check if a rusqlite error is SQLITE_BUSY or SQLITE_LOCKED.
///
/// SQLITE_BUSY ("database is locked") occurs with file-based databases under write contention.
/// SQLITE_LOCKED ("database table is locked") occurs with shared-cache in-memory databases
/// when another connection holds a write lock on the same table.
fn is_rusqlite_contention(err: &rusqlite::Error) -> bool {
    use rusqlite::ffi;
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            ffi::Error {
                code: ffi::ErrorCode::DatabaseBusy | ffi::ErrorCode::DatabaseLocked,
                ..
            },
            _,
        )
    )
}

/// Check if a MiraError wraps a SQLite contention error.
fn is_mira_contention(err: &MiraError) -> bool {
    match err {
        MiraError::Db(rusqlite_err) => is_rusqlite_contention(rusqlite_err),
        _ => false,
    }
}

/// Check if an anyhow::Error chain contains a SQLite contention error.
///
/// Walks the error chain looking for rusqlite::Error or MiraError::Db variants
/// with SQLITE_BUSY or SQLITE_LOCKED codes.
fn is_sqlite_contention(err: &anyhow::Error) -> bool {
    // Check direct downcast to rusqlite::Error
    if let Some(rusqlite_err) = err.downcast_ref::<rusqlite::Error>() {
        return is_rusqlite_contention(rusqlite_err);
    }
    // Check if wrapped in MiraError::Db
    if let Some(mira_err) = err.downcast_ref::<MiraError>() {
        return is_mira_contention(mira_err);
    }
    false
}

/// Retry delays for SQLite contention backoff (100ms, 500ms, 2s).
const RETRY_DELAYS: [std::time::Duration; 3] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(500),
    std::time::Duration::from_millis(2000),
];

/// Generic retry-with-backoff for async operations that may encounter SQLite contention.
///
/// Calls `op` up to `RETRY_DELAYS.len() + 1` times, sleeping between retries when
/// `is_retryable` returns true for the error.
async fn retry_with_backoff<F, Fut, R, E>(
    mut op: F,
    is_retryable: impl Fn(&E) -> bool,
) -> Result<R, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<R, E>>,
    E: std::fmt::Display,
{
    for (attempt, delay) in RETRY_DELAYS.iter().enumerate() {
        match op().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if is_retryable(&e) {
                    tracing::warn!(
                        "SQLite contention on attempt {}/{}, retrying in {:?}",
                        attempt + 1,
                        RETRY_DELAYS.len(),
                        delay
                    );
                    tokio::time::sleep(*delay).await;
                } else {
                    return Err(e);
                }
            }
        }
    }

    // Final attempt (no retry after this)
    op().await
}

/// Database pool wrapper with sqlite-vec support and per-connection setup.
///
/// Connection pool that scales for concurrent access.
pub struct DatabasePool {
    pool: Pool,
    path: Option<PathBuf>,
    /// URI for in-memory databases (used to share state in tests)
    memory_uri: Option<String>,
}

/// Whether to run main or code-specific migrations.
enum DbKind {
    Main,
    Code,
}

/// Whether to use a file path or shared in-memory URI.
enum DbStorage {
    File(PathBuf),
    InMemory { label: &'static str },
}

impl DatabasePool {
    /// Open a pooled database at the given path.
    pub async fn open(path: &Path) -> Result<Self> {
        Self::open_internal(DbStorage::File(path.to_path_buf()), DbKind::Main).await
    }

    /// Open a lightweight pool optimized for hooks.
    ///
    /// Hooks are short-lived, single-threaded processes that each open their own
    /// pool. This method reduces overhead by:
    /// - Using `max_size(1)` — hooks only need one connection
    /// - Skipping migrations if the DB file already exists (the MCP server is
    ///   responsible for running migrations via `mira serve`)
    ///
    /// Falls back to a full `open()` if the DB file doesn't exist (first run).
    pub async fn open_hook(path: &Path) -> Result<Self> {
        // If the DB file doesn't exist yet, fall back to full open (runs migrations)
        if !path.exists() {
            return Self::open(path).await;
        }

        ensure_sqlite_vec_registered();
        ensure_parent_directory(path)?;

        let conn_str = path_to_string(path);
        let hook = make_file_post_create_hook(path.to_path_buf());

        let cfg = Config::new(&conn_str);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder")?
            .max_size(1)
            .post_create(hook)
            .build()
            .context("Failed to build connection pool")?;

        Ok(Self {
            pool,
            path: Some(path.to_path_buf()),
            memory_uri: None,
        })
    }

    /// Open a pooled database for the code index at the given path.
    ///
    /// Runs code-specific migrations instead of the main schema migrations.
    /// The code database holds: code_symbols, call_graph, imports,
    /// codebase_modules, vec_code, code_fts, and pending_embeddings.
    pub async fn open_code_db(path: &Path) -> Result<Self> {
        Self::open_internal(DbStorage::File(path.to_path_buf()), DbKind::Code).await
    }

    /// Open a pooled in-memory database for the code index (for tests).
    pub async fn open_code_db_in_memory() -> Result<Self> {
        Self::open_internal(
            DbStorage::InMemory {
                label: "memdb_code",
            },
            DbKind::Code,
        )
        .await
    }

    /// Open a pooled in-memory database.
    ///
    /// Uses a shared cache URI so all connections access the same in-memory database.
    /// This is critical for tests - without shared cache, each connection would get
    /// its own separate in-memory database.
    pub async fn open_in_memory() -> Result<Self> {
        Self::open_internal(DbStorage::InMemory { label: "memdb" }, DbKind::Main).await
    }

    /// Internal constructor shared by all open variants.
    ///
    /// 1. Registers sqlite-vec extension globally (if not already done)
    /// 2. Creates the pool with appropriate hooks (file permissions or in-memory setup)
    /// 3. Runs schema migrations (main or code) on a dedicated connection
    async fn open_internal(storage: DbStorage, kind: DbKind) -> Result<Self> {
        ensure_sqlite_vec_registered();

        let (conn_str, path, memory_uri, hook) = match storage {
            DbStorage::File(p) => {
                ensure_parent_directory(&p)?;
                let s = path_to_string(&p);
                let hook = make_file_post_create_hook(p.clone());
                (s, Some(p), None, hook)
            }
            DbStorage::InMemory { label } => {
                let uri = format!(
                    "file:{}_{:?}?mode=memory&cache=shared",
                    label,
                    uuid::Uuid::new_v4()
                );
                let hook = make_memory_post_create_hook();
                (uri.clone(), None, Some(uri), hook)
            }
        };

        let cfg = Config::new(&conn_str);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder")?
            .max_size(8)
            .post_create(hook)
            .build()
            .context("Failed to build connection pool")?;

        let db_pool = Self {
            pool,
            path,
            memory_uri,
        };

        // Backup before migrations (file-based DBs only)
        if let Some(ref db_path) = db_pool.path
            && let Err(e) = Self::backup_before_migration(db_path)
        {
            tracing::warn!("Pre-migration backup failed (continuing anyway): {}", e);
        }

        match kind {
            DbKind::Main => db_pool.run_migrations().await?,
            DbKind::Code => db_pool.run_code_migrations().await?,
        }

        Ok(db_pool)
    }

    /// Get the memory URI (for sharing state in tests)
    pub fn memory_uri(&self) -> Option<&str> {
        self.memory_uri.as_deref()
    }

    /// Run a closure with a connection from the pool.
    ///
    /// This is the primary API for database access. The closure runs on a
    /// blocking thread pool, so it won't block the async runtime.
    ///
    /// # Example
    /// ```ignore
    /// let result = pool.interact(|conn| {
    ///     conn.execute("INSERT INTO ...", params![...])?;
    ///     Ok(())
    /// }).await?;
    /// ```
    pub async fn interact<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self
            .pool
            .get()
            .await
            .context("Failed to get connection from pool")?;

        conn.interact(move |conn| f(conn))
            .await
            .map_err(|e| anyhow::anyhow!("interact failed: {e}"))?
    }

    /// Run a closure on a pooled connection, logging errors at debug but not propagating.
    /// Use for low-priority best-effort operations (heartbeats, behavior logging, telemetry).
    pub async fn try_interact<F, R>(&self, label: &str, f: F) -> Option<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let label = label.to_string();
        match self.interact(move |conn| f(conn)).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::debug!("{}: {}", label, e);
                None
            }
        }
    }

    /// Like `try_interact` but logs failures at warn level.
    /// Use for important best-effort operations (session close, exports, caching).
    pub async fn try_interact_warn<F, R>(&self, label: &str, f: F) -> Option<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let label = label.to_string();
        match self.interact(move |conn| f(conn)).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("{}: {}", label, e);
                None
            }
        }
    }

    /// Run a closure that may return a rusqlite::Error.
    ///
    /// Convenience wrapper for operations that return rusqlite::Result.
    pub async fn interact_raw<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.interact(move |conn| f(conn).map_err(Into::into)).await
    }

    /// Run a closure and return Result<T, String> for tool handlers.
    ///
    /// This is the preferred method for MCP tool implementations that need
    /// to return `Result<_, String>`. It handles all the error conversion
    /// boilerplate in one place.
    ///
    /// # Example
    /// ```ignore
    /// // Before (8 lines):
    /// let result = ctx.pool()
    ///     .interact(move |conn| {
    ///         some_function(conn).map_err(|e| anyhow::anyhow!("{}", e))
    ///     })
    ///     .await
    ///     .map_err(|e| e.to_string())?;
    ///
    /// // After (3 lines):
    /// let result = ctx.pool()
    ///     .run(move |conn| some_function(conn))
    ///     .await?;
    /// ```
    pub async fn run<F, R, E>(&self, f: F) -> Result<R, MiraError>
    where
        F: FnOnce(&Connection) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: Into<MiraError> + Send + 'static,
    {
        self.pool
            .get()
            .await
            .map_err(|e| MiraError::Other(format!("Failed to get connection: {}", e)))?
            .interact(move |conn| f(conn).map_err(Into::into))
            .await
            .map_err(|e| MiraError::Other(format!("Database error: {}", e)))?
    }

    /// Like [`run`](Self::run) but with retry on SQLite contention errors.
    ///
    /// Uses exponential backoff (100ms, 500ms, 2000ms) for up to 3 retries.
    /// Use this for critical writes that must not be lost (memory storage,
    /// session creation, goal updates). The closure must be `Clone` to
    /// support retries.
    pub async fn run_with_retry<F, R, E>(&self, f: F) -> Result<R, MiraError>
    where
        F: FnOnce(&Connection) -> Result<R, E> + Send + Clone + 'static,
        R: Send + 'static,
        E: Into<MiraError> + Send + 'static,
    {
        retry_with_backoff(
            || {
                let f_clone = f.clone();
                self.run(f_clone)
            },
            is_mira_contention,
        )
        .await
    }

    /// Run a closure with retry on SQLite contention errors.
    ///
    /// Uses exponential backoff (100ms, 500ms, 2000ms) for up to 3 retries.
    /// Like [`run_with_retry`](Self::run_with_retry) but returns `anyhow::Result`.
    pub async fn interact_with_retry<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + Clone + 'static,
        R: Send + 'static,
    {
        retry_with_backoff(
            || {
                let f_clone = f.clone();
                self.interact(f_clone)
            },
            |e: &anyhow::Error| is_sqlite_contention(e),
        )
        .await
    }

    /// Get the database file path (None for in-memory).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Create a backup of the database before running migrations.
    /// Uses SQLite's VACUUM INTO for an atomic, consistent backup.
    /// Only backs up file-based databases (skips in-memory).
    /// Keeps at most one backup per database (overwrites previous).
    fn backup_before_migration(path: &Path) -> Result<()> {
        use std::fs;

        let backup_path = path.with_extension("db.pre-migration");

        // Only backup if the source DB file exists (not first run)
        if !path.exists() {
            return Ok(());
        }

        // Open a direct connection for the backup (pool isn't ready yet)
        let conn = rusqlite::Connection::open(path).context("Failed to open DB for backup")?;

        // Remove old backup if it exists (VACUUM INTO fails if target exists)
        let _ = fs::remove_file(&backup_path);

        conn.execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()])
            .context("Failed to create pre-migration backup")?;

        tracing::info!("Created pre-migration backup: {}", backup_path.display());
        Ok(())
    }

    /// Run main schema migrations.
    ///
    /// Called during pool creation, but can also be called explicitly if needed.
    async fn run_migrations(&self) -> Result<()> {
        self.interact(|conn| {
            super::schema::run_all_migrations(conn)?;
            if let Err(e) = conn.execute_batch("PRAGMA optimize") {
                tracing::debug!("PRAGMA optimize (main DB) skipped: {}", e);
            }
            Ok(())
        })
        .await
    }

    /// Run code database schema migrations.
    ///
    /// Called during code pool creation for the separate code index database.
    async fn run_code_migrations(&self) -> Result<()> {
        self.interact(|conn| {
            super::schema::run_code_migrations(conn)?;
            if let Err(e) = conn.execute_batch("PRAGMA optimize") {
                tracing::debug!("PRAGMA optimize (code DB) skipped: {}", e);
            }
            Ok(())
        })
        .await
    }

    /// Rebuild FTS5 search index from vec_code.
    pub async fn rebuild_fts(&self) -> Result<()> {
        self.interact(|conn| {
            super::schema::rebuild_code_fts(conn)?;
            Ok(())
        })
        .await
    }

    /// Rebuild FTS5 search index for a specific project.
    pub async fn rebuild_fts_for_project(&self, project_id: i64) -> Result<()> {
        self.interact(move |conn| {
            super::schema::rebuild_code_fts_for_project(conn, project_id)?;
            Ok(())
        })
        .await
    }

    /// Compact vec_code storage and VACUUM the database file.
    ///
    /// This reclaims wasted space from sqlite-vec's pre-allocated chunks.
    /// VACUUM runs in a separate `interact()` call to ensure no open
    /// statements or transactions interfere (VACUUM cannot run inside a tx).
    ///
    /// Note: VACUUM temporarily requires ~2x the current db file size in
    /// free disk space.
    pub async fn compact_code_db(&self) -> Result<super::index::CompactStats> {
        // Step 1: Compact vec_code (extract, drop, recreate, reinsert)
        let stats = self
            .interact(|conn| {
                super::index::compact_vec_code_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await?;

        // Step 2: VACUUM in a separate interact() to reclaim file space.
        // Non-fatal — the compact already succeeded, VACUUM is best-effort.
        if let Err(e) = self
            .interact(|conn| {
                conn.execute_batch("VACUUM")
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
        {
            tracing::warn!(
                "VACUUM after compact failed (non-fatal, space will be reused): {}",
                e
            );
        }

        Ok(stats)
    }

    /// Get pool status for monitoring.
    pub fn status(&self) -> PoolStatus {
        let status = self.pool.status();
        PoolStatus {
            size: status.size,
            available: status.available,
            waiting: status.waiting,
        }
    }
}

/// Pool status for monitoring.
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub size: usize,
    pub available: usize,
    pub waiting: usize,
}

/// Ensure parent directory exists with secure permissions (0o700 on Unix).
fn ensure_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(parent)?.permissions();
            perms.set_mode(0o700); // rwx------
            std::fs::set_permissions(parent, perms)?;
        }
        #[cfg(not(unix))]
        tracing::debug!(
            "Skipping directory permission restriction on non-Unix platform: {}",
            parent.display()
        );
    }
    Ok(())
}

/// Create a post_create hook for file-based databases.
///
/// Sets up PRAGMAs via `setup_connection` and restricts file permissions to 0o600.
fn make_file_post_create_hook(path: PathBuf) -> Hook {
    Hook::async_fn(move |conn, _metrics| {
        let path_for_perms = path.clone();
        Box::pin(async move {
            conn.interact(move |conn| {
                setup_connection(conn)?;

                #[cfg(unix)]
                if let Ok(metadata) = std::fs::metadata(&path_for_perms) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o600); // rw-------
                    if let Err(e) = std::fs::set_permissions(&path_for_perms, perms) {
                        tracing::warn!("Failed to set database file permissions to 0600: {}", e);
                    }
                }
                #[cfg(not(unix))]
                tracing::debug!(
                    "Skipping DB file permission restriction on non-Unix platform: {}",
                    path_for_perms.display()
                );

                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| {
                deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
            })?
            .map_err(|e| {
                deadpool_sqlite::HookError::Message(format!("connection setup failed: {e}").into())
            })
        })
    })
}

/// Create a post_create hook for in-memory databases.
///
/// Enables foreign keys and busy_timeout (WAL mode is not applicable to in-memory DBs).
fn make_memory_post_create_hook() -> Hook {
    Hook::async_fn(|conn, _metrics| {
        Box::pin(async move {
            conn.interact(|conn| {
                conn.execute_batch(
                    "PRAGMA foreign_keys=ON; \
                     PRAGMA busy_timeout=5000;",
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| {
                deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
            })?
            .map_err(|e| {
                deadpool_sqlite::HookError::Message(format!("connection setup failed: {e}").into())
            })
        })
    })
}

/// Configure a connection after it's created.
/// Called from the post_create hook.
fn setup_connection(conn: &Connection) -> rusqlite::Result<()> {
    // Enable WAL mode for better concurrency, foreign key enforcement,
    // busy timeout for write contention (5s retry window), and
    // NORMAL synchronous mode (safe with WAL, reduces fsync overhead).
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA foreign_keys=ON; \
         PRAGMA busy_timeout=5000; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA journal_size_limit=32768;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_in_memory() {
        let pool = DatabasePool::open_in_memory()
            .await
            .expect("Failed to open in-memory pool");

        // Test basic operation
        let result = pool
            .interact(|conn| {
                conn.execute(
                    "INSERT INTO projects (path, name) VALUES (?, ?)",
                    rusqlite::params!["/test/path", "test"],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .expect("Failed to insert");

        assert!(result > 0);

        // Verify from another connection in the pool (tests shared cache)
        let name: String = pool
            .interact(move |conn| {
                conn.query_row("SELECT name FROM projects WHERE id = ?", [result], |row| {
                    row.get(0)
                })
                .map_err(Into::into)
            })
            .await
            .expect("Failed to query");

        assert_eq!(name, "test");
    }

    #[tokio::test]
    async fn test_pool_status() {
        let pool = DatabasePool::open_in_memory()
            .await
            .expect("Failed to open pool");

        let status = pool.status();
        // Verify we can get status without panicking
        let _ = status;
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let pool = std::sync::Arc::new(
            DatabasePool::open_in_memory()
                .await
                .expect("Failed to open pool"),
        );

        // Spawn multiple concurrent tasks
        let mut handles = Vec::new();
        for i in 0..10 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                pool.interact(move |conn| {
                    conn.execute(
                        "INSERT INTO projects (path, name) VALUES (?, ?)",
                        rusqlite::params![format!("/test/{i}"), format!("project-{i}")],
                    )?;
                    Ok(())
                })
                .await
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap().expect("Insert failed");
        }

        // Verify all inserted
        let count: i64 = pool
            .interact(|conn| {
                conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
                    .map_err(Into::into)
            })
            .await
            .expect("Count failed");

        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn test_interact_with_retry_succeeds() {
        let pool = DatabasePool::open_in_memory()
            .await
            .expect("Failed to open pool");

        // A normal operation should succeed on first attempt
        let result = pool
            .interact_with_retry(|conn| {
                conn.execute(
                    "INSERT INTO projects (path, name) VALUES (?, ?)",
                    rusqlite::params!["/retry/test", "retry-test"],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .expect("interact_with_retry should succeed");

        assert!(result > 0);
    }

    #[tokio::test]
    async fn test_interact_with_retry_non_busy_error_fails_fast() {
        let pool = DatabasePool::open_in_memory()
            .await
            .expect("Failed to open pool");

        // A SQL error (not SQLITE_BUSY) should fail immediately without retrying
        let result = pool
            .interact_with_retry(|conn| {
                conn.execute(
                    "INSERT INTO nonexistent_table VALUES (?)",
                    rusqlite::params![1],
                )?;
                Ok(())
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_writes_with_busy_timeout() {
        let pool = std::sync::Arc::new(
            DatabasePool::open_in_memory()
                .await
                .expect("Failed to open pool"),
        );

        // Spawn 10 concurrent write operations - all should succeed
        // thanks to busy_timeout PRAGMA
        let mut handles = Vec::new();
        for i in 0..10 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                pool.interact_with_retry(move |conn| {
                    conn.execute(
                        "INSERT INTO projects (path, name) VALUES (?, ?)",
                        rusqlite::params![format!("/concurrent/{i}"), format!("project-{i}")],
                    )?;
                    Ok(())
                })
                .await
            }));
        }

        // All should succeed
        for handle in handles {
            handle.await.unwrap().expect("Concurrent write failed");
        }

        let count: i64 = pool
            .interact(|conn| {
                conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
                    .map_err(Into::into)
            })
            .await
            .expect("Count failed");

        assert_eq!(count, 10);
    }

    // ============================================================================
    // Contention detection tests
    // ============================================================================

    #[test]
    fn test_is_rusqlite_contention_busy() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        assert!(is_rusqlite_contention(&err));
    }

    #[test]
    fn test_is_rusqlite_contention_locked() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseLocked,
                extended_code: 6,
            },
            Some("database table is locked".to_string()),
        );
        assert!(is_rusqlite_contention(&err));
    }

    #[test]
    fn test_is_rusqlite_contention_other_error() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                extended_code: 19,
            },
            Some("UNIQUE constraint failed".to_string()),
        );
        assert!(!is_rusqlite_contention(&err));
    }

    #[test]
    fn test_is_mira_contention_db_busy() {
        let rusqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            None,
        );
        let mira_err = MiraError::Db(rusqlite_err);
        assert!(is_mira_contention(&mira_err));
    }

    #[test]
    fn test_is_mira_contention_other_variant() {
        assert!(!is_mira_contention(&MiraError::ProjectNotSet));
        assert!(!is_mira_contention(&MiraError::Other(
            "database is locked".to_string()
        )));
    }

    #[test]
    fn test_is_sqlite_contention_anyhow_with_rusqlite() {
        let rusqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            None,
        );
        let anyhow_err: anyhow::Error = rusqlite_err.into();
        assert!(is_sqlite_contention(&anyhow_err));
    }

    #[test]
    fn test_is_sqlite_contention_anyhow_with_mira_error() {
        let rusqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseLocked,
                extended_code: 6,
            },
            None,
        );
        let mira_err = MiraError::Db(rusqlite_err);
        let anyhow_err: anyhow::Error = mira_err.into();
        assert!(is_sqlite_contention(&anyhow_err));
    }

    #[test]
    fn test_is_sqlite_contention_anyhow_string_not_matched() {
        // String-only errors should NOT trigger contention detection
        // (no more fragile string matching)
        let anyhow_err = anyhow::anyhow!("database is locked");
        assert!(!is_sqlite_contention(&anyhow_err));
    }

    #[test]
    fn test_backup_before_migration() {
        // Create a temp directory with a test SQLite database
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test.db");

        // Create a real SQLite database with some data
        {
            let conn = rusqlite::Connection::open(&db_path).expect("Failed to create test DB");
            conn.execute_batch(
                "CREATE TABLE test_table (id INTEGER PRIMARY KEY, value TEXT);
                 INSERT INTO test_table (value) VALUES ('hello');",
            )
            .expect("Failed to populate test DB");
        }

        // Run backup
        DatabasePool::backup_before_migration(&db_path).expect("Backup should succeed");

        // Verify backup file exists
        let backup_path = db_path.with_extension("db.pre-migration");
        assert!(backup_path.exists(), "Backup file should exist");

        // Verify backup contains valid SQLite data
        {
            let conn = rusqlite::Connection::open(&backup_path)
                .expect("Backup should be a valid SQLite DB");
            let value: String = conn
                .query_row("SELECT value FROM test_table WHERE id = 1", [], |row| {
                    row.get(0)
                })
                .expect("Should be able to read from backup");
            assert_eq!(value, "hello");
        }
    }

    #[test]
    fn test_backup_before_migration_skips_nonexistent_db() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("nonexistent.db");

        // Should return Ok without creating any file
        DatabasePool::backup_before_migration(&db_path).expect("Should succeed for nonexistent DB");

        let backup_path = db_path.with_extension("db.pre-migration");
        assert!(
            !backup_path.exists(),
            "No backup should be created for nonexistent DB"
        );
    }
}
