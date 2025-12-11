// src/state_stubs.rs
// Minimal AppState stub for compatibility
// Full state management is not needed in power suit mode

use sqlx::SqlitePool;
use std::sync::Arc;

/// SQLite store wrapper for compatibility
pub struct SqliteStore {
    pub pool: SqlitePool,
}

impl SqliteStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Minimal app state for MCP server
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<SqlitePool>,
    pub sqlite_store: Arc<SqliteStore>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        let pool_arc = Arc::new(pool);
        let sqlite_store = Arc::new(SqliteStore {
            pool: SqlitePool::connect_lazy("sqlite::memory:").unwrap(), // placeholder
        });
        Self {
            db: pool_arc,
            sqlite_store,
        }
    }

    pub fn from_pool(pool: SqlitePool) -> Self {
        let pool_clone = pool.clone();
        Self {
            db: Arc::new(pool),
            sqlite_store: Arc::new(SqliteStore::new(pool_clone)),
        }
    }

    pub fn db(&self) -> &SqlitePool {
        &self.db
    }
}
