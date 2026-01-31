// db/pool.rs
// Async connection pool using deadpool-sqlite
//
// # Async Database Access Patterns
//
// ## Primary Pattern: pool.interact()
// Use `pool.interact()` for MCP tool handlers and other code that has access to a `DatabasePool`:
// ```ignore
// let result = ctx.pool().interact(move |conn| {
//     some_sync_function(conn, arg1, arg2)
// }).await?;
// ```
//
// ## Common Pitfalls
//
// 1. **Don't block the async runtime**: Always use `pool.interact()` for database access.
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

use crate::utils::path_to_string;
use crate::utils::ResultExt;
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
fn ensure_sqlite_vec_registered() {
    SQLITE_VEC_INIT.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }
        tracing::debug!("sqlite-vec extension registered globally");
    });
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

impl DatabasePool {
    /// Open a pooled database at the given path.
    ///
    /// This will:
    /// 1. Register sqlite-vec extension globally (if not already done)
    /// 2. Ensure parent directory exists with secure permissions
    /// 3. Create the pool with post_create hooks for per-connection setup
    /// 4. Run schema migrations on a dedicated connection before returning
    pub async fn open(path: &Path) -> Result<Self> {
        // Step 1: Register sqlite-vec globally
        ensure_sqlite_vec_registered();

        // Step 2: Ensure parent directory exists with secure permissions
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(parent)?.permissions();
                perms.set_mode(0o700); // rwx------
                std::fs::set_permissions(parent, perms)?;
            }
        }

        let path_buf = path.to_path_buf();
        let path_str = path_to_string(path);

        // Step 3: Create pool with post_create hook
        let cfg = Config::new(&path_str);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder")?
            .post_create(Hook::async_fn(move |conn, _metrics| {
                let path_for_perms = path_buf.clone();
                Box::pin(async move {
                    conn.interact(move |conn| {
                        setup_connection(conn)?;

                        // Set file permissions on first access
                        #[cfg(unix)]
                        if let Ok(metadata) = std::fs::metadata(&path_for_perms) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o600); // rw-------
                            if let Err(e) = std::fs::set_permissions(&path_for_perms, perms) {
                                tracing::warn!(
                                    "Failed to set database file permissions to 0600: {}",
                                    e
                                );
                            }
                        }

                        Ok::<_, rusqlite::Error>(())
                    })
                    .await
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
                    })?
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(
                            format!("connection setup failed: {e}").into(),
                        )
                    })
                })
            }))
            .build()
            .context("Failed to build connection pool")?;

        let db_pool = Self {
            pool,
            path: Some(path.to_path_buf()),
            memory_uri: None,
        };

        // Step 4: Run migrations on a dedicated connection
        db_pool.run_migrations().await?;

        Ok(db_pool)
    }

    /// Open a pooled database for the code index at the given path.
    ///
    /// Same setup as `open()` but runs code-specific migrations instead
    /// of the main schema migrations. The code database holds:
    /// code_symbols, call_graph, imports, codebase_modules, vec_code,
    /// code_fts, and pending_embeddings.
    pub async fn open_code_db(path: &Path) -> Result<Self> {
        ensure_sqlite_vec_registered();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(parent)?.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(parent, perms)?;
            }
        }

        let path_buf = path.to_path_buf();
        let path_str = path_to_string(path);

        let cfg = Config::new(&path_str);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder for code DB")?
            .post_create(Hook::async_fn(move |conn, _metrics| {
                let path_for_perms = path_buf.clone();
                Box::pin(async move {
                    conn.interact(move |conn| {
                        setup_connection(conn)?;

                        #[cfg(unix)]
                        if let Ok(metadata) = std::fs::metadata(&path_for_perms) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o600);
                            if let Err(e) = std::fs::set_permissions(&path_for_perms, perms) {
                                tracing::warn!(
                                    "Failed to set code DB file permissions to 0600: {}",
                                    e
                                );
                            }
                        }

                        Ok::<_, rusqlite::Error>(())
                    })
                    .await
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
                    })?
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(
                            format!("connection setup failed: {e}").into(),
                        )
                    })
                })
            }))
            .build()
            .context("Failed to build code DB connection pool")?;

        let db_pool = Self {
            pool,
            path: Some(path.to_path_buf()),
            memory_uri: None,
        };

        db_pool.run_code_migrations().await?;

        Ok(db_pool)
    }

    /// Open a pooled in-memory database for the code index (for tests).
    pub async fn open_code_db_in_memory() -> Result<Self> {
        ensure_sqlite_vec_registered();

        let unique_id = uuid::Uuid::new_v4();
        let uri = format!("file:memdb_code_{unique_id}?mode=memory&cache=shared");

        let cfg = Config::new(&uri);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder for in-memory code DB")?
            .post_create(Hook::async_fn(|conn, _metrics| {
                Box::pin(async move {
                    conn.interact(|conn| {
                        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
                        Ok::<_, rusqlite::Error>(())
                    })
                    .await
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
                    })?
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(
                            format!("connection setup failed: {e}").into(),
                        )
                    })
                })
            }))
            .build()
            .context("Failed to build in-memory code DB pool")?;

        let db_pool = Self {
            pool,
            path: None,
            memory_uri: Some(uri),
        };
        db_pool.run_code_migrations().await?;
        Ok(db_pool)
    }

    /// Open a pooled in-memory database.
    ///
    /// Uses a shared cache URI so all connections access the same in-memory database.
    /// This is critical for tests - without shared cache, each connection would get
    /// its own separate in-memory database.
    pub async fn open_in_memory() -> Result<Self> {
        ensure_sqlite_vec_registered();

        // Use shared cache mode for in-memory DB so all pool connections share state.
        // The unique ID ensures different test instances don't collide.
        let unique_id = uuid::Uuid::new_v4();
        let uri = format!("file:memdb_{unique_id}?mode=memory&cache=shared");

        let cfg = Config::new(&uri);
        let pool = cfg
            .builder(Runtime::Tokio1)
            .context("Failed to create pool builder for in-memory DB")?
            .post_create(Hook::async_fn(|conn, _metrics| {
                Box::pin(async move {
                    conn.interact(|conn| {
                        // WAL mode doesn't work for in-memory, just set foreign keys
                        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
                        Ok::<_, rusqlite::Error>(())
                    })
                    .await
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(format!("interact failed: {e}").into())
                    })?
                    .map_err(|e| {
                        deadpool_sqlite::HookError::Message(
                            format!("connection setup failed: {e}").into(),
                        )
                    })
                })
            }))
            .build()
            .context("Failed to build in-memory connection pool")?;

        let db_pool = Self {
            pool,
            path: None,
            memory_uri: Some(uri),
        };
        db_pool.run_migrations().await?;
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
    pub async fn run<F, R, E>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: std::fmt::Display + Send + 'static,
    {
        self.pool
            .get()
            .await
            .map_err(|e| format!("Failed to get connection: {}", e))?
            .interact(move |conn| f(conn).map_err(|e| anyhow::anyhow!("{}", e)))
            .await
            .map_err(|e| format!("Database error: {}", e))?
            .str_err()
    }

    /// Run a closure with retry on SQLITE_BUSY errors.
    ///
    /// Uses exponential backoff (100ms, 500ms, 2000ms) for up to 3 attempts.
    /// Use this for critical writes that must not be lost (session creation,
    /// memory storage, goal updates). For non-critical writes (tool history,
    /// analytics), prefer fire-and-forget with `tokio::spawn`.
    pub async fn interact_with_retry<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + Clone + 'static,
        R: Send + 'static,
    {
        let delays = [
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(2000),
        ];

        for (attempt, delay) in delays.iter().enumerate() {
            let f_clone = f.clone();
            match self.interact(f_clone).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("database is locked") || err_str.contains("SQLITE_BUSY") {
                        tracing::warn!(
                            "SQLITE_BUSY on attempt {}/{}, retrying in {:?}",
                            attempt + 1,
                            delays.len(),
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
        self.interact(f).await
    }

    /// Get the database file path (None for in-memory).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Run main schema migrations.
    ///
    /// Called during pool creation, but can also be called explicitly if needed.
    async fn run_migrations(&self) -> Result<()> {
        self.interact(|conn| {
            super::schema::run_all_migrations(conn)?;
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

    /// Get pool status for monitoring.
    pub fn status(&self) -> PoolStatus {
        let status = self.pool.status();
        PoolStatus {
            size: status.size,
            available: status.available,
            waiting: status.waiting,
        }
    }

    // =========================================================================
    // Expert Configuration (async versions to avoid blocking)
    // =========================================================================

    /// Get custom system prompt for an expert role (async).
    /// Returns None if no custom prompt is set.
    pub async fn get_custom_prompt(&self, role: &str) -> Result<Option<String>> {
        let role = role.to_string();
        self.interact(move |conn| {
            let result = conn.query_row(
                "SELECT prompt FROM system_prompts WHERE role = ?",
                rusqlite::params![role],
                |row| row.get(0),
            );

            match result {
                Ok(prompt) => Ok(Some(prompt)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Get full expert configuration for a role (async).
    /// Returns default config if no custom config is set.
    pub async fn get_expert_config(&self, role: &str) -> Result<super::config::ExpertConfig> {
        use crate::llm::Provider;

        let role = role.to_string();
        self.interact(move |conn| {
            let result = conn.query_row(
                "SELECT prompt, provider, model FROM system_prompts WHERE role = ?",
                rusqlite::params![role],
                |row| {
                    let prompt: Option<String> = row.get(0)?;
                    let provider_str: Option<String> = row.get(1)?;
                    let model: Option<String> = row.get(2)?;
                    Ok((prompt, provider_str, model))
                },
            );

            match result {
                Ok((prompt, provider_str, model)) => {
                    let provider = provider_str
                        .as_deref()
                        .and_then(Provider::from_str)
                        .unwrap_or(Provider::DeepSeek);
                    Ok(super::config::ExpertConfig {
                        prompt,
                        provider,
                        model,
                    })
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Ok(super::config::ExpertConfig::default())
                }
                Err(e) => Err(e.into()),
            }
        })
        .await
    }
}

/// Pool status for monitoring.
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub size: usize,
    pub available: usize,
    pub waiting: usize,
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
}
