// crates/mira-server/src/db/memory_tests.rs
// Tests for memory storage and retrieval operations

use super::test_support::{
    setup_second_project, setup_test_pool, setup_test_pool_with_project, store_memory_helper,
    store_memory_with_session_helper,
};
use super::{
    StoreMemoryParams, clear_project_persona_sync, count_facts_without_embeddings_sync,
    delete_memory_sync, find_facts_without_embeddings_sync, get_base_persona_sync,
    get_global_memories_sync, get_health_alerts_memory_sync, get_memory_stats_sync,
    get_preferences_memory_sync, get_project_persona_sync,
    mark_fact_has_embedding_sync, record_memory_access_sync, search_memories_sync,
    store_fact_embedding_sync, store_memory_sync,
};
use crate::search::embedding_to_bytes;

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // Basic CRUD Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_store_memory_basic() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("test-key"),
            "test content",
            "general",
            None,
            1.0,
        ));

        assert!(id > 0);

        // Verify storage
        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "test",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test content");
        assert_eq!(results[0].key, Some("test-key".to_string()));
        assert_eq!(results[0].fact_type, "general");
    }

    #[tokio::test]
    async fn test_store_memory_without_key() {
        let (pool, _project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "content without key",
            "general",
            None,
            0.8,
        ));

        assert!(id > 0);
        assert_eq!(id, 1); // First memory
    }

    #[tokio::test]
    async fn test_store_memory_with_all_fields() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let _id = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("full-key"),
            "full content",
            "decision",
            Some("architecture"),
            0.95,
        ));

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "full",
            None,
            10
        )
        .map_err(Into::into));
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
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Store initial memory
        let id1 = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("upsert-key"),
            "initial content",
            "general",
            None,
            0.5,
        ));

        // Update with same session (no session_id provided)
        let id2 = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("upsert-key"),
            "updated content",
            "decision",
            Some("architecture"),
            0.8,
        ));

        // Should be same ID
        assert_eq!(id1, id2);

        // Verify update
        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "updated",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "updated content");
        assert_eq!(results[0].fact_type, "decision");
        assert_eq!(results[0].session_count, 1); // No new session
    }

    #[tokio::test]
    async fn test_upsert_by_key_different_session() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Store initial memory with session
        let id1 = db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some("session-key"),
            "initial",
            "general",
            None,
            0.5,
            Some("session-1"),
        ));

        // Update with different session
        let id2 = db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some("session-key"),
            "updated",
            "decision",
            None,
            0.7,
            Some("session-2"),
        ));

        assert_eq!(id1, id2);

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "updated",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results[0].session_count, 2); // Incremented
        assert_eq!(results[0].first_session_id.as_deref(), Some("session-1"));
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-2"));
    }

    #[tokio::test]
    async fn test_upsert_promotes_after_three_sessions() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let key = "promotion-key";

        // Session 1
        db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some(key),
            "content",
            "general",
            None,
            0.5,
            Some("s1"),
        ));

        // Session 2
        db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some(key),
            "content v2",
            "general",
            None,
            0.5,
            Some("s2"),
        ));

        // Session 3 - should promote
        db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some(key),
            "content v3",
            "general",
            None,
            0.5,
            Some("s3"),
        ));

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "content",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results[0].status, "confirmed");
        assert!(results[0].confidence > 0.5); // Should have increased
    }

    // ═══════════════════════════════════════
    // Session Tracking Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_record_memory_access_new_session() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("access-key"),
            "content",
            "general",
            None,
            0.5,
        ));

        // Record access from new session
        db!(pool, |conn| record_memory_access_sync(conn, id, "session-new")
            .map_err(Into::into));

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "content",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results[0].session_count, 2);
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-new"));
    }

    #[tokio::test]
    async fn test_record_memory_access_same_session() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_with_session_helper(
            conn,
            Some(project_id),
            Some("access-key2"),
            "content",
            "general",
            None,
            0.5,
            Some("session-1"),
        ));

        // Same session - should not increment
        db!(pool, |conn| record_memory_access_sync(conn, id, "session-1")
            .map_err(Into::into));

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "content",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results[0].session_count, 1); // Unchanged
    }

    // ═══════════════════════════════════════
    // Statistics Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_memory_stats_empty() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let (candidates, confirmed) =
            db!(pool, |conn| get_memory_stats_sync(conn, Some(project_id)).map_err(Into::into));
        assert_eq!(candidates, 0);
        assert_eq!(confirmed, 0);
    }

    #[tokio::test]
    async fn test_get_memory_stats_with_data() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Add some candidates
        for i in 0..3 {
            let key = format!("key-{}", i);
            let content = format!("content {}", i);
            db!(pool, |conn| store_memory_helper(
                conn,
                Some(project_id),
                Some(&key),
                &content,
                "general",
                None,
                0.5,
            ));
        }

        let (candidates, confirmed) =
            db!(pool, |conn| get_memory_stats_sync(conn, Some(project_id)).map_err(Into::into));
        assert_eq!(candidates, 3);
        assert_eq!(confirmed, 0);
    }

    #[tokio::test]
    async fn test_get_memory_stats_global() {
        let pool = setup_test_pool().await;

        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "global fact",
            "personal",
            Some("test"),
            1.0,
        ));

        let (candidates, _confirmed) =
            db!(pool, |conn| get_memory_stats_sync(conn, None).map_err(Into::into));
        assert_eq!(candidates, 1);
    }

    // ═══════════════════════════════════════
    // Search Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_search_memories_basic() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("key1"),
            "the quick brown fox",
            "general",
            None,
            0.5,
        ));

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("key2"),
            "lazy dog jumps",
            "general",
            None,
            0.5,
        ));

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "fox",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("fox"));
    }

    #[tokio::test]
    async fn test_search_memories_limit() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        for i in 0..10 {
            let key = format!("key-{}", i);
            db!(pool, |conn| store_memory_helper(
                conn,
                Some(project_id),
                Some(&key),
                "test content",
                "general",
                None,
                0.5,
            ));
        }

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "test",
            None,
            5
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_search_memories_sql_injection_escape() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("key"),
            "100% complete",
            "general",
            None,
            0.5,
        ));

        // % is a SQL wildcard - should be escaped
        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "100%",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Preferences Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_preferences() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Add some preferences
        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("pref-1"),
            "pref content 1",
            "preference",
            Some("ui"),
            0.8,
        ));

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("pref-2"),
            "pref content 2",
            "preference",
            Some("editor"),
            0.9,
        ));

        // Add non-preference
        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("other"),
            "other content",
            "general",
            None,
            0.5,
        ));

        let prefs = db!(pool, |conn| get_preferences_memory_sync(conn, Some(project_id))
            .map_err(Into::into));
        assert_eq!(prefs.len(), 2);
        assert!(prefs.iter().all(|p| p.fact_type == "preference"));
    }

    // ═══════════════════════════════════════
    // Delete Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_memory() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("delete-key"),
            "to be deleted",
            "general",
            None,
            0.5,
        ));

        let deleted = db!(pool, |conn| delete_memory_sync(conn, id).map_err(Into::into));
        assert!(deleted);

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "deleted",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_memory_nonexistent() {
        let pool = setup_test_pool().await;

        let deleted = db!(pool, |conn| delete_memory_sync(conn, 99999).map_err(Into::into));
        assert!(!deleted);
    }

    // ═══════════════════════════════════════
    // Health Alerts Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_health_alerts() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Add high-confidence health alert
        db!(pool, |conn| store_memory_sync(
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
        .map_err(Into::into));

        // Add low-confidence (should not appear)
        db!(pool, |conn| store_memory_sync(
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
        .map_err(Into::into));

        let alerts = db!(pool, |conn| get_health_alerts_memory_sync(
            conn,
            Some(project_id),
            10
        )
        .map_err(Into::into));
        assert_eq!(alerts.len(), 1);
    }

    // ═══════════════════════════════════════
    // Global Memory Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_store_global_memory() {
        let pool = setup_test_pool().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "user prefers dark mode",
            "personal",
            Some("ui"),
            1.0,
        ));

        assert!(id > 0);

        let results =
            db!(pool, |conn| get_global_memories_sync(conn, None, 10).map_err(Into::into));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_type, "personal");
    }

    #[tokio::test]
    async fn test_get_global_memories_with_category() {
        let pool = setup_test_pool().await;

        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "fact 1",
            "personal",
            Some("category-a"),
            1.0,
        ));
        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "fact 2",
            "personal",
            Some("category-a"),
            1.0,
        ));
        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "fact 3",
            "personal",
            Some("category-b"),
            1.0,
        ));

        let cat_a = db!(pool, |conn| get_global_memories_sync(
            conn,
            Some("category-a"),
            10
        )
        .map_err(Into::into));
        assert_eq!(cat_a.len(), 2);

        let cat_b = db!(pool, |conn| get_global_memories_sync(
            conn,
            Some("category-b"),
            10
        )
        .map_err(Into::into));
        assert_eq!(cat_b.len(), 1);
    }

    #[tokio::test]
    async fn test_get_user_profile() {
        let pool = setup_test_pool().await;

        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "name: Alice",
            "personal",
            Some("profile"),
            1.0,
        ));
        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "role: Developer",
            "personal",
            Some("profile"),
            1.0,
        ));
        db!(pool, |conn| store_memory_helper(
            conn,
            None,
            None,
            "likes coffee",
            "personal",
            Some("preference"),
            1.0,
        ));

        let profile = db!(pool, |conn| get_global_memories_sync(
            conn,
            Some("profile"),
            20
        )
        .map_err(Into::into));
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
        let pool = setup_test_pool().await;

        let persona = db!(pool, |conn| get_base_persona_sync(conn).map_err(Into::into));
        assert!(persona.is_none());

        db!(pool, |conn| store_memory_sync(
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
        .map_err(Into::into));

        let persona = db!(pool, |conn| get_base_persona_sync(conn).map_err(Into::into))
            .unwrap();
        assert_eq!(persona, "You are a helpful assistant");
    }

    #[tokio::test]
    async fn test_project_persona() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let persona =
            db!(pool, |conn| get_project_persona_sync(conn, project_id).map_err(Into::into));
        assert!(persona.is_none());

        db!(pool, |conn| store_memory_sync(
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
        .map_err(Into::into));

        let persona =
            db!(pool, |conn| get_project_persona_sync(conn, project_id).map_err(Into::into))
                .unwrap();
        assert_eq!(persona, "Project-specific persona");
    }

    #[tokio::test]
    async fn test_clear_project_persona() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        db!(pool, |conn| store_memory_sync(
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
        .map_err(Into::into));

        let persona =
            db!(pool, |conn| get_project_persona_sync(conn, project_id).map_err(Into::into));
        assert!(persona.is_some());

        let cleared =
            db!(pool, |conn| clear_project_persona_sync(conn, project_id).map_err(Into::into));
        assert!(cleared);

        let persona =
            db!(pool, |conn| get_project_persona_sync(conn, project_id).map_err(Into::into));
        assert!(persona.is_none());
    }

    // ═══════════════════════════════════════
    // Embedding Status Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_mark_and_find_facts_without_embeddings() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Add some memories
        for i in 0..3 {
            let key = format!("key-{}", i);
            let content = format!("content {}", i);
            db!(pool, |conn| store_memory_helper(
                conn,
                Some(project_id),
                Some(&key),
                &content,
                "general",
                None,
                0.5,
            ));
        }

        // All should be without embeddings
        let facts =
            db!(pool, |conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into));
        assert_eq!(facts.len(), 3);

        // Mark one as having embedding
        let fact_id = facts[0].id;
        db!(pool, |conn| mark_fact_has_embedding_sync(conn, fact_id).map_err(Into::into));

        let remaining =
            db!(pool, |conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into));
        assert_eq!(remaining.len(), 2);
    }

    #[tokio::test]
    async fn test_count_facts_without_embeddings() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        for i in 0..5 {
            let key = format!("key-{}", i);
            db!(pool, |conn| store_memory_helper(
                conn,
                Some(project_id),
                Some(&key),
                "content",
                "general",
                None,
                0.5,
            ));
        }

        let count =
            db!(pool, |conn| count_facts_without_embeddings_sync(conn).map_err(Into::into));
        assert_eq!(count, 5);

        // Mark some
        let facts =
            db!(pool, |conn| find_facts_without_embeddings_sync(conn, 3).map_err(Into::into));
        for fact in &facts {
            let fact_id = fact.id;
            db!(pool, |conn| mark_fact_has_embedding_sync(conn, fact_id).map_err(Into::into));
        }

        let count =
            db!(pool, |conn| count_facts_without_embeddings_sync(conn).map_err(Into::into));
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_store_fact_embedding() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let id = db!(pool, |conn| store_memory_helper(
            conn,
            Some(project_id),
            Some("embed-key"),
            "content to embed",
            "general",
            None,
            0.5,
        ));

        // Create a 1536-dimensional embedding (matches schema)
        let embedding: Vec<f32> = (0..1536).map(|i| (i as f32) * 0.001).collect();
        let embedding_bytes = embedding_to_bytes(&embedding);
        db!(pool, |conn| store_fact_embedding_sync(
            conn,
            id,
            "content to embed",
            &embedding_bytes
        )
        .map_err(Into::into));

        // Should no longer be in facts without embeddings
        let facts =
            db!(pool, |conn| find_facts_without_embeddings_sync(conn, 10).map_err(Into::into));
        assert!(!facts.iter().any(|f| f.id == id));

        // Memory should be marked
        let results = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project_id),
            "embed",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Scope Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_project_isolation() {
        let (pool, project1) = setup_test_pool_with_project().await;
        let project2 = setup_second_project(&pool).await;

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project1),
            Some("key"),
            "project 1 content",
            "general",
            None,
            0.5,
        ));

        db!(pool, |conn| store_memory_helper(
            conn,
            Some(project2),
            Some("key"),
            "project 2 content",
            "general",
            None,
            0.5,
        ));

        let results1 = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project1),
            "content",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].content, "project 1 content");

        let results2 = db!(pool, |conn| search_memories_sync(
            conn,
            Some(project2),
            "content",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].content, "project 2 content");
    }

    // ═══════════════════════════════════════
    // Empty/Edge Cases
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_empty_search() {
        let pool = setup_test_pool().await;

        let results = db!(pool, |conn| search_memories_sync(
            conn,
            None,
            "nonexistent",
            None,
            10
        )
        .map_err(Into::into));
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_preferences() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let prefs = db!(pool, |conn| get_preferences_memory_sync(conn, Some(project_id))
            .map_err(Into::into));
        assert_eq!(prefs.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_health_alerts() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let alerts = db!(pool, |conn| get_health_alerts_memory_sync(
            conn,
            Some(project_id),
            10
        )
        .map_err(Into::into));
        assert_eq!(alerts.len(), 0);
    }

    #[tokio::test]
    async fn test_empty_global_memories() {
        let pool = setup_test_pool().await;

        let memories =
            db!(pool, |conn| get_global_memories_sync(conn, None, 10).map_err(Into::into));
        assert_eq!(memories.len(), 0);
    }
}
