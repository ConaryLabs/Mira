// db/test_support.rs
// Shared test helpers for database tests

use super::pool::DatabasePool;
use super::get_or_create_project_sync;
use std::sync::Arc;

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
    let project_id = pool
        .interact(|conn| {
            get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into)
        })
        .await
        .expect("Failed to create project")
        .0;
    (pool, project_id)
}
