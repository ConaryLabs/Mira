// db/test_support.rs
// Shared test helpers and macros for database tests

use super::pool::DatabasePool;
use super::{StoreMemoryParams, get_or_create_project_sync, store_memory_sync};
use std::sync::Arc;

/// Run a sync database operation in the test pool, unwrapping the result.
///
/// Wraps `pool.interact(move |conn| body).await.unwrap()` into a single expression.
/// The body must return `anyhow::Result<T>` (use `.map_err(Into::into)` for rusqlite errors).
///
/// ```ignore
/// let id = db!(pool, |conn| create_task_sync(conn, args).map_err(Into::into));
/// let recap = db!(pool, |conn| Ok::<_, anyhow::Error>(build_recap(conn)));
/// ```
macro_rules! db {
    ($pool:expr, |$conn:ident| $body:expr) => {
        $pool.interact(move |$conn| $body).await.unwrap()
    };
}

/// Create a test pool (in-memory DB, no project)
pub async fn setup_test_pool() -> Arc<DatabasePool> {
    Arc::new(
        DatabasePool::open_in_memory()
            .await
            .expect("Failed to open in-memory pool"),
    )
}

/// Create a test pool with a default project
pub async fn setup_test_pool_with_project() -> (Arc<DatabasePool>, i64) {
    let pool = setup_test_pool().await;
    let project_id = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into)
    })
    .0;
    (pool, project_id)
}

/// Create a second project in the test pool (for isolation tests)
pub async fn setup_second_project(pool: &Arc<DatabasePool>) -> i64 {
    pool.interact(|conn| {
        get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
    })
    .await
    .unwrap()
    .0
}

/// Create a sync in-memory connection with all migrations applied.
/// Use this for sync tests that don't need async pool semantics.
/// Loads sqlite-vec and runs all migrations.
pub fn setup_test_connection() -> rusqlite::Connection {
    use super::pool::ensure_sqlite_vec_registered;
    use super::schema::run_all_migrations;
    ensure_sqlite_vec_registered();
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    run_all_migrations(&conn).unwrap();
    conn
}

/// Store a test memory with common defaults
pub fn store_memory_helper(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    key: Option<&str>,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    confidence: f64,
) -> anyhow::Result<i64> {
    store_memory_sync(
        conn,
        StoreMemoryParams {
            project_id,
            key,
            content,
            fact_type,
            category,
            confidence,
            session_id: None,
            user_id: None,
            scope: "project",
            branch: None,
        },
    )
    .map_err(Into::into)
}

/// Store a test memory with session tracking
pub fn store_memory_with_session_helper(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    key: Option<&str>,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    confidence: f64,
    session_id: Option<&str>,
) -> anyhow::Result<i64> {
    store_memory_sync(
        conn,
        StoreMemoryParams {
            project_id,
            key,
            content,
            fact_type,
            category,
            confidence,
            session_id,
            user_id: None,
            scope: "project",
            branch: None,
        },
    )
    .map_err(Into::into)
}
