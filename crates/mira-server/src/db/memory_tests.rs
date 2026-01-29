// crates/mira-server/src/db/memory_tests.rs
// Tests for memory storage and retrieval operations

use super::pool::DatabasePool;
use super::{
    StoreMemoryParams, clear_project_persona_sync, count_facts_without_embeddings_sync,
    delete_memory_sync, find_facts_without_embeddings_sync, get_base_persona_sync,
    get_global_memories_sync, get_health_alerts_memory_sync, get_memory_stats_sync,
    get_or_create_project_sync, get_preferences_memory_sync, get_project_persona_sync,
    mark_fact_has_embedding_sync, record_memory_access_sync, search_memories_sync,
    store_fact_embedding_sync, store_memory_sync,
};
use crate::search::embedding_to_bytes;
use std::sync::Arc;

/// Helper to create a test pool with a project
async fn setup_test_pool() -> (Arc<DatabasePool>, i64) {
    let pool = Arc::new(
        DatabasePool::open_in_memory()
            .await
            .expect("Failed to open in-memory pool"),
    );
    let project_id = pool
        .interact(|conn| {
            get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into)
        })
        .await
        .expect("Failed to create project")
        .0;
    (pool, project_id)
}

/// Helper to store a memory
fn store_memory_helper(
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

/// Helper to store a memory with session
fn store_memory_with_session_helper(
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

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // Basic CRUD Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_store_memory_basic() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("test-key"),
                    "test content",
                    "general",
                    None,
                    1.0,
                )
            })
            .await
            .unwrap();

        assert!(id > 0);

        // Verify storage
        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "test", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test content");
        assert_eq!(results[0].key, Some("test-key".to_string()));
        assert_eq!(results[0].fact_type, "general");
    }

    #[tokio::test]
    async fn test_store_memory_without_key() {
        let (pool, _project_id) = setup_test_pool().await;

        let id = pool
            .interact(|conn| {
                store_memory_helper(
                    conn,
                    None,
                    None,
                    "content without key",
                    "general",
                    None,
                    0.8,
                )
            })
            .await
            .unwrap();

        assert!(id > 0);
        assert_eq!(id, 1); // First memory
    }

    #[tokio::test]
    async fn test_store_memory_with_all_fields() {
        let (pool, project_id) = setup_test_pool().await;

        let _id = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("full-key"),
                    "full content",
                    "decision",
                    Some("architecture"),
                    0.95,
                )
            })
            .await
            .unwrap();

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "full", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_type, "decision");
        assert_eq!(results[0].category, Some("architecture".to_string()));
        // Confidence is stored as-is when < 1.0
        assert!((results[0].confidence - 0.95).abs() < 0.01);
    }

    // ═══════════════════════════════════════
    // Upsert Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_upsert_by_key_same_session() {
        let (pool, project_id) = setup_test_pool().await;

        // Store initial memory
        let id1 = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("upsert-key"),
                    "initial content",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();

        // Update with same session (no session_id provided)
        let id2 = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("upsert-key"),
                    "updated content",
                    "decision",
                    Some("architecture"),
                    0.8,
                )
            })
            .await
            .unwrap();

        // Should be same ID
        assert_eq!(id1, id2);

        // Verify update
        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "updated", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "updated content");
        assert_eq!(results[0].fact_type, "decision");
        assert_eq!(results[0].session_count, 1); // No new session
    }

    #[tokio::test]
    async fn test_upsert_by_key_different_session() {
        let (pool, project_id) = setup_test_pool().await;

        // Store initial memory with session
        let id1 = pool
            .interact(move |conn| {
                store_memory_with_session_helper(
                    conn,
                    Some(project_id),
                    Some("session-key"),
                    "initial",
                    "general",
                    None,
                    0.5,
                    Some("session-1"),
                )
            })
            .await
            .unwrap();

        // Update with different session
        let id2 = pool
            .interact(move |conn| {
                store_memory_with_session_helper(
                    conn,
                    Some(project_id),
                    Some("session-key"),
                    "updated",
                    "decision",
                    None,
                    0.7,
                    Some("session-2"),
                )
            })
            .await
            .unwrap();

        assert_eq!(id1, id2);

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "updated", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results[0].session_count, 2); // Incremented
        assert_eq!(results[0].first_session_id.as_deref(), Some("session-1"));
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-2"));
    }

    #[tokio::test]
    async fn test_upsert_promotes_after_three_sessions() {
        let (pool, project_id) = setup_test_pool().await;

        let key = "promotion-key";

        // Session 1
        pool.interact(move |conn| {
            store_memory_with_session_helper(
                conn,
                Some(project_id),
                Some(key),
                "content",
                "general",
                None,
                0.5,
                Some("s1"),
            )
        })
        .await
        .unwrap();

        // Session 2
        pool.interact(move |conn| {
            store_memory_with_session_helper(
                conn,
                Some(project_id),
                Some(key),
                "content v2",
                "general",
                None,
                0.5,
                Some("s2"),
            )
        })
        .await
        .unwrap();

        // Session 3 - should promote
        pool.interact(move |conn| {
            store_memory_with_session_helper(
                conn,
                Some(project_id),
                Some(key),
                "content v3",
                "general",
                None,
                0.5,
                Some("s3"),
            )
        })
        .await
        .unwrap();

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "content", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results[0].status, "confirmed");
        assert!(results[0].confidence > 0.5); // Should have increased
    }

    // ═══════════════════════════════════════
    // Session Tracking Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_record_memory_access_new_session() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("access-key"),
                    "content",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();

        // Record access from new session
        pool.interact(move |conn| {
            record_memory_access_sync(conn, id, "session-new").map_err(Into::into)
        })
        .await
        .unwrap();

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "content", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results[0].session_count, 2);
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-new"));
    }

    #[tokio::test]
    async fn test_record_memory_access_same_session() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                store_memory_with_session_helper(
                    conn,
                    Some(project_id),
                    Some("access-key2"),
                    "content",
                    "general",
                    None,
                    0.5,
                    Some("session-1"),
                )
            })
            .await
            .unwrap();

        // Same session - should not increment
        pool.interact(move |conn| {
            record_memory_access_sync(conn, id, "session-1").map_err(Into::into)
        })
        .await
        .unwrap();

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "content", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results[0].session_count, 1); // Unchanged
    }

    // ═══════════════════════════════════════
    // Statistics Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_memory_stats_empty() {
        let (pool, project_id) = setup_test_pool().await;

        let (candidates, confirmed) = pool
            .interact(move |conn| get_memory_stats_sync(conn, Some(project_id)).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(candidates, 0);
        assert_eq!(confirmed, 0);
    }

    #[tokio::test]
    async fn test_get_memory_stats_with_data() {
        let (pool, project_id) = setup_test_pool().await;

        // Add some candidates
        for i in 0..3 {
            let key = format!("key-{}", i);
            let content = format!("content {}", i);
            pool.interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some(&key),
                    &content,
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();
        }

        let (candidates, confirmed) = pool
            .interact(move |conn| get_memory_stats_sync(conn, Some(project_id)).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(candidates, 3);
        assert_eq!(confirmed, 0);
    }

    #[tokio::test]
    async fn test_get_memory_stats_global() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "global fact",
                "personal",
                Some("test"),
                1.0,
            )
        })
        .await
        .unwrap();

        let (candidates, _confirmed) = pool
            .interact(|conn| get_memory_stats_sync(conn, None).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(candidates, 1);
    }

    // ═══════════════════════════════════════
    // Search Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_search_memories_basic() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("key1"),
                "the quick brown fox",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("key2"),
                "lazy dog jumps",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "fox", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("fox"));
    }

    #[tokio::test]
    async fn test_search_memories_limit() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..10 {
            let key = format!("key-{}", i);
            pool.interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some(&key),
                    "test content",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();
        }

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "test", None, 5).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_search_memories_sql_injection_escape() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("key"),
                "100% complete",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        // % is a SQL wildcard - should be escaped
        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "100%", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Preferences Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_preferences() {
        let (pool, project_id) = setup_test_pool().await;

        // Add some preferences
        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("pref-1"),
                "pref content 1",
                "preference",
                Some("ui"),
                0.8,
            )
        })
        .await
        .unwrap();

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("pref-2"),
                "pref content 2",
                "preference",
                Some("editor"),
                0.9,
            )
        })
        .await
        .unwrap();

        // Add non-preference
        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project_id),
                Some("other"),
                "other content",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        let prefs = pool
            .interact(move |conn| {
                get_preferences_memory_sync(conn, Some(project_id)).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(prefs.len(), 2);
        assert!(prefs.iter().all(|p| p.fact_type == "preference"));
    }

    // ═══════════════════════════════════════
    // Delete Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_memory() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("delete-key"),
                    "to be deleted",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();

        let deleted = pool
            .interact(move |conn| delete_memory_sync(conn, id).map_err(Into::into))
            .await
            .unwrap();
        assert!(deleted);

        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "deleted", None, 10)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_memory_nonexistent() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let deleted = pool
            .interact(|conn| delete_memory_sync(conn, 99999).map_err(Into::into))
            .await
            .unwrap();
        assert!(!deleted);
    }

    // ═══════════════════════════════════════
    // Health Alerts Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_health_alerts() {
        let (pool, project_id) = setup_test_pool().await;

        // Add high-confidence health alert
        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(project_id),
                    key: Some("health-1"),
                    content: "critical issue found",
                    fact_type: "health",
                    category: Some("security"),
                    confidence: 0.9,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        // Add low-confidence (should not appear)
        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(project_id),
                    key: Some("health-2"),
                    content: "minor issue",
                    fact_type: "health",
                    category: None,
                    confidence: 0.5,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let alerts = pool
            .interact(move |conn| {
                get_health_alerts_memory_sync(conn, Some(project_id), 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(alerts.len(), 1);
    }

    // ═══════════════════════════════════════
    // Global Memory Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_store_global_memory() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let id = pool
            .interact(|conn| {
                store_memory_helper(
                    conn,
                    None,
                    None,
                    "user prefers dark mode",
                    "personal",
                    Some("ui"),
                    1.0,
                )
            })
            .await
            .unwrap();

        assert!(id > 0);

        let results = pool
            .interact(|conn| get_global_memories_sync(conn, None, 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_type, "personal");
    }

    #[tokio::test]
    async fn test_get_global_memories_with_category() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "fact 1",
                "personal",
                Some("category-a"),
                1.0,
            )
        })
        .await
        .unwrap();
        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "fact 2",
                "personal",
                Some("category-a"),
                1.0,
            )
        })
        .await
        .unwrap();
        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "fact 3",
                "personal",
                Some("category-b"),
                1.0,
            )
        })
        .await
        .unwrap();

        let cat_a = pool
            .interact(|conn| {
                get_global_memories_sync(conn, Some("category-a"), 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(cat_a.len(), 2);

        let cat_b = pool
            .interact(|conn| {
                get_global_memories_sync(conn, Some("category-b"), 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(cat_b.len(), 1);
    }

    #[tokio::test]
    async fn test_get_user_profile() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "name: Alice",
                "personal",
                Some("profile"),
                1.0,
            )
        })
        .await
        .unwrap();
        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "role: Developer",
                "personal",
                Some("profile"),
                1.0,
            )
        })
        .await
        .unwrap();
        pool.interact(|conn| {
            store_memory_helper(
                conn,
                None,
                None,
                "likes coffee",
                "personal",
                Some("preference"),
                1.0,
            )
        })
        .await
        .unwrap();

        let profile = pool
            .interact(|conn| {
                get_global_memories_sync(conn, Some("profile"), 20).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(profile.len(), 2);
        assert!(
            profile
                .iter()
                .all(|p| p.category.as_deref() == Some("profile"))
        );
    }

    // ═══════════════════════════════════════
    // Persona Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_base_persona() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let persona = pool
            .interact(|conn| get_base_persona_sync(conn).map_err(Into::into))
            .await
            .unwrap();
        assert!(persona.is_none());

        pool.interact(|conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: None,
                    key: Some("base_persona"),
                    content: "You are a helpful assistant",
                    fact_type: "persona",
                    category: None,
                    confidence: 1.0,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let persona = pool
            .interact(|conn| get_base_persona_sync(conn).map_err(Into::into))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persona, "You are a helpful assistant");
    }

    #[tokio::test]
    async fn test_project_persona() {
        let (pool, project_id) = setup_test_pool().await;

        let persona = pool
            .interact(move |conn| get_project_persona_sync(conn, project_id).map_err(Into::into))
            .await
            .unwrap();
        assert!(persona.is_none());

        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(project_id),
                    key: Some("project_persona"),
                    content: "Project-specific persona",
                    fact_type: "persona",
                    category: None,
                    confidence: 1.0,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let persona = pool
            .interact(move |conn| get_project_persona_sync(conn, project_id).map_err(Into::into))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persona, "Project-specific persona");
    }

    #[tokio::test]
    async fn test_clear_project_persona() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(project_id),
                    key: Some("project_persona"),
                    content: "Persona",
                    fact_type: "persona",
                    category: None,
                    confidence: 1.0,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let persona = pool
            .interact(move |conn| get_project_persona_sync(conn, project_id).map_err(Into::into))
            .await
            .unwrap();
        assert!(persona.is_some());

        let cleared = pool
            .interact(move |conn| clear_project_persona_sync(conn, project_id).map_err(Into::into))
            .await
            .unwrap();
        assert!(cleared);

        let persona = pool
            .interact(move |conn| get_project_persona_sync(conn, project_id).map_err(Into::into))
            .await
            .unwrap();
        assert!(persona.is_none());
    }

    // ═══════════════════════════════════════
    // Embedding Status Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_mark_and_find_facts_without_embeddings() {
        let (pool, project_id) = setup_test_pool().await;

        // Add some memories
        for i in 0..3 {
            let key = format!("key-{}", i);
            let content = format!("content {}", i);
            pool.interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some(&key),
                    &content,
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();
        }

        // All should be without embeddings
        let facts = pool
            .interact(|conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(facts.len(), 3);

        // Mark one as having embedding
        let fact_id = facts[0].id;
        pool.interact(move |conn| mark_fact_has_embedding_sync(conn, fact_id).map_err(Into::into))
            .await
            .unwrap();

        let remaining = pool
            .interact(|conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[tokio::test]
    async fn test_count_facts_without_embeddings() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..5 {
            let key = format!("key-{}", i);
            pool.interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some(&key),
                    "content",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();
        }

        let count = pool
            .interact(|conn| count_facts_without_embeddings_sync(conn).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(count, 5);

        // Mark some
        let facts = pool
            .interact(|conn| find_facts_without_embeddings_sync(conn, 3).map_err(Into::into))
            .await
            .unwrap();
        for fact in &facts {
            let fact_id = fact.id;
            pool.interact(move |conn| {
                mark_fact_has_embedding_sync(conn, fact_id).map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let count = pool
            .interact(|conn| count_facts_without_embeddings_sync(conn).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_store_fact_embedding() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                store_memory_helper(
                    conn,
                    Some(project_id),
                    Some("embed-key"),
                    "content to embed",
                    "general",
                    None,
                    0.5,
                )
            })
            .await
            .unwrap();

        // Create a 1536-dimensional embedding (matches schema)
        let embedding: Vec<f32> = (0..1536).map(|i| (i as f32) * 0.001).collect();
        let embedding_bytes = embedding_to_bytes(&embedding);
        pool.interact(move |conn| {
            store_fact_embedding_sync(conn, id, "content to embed", &embedding_bytes)
                .map_err(Into::into)
        })
        .await
        .unwrap();

        // Should no longer be in facts without embeddings
        let facts = pool
            .interact(|conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into))
            .await
            .unwrap();
        assert!(!facts.iter().any(|f| f.id == id));

        // Memory should be marked
        let results = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project_id), "embed", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Scope Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_project_isolation() {
        let (pool, project1) = setup_test_pool().await;
        let project2 = pool
            .interact(|conn| {
                get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
            })
            .await
            .unwrap()
            .0;

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project1),
                Some("key"),
                "project 1 content",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        pool.interact(move |conn| {
            store_memory_helper(
                conn,
                Some(project2),
                Some("key"),
                "project 2 content",
                "general",
                None,
                0.5,
            )
        })
        .await
        .unwrap();

        let results1 = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project1), "content", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].content, "project 1 content");

        let results2 = pool
            .interact(move |conn| {
                search_memories_sync(conn, Some(project2), "content", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].content, "project 2 content");
    }

    // ═══════════════════════════════════════
    // Empty/Edge Cases
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_empty_search() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let results = pool
            .interact(|conn| {
                search_memories_sync(conn, None, "nonexistent", None, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_preferences() {
        let (pool, project_id) = setup_test_pool().await;

        let prefs = pool
            .interact(move |conn| {
                get_preferences_memory_sync(conn, Some(project_id)).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(prefs.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_health_alerts() {
        let (pool, project_id) = setup_test_pool().await;

        let alerts = pool
            .interact(move |conn| {
                get_health_alerts_memory_sync(conn, Some(project_id), 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(alerts.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_global_memories() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let memories = pool
            .interact(|conn| get_global_memories_sync(conn, None, 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(memories.len(), 0);
    }
}
