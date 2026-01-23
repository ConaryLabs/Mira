// crates/mira-server/src/db/memory_tests.rs
// Tests for memory storage and retrieval operations

use super::*;

/// Helper to create a test database with a project
fn setup_test_db() -> (Database, i64) {
    let db = Database::open_in_memory().expect("Failed to open in-memory db");
    let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();
    (db, project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // Basic CRUD Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_store_memory_basic() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory(
                Some(project_id),
                Some("test-key"),
                "test content",
                "general",
                None,
                1.0,
            )
            .unwrap();

        assert!(id > 0);

        // Verify storage
        let results = db.search_memories(Some(project_id), "test", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test content");
        assert_eq!(results[0].key, Some("test-key".to_string()));
        assert_eq!(results[0].fact_type, "general");
    }

    #[test]
    fn test_store_memory_without_key() {
        let (db, _project_id) = setup_test_db();

        let _id = db
            .store_memory(None, None, "content without key", "general", None, 0.8)
            .unwrap();

        assert!(_id > 0);
        assert_eq!(_id, 1); // First memory
    }

    #[test]
    fn test_store_memory_with_all_fields() {
        let (db, project_id) = setup_test_db();

        let _id = db
            .store_memory(
                Some(project_id),
                Some("full-key"),
                "full content",
                "decision",
                Some("architecture"),
                0.95,
            )
            .unwrap();

        let results = db.search_memories(Some(project_id), "full", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_type, "decision");
        assert_eq!(results[0].category, Some("architecture".to_string()));
        assert!((results[0].confidence - 0.5).abs() < 0.01); // Initial confidence capped at 0.5 for candidates
    }

    // ═══════════════════════════════════════
    // Upsert Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_upsert_by_key_same_session() {
        let (db, project_id) = setup_test_db();

        // Store initial memory
        let id1 = db
            .store_memory(
                Some(project_id),
                Some("upsert-key"),
                "initial content",
                "general",
                None,
                0.5,
            )
            .unwrap();

        // Update with same session (no session_id provided)
        let id2 = db
            .store_memory(
                Some(project_id),
                Some("upsert-key"),
                "updated content",
                "decision",
                Some("architecture"),
                0.8,
            )
            .unwrap();

        // Should be same ID
        assert_eq!(id1, id2);

        // Verify update
        let results = db.search_memories(Some(project_id), "updated", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "updated content");
        assert_eq!(results[0].fact_type, "decision");
        assert_eq!(results[0].session_count, 1); // No new session
    }

    #[test]
    fn test_upsert_by_key_different_session() {
        let (db, project_id) = setup_test_db();

        // Store initial memory with session
        let id1 = db
            .store_memory_with_session(
                Some(project_id),
                Some("session-key"),
                "initial",
                "general",
                None,
                0.5,
                Some("session-1"),
            )
            .unwrap();

        // Update with different session
        let id2 = db
            .store_memory_with_session(
                Some(project_id),
                Some("session-key"),
                "updated",
                "decision",
                None,
                0.7,
                Some("session-2"),
            )
            .unwrap();

        assert_eq!(id1, id2);

        let results = db.search_memories(Some(project_id), "updated", 10).unwrap();
        assert_eq!(results[0].session_count, 2); // Incremented
        assert_eq!(results[0].first_session_id.as_deref(), Some("session-1"));
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-2"));
    }

    #[test]
    fn test_upsert_promotes_after_three_sessions() {
        let (db, project_id) = setup_test_db();

        let key = "promotion-key";

        // Session 1
        db.store_memory_with_session(
            Some(project_id),
            Some(key),
            "content",
            "general",
            None,
            0.5,
            Some("s1"),
        )
        .unwrap();

        // Session 2
        db.store_memory_with_session(
            Some(project_id),
            Some(key),
            "content v2",
            "general",
            None,
            0.5,
            Some("s2"),
        )
        .unwrap();

        // Session 3 - should promote
        db.store_memory_with_session(
            Some(project_id),
            Some(key),
            "content v3",
            "general",
            None,
            0.5,
            Some("s3"),
        )
        .unwrap();

        let results = db.search_memories(Some(project_id), "content", 10).unwrap();
        assert_eq!(results[0].status, "confirmed");
        assert!(results[0].confidence > 0.5); // Should have increased
    }

    // ═══════════════════════════════════════
    // Session Tracking Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_record_memory_access_new_session() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory(
                Some(project_id),
                Some("access-key"),
                "content",
                "general",
                None,
                0.5,
            )
            .unwrap();

        // Record access from new session
        db.record_memory_access(id, "session-new").unwrap();

        let results = db.search_memories(Some(project_id), "content", 10).unwrap();
        assert_eq!(results[0].session_count, 2);
        assert_eq!(results[0].last_session_id.as_deref(), Some("session-new"));
    }

    #[test]
    fn test_record_memory_access_same_session() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory_with_session(
                Some(project_id),
                Some("access-key2"),
                "content",
                "general",
                None,
                0.5,
                Some("session-1"),
            )
            .unwrap();

        // Same session - should not increment
        db.record_memory_access(id, "session-1").unwrap();

        let results = db.search_memories(Some(project_id), "content", 10).unwrap();
        assert_eq!(results[0].session_count, 1); // Unchanged
    }

    // ═══════════════════════════════════════
    // Statistics Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_memory_stats_empty() {
        let (db, project_id) = setup_test_db();

        let (candidates, confirmed) = db.get_memory_stats(Some(project_id)).unwrap();
        assert_eq!(candidates, 0);
        assert_eq!(confirmed, 0);
    }

    #[test]
    fn test_get_memory_stats_with_data() {
        let (db, project_id) = setup_test_db();

        // Add some candidates
        for i in 0..3 {
            db.store_memory(
                Some(project_id),
                Some(&format!("key-{}", i)),
                &format!("content {}", i),
                "general",
                None,
                0.5,
            )
            .unwrap();
        }

        let (candidates, confirmed) = db.get_memory_stats(Some(project_id)).unwrap();
        assert_eq!(candidates, 3);
        assert_eq!(confirmed, 0);
    }

    #[test]
    fn test_get_memory_stats_global() {
        let db = Database::open_in_memory().unwrap();

        db.store_global_memory("global fact", "test", None, None)
            .unwrap();

        let (candidates, _confirmed) = db.get_memory_stats(None).unwrap();
        assert_eq!(candidates, 1);
    }

    // ═══════════════════════════════════════
    // Search Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_search_memories_basic() {
        let (db, project_id) = setup_test_db();

        db.store_memory(
            Some(project_id),
            Some("key1"),
            "the quick brown fox",
            "general",
            None,
            0.5,
        )
        .unwrap();

        db.store_memory(
            Some(project_id),
            Some("key2"),
            "lazy dog jumps",
            "general",
            None,
            0.5,
        )
        .unwrap();

        let results = db.search_memories(Some(project_id), "fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("fox"));
    }

    #[test]
    fn test_search_memories_limit() {
        let (db, project_id) = setup_test_db();

        for i in 0..10 {
            db.store_memory(
                Some(project_id),
                Some(&format!("key-{}", i)),
                "test content",
                "general",
                None,
                0.5,
            )
            .unwrap();
        }

        let results = db.search_memories(Some(project_id), "test", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_search_memories_sql_injection_escape() {
        let (db, project_id) = setup_test_db();

        db.store_memory(
            Some(project_id),
            Some("key"),
            "100% complete",
            "general",
            None,
            0.5,
        )
        .unwrap();

        // % is a SQL wildcard - should be escaped
        let results = db
            .search_memories(Some(project_id), "100%", 10)
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_memories_underscore_escape() {
        let (db, project_id) = setup_test_db();

        db.store_memory(
            Some(project_id),
            Some("key"),
            "test_value",
            "general",
            None,
            0.5,
        )
        .unwrap();

        // _ is a SQL wildcard - should be escaped
        let results = db.search_memories(Some(project_id), "test_value", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Preferences Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_preferences() {
        let (db, project_id) = setup_test_db();

        // Add some preferences
        db.store_memory(
            Some(project_id),
            Some("pref-1"),
            "pref content 1",
            "preference",
            Some("ui"),
            0.8,
        )
        .unwrap();

        db.store_memory(
            Some(project_id),
            Some("pref-2"),
            "pref content 2",
            "preference",
            Some("editor"),
            0.9,
        )
        .unwrap();

        // Add non-preference
        db.store_memory(
            Some(project_id),
            Some("other"),
            "other content",
            "general",
            None,
            0.5,
        )
        .unwrap();

        let prefs = db.get_preferences(Some(project_id)).unwrap();
        assert_eq!(prefs.len(), 2);
        assert!(prefs.iter().all(|p| p.fact_type == "preference"));
    }

    // ═══════════════════════════════════════
    // Delete Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_delete_memory() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory(
                Some(project_id),
                Some("delete-key"),
                "to be deleted",
                "general",
                None,
                0.5,
            )
            .unwrap();

        let deleted = db.delete_memory(id).unwrap();
        assert!(deleted);

        let results = db.search_memories(Some(project_id), "deleted", 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_delete_memory_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let deleted = db.delete_memory(99999).unwrap();
        assert!(!deleted);
    }

    // ═══════════════════════════════════════
    // Health Alerts Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_health_alerts() {
        let (db, project_id) = setup_test_db();

        // Add high-confidence health alert
        db.store_memory(
            Some(project_id),
            Some("health-1"),
            "critical issue found",
            "health",
            Some("security"),
            0.9,
        )
        .unwrap();

        // Add low-confidence (should not appear)
        db.store_memory(
            Some(project_id),
            Some("health-2"),
            "minor issue",
            "health",
            None,
            0.5,
        )
        .unwrap();

        let alerts = db.get_health_alerts(Some(project_id), 10).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].confidence, 0.9); // Initial capped to 0.5 for candidates, but set to 0.9
    }

    #[test]
    fn test_get_health_alerts_threshold() {
        let (db, project_id) = setup_test_db();

        // Add alerts with different confidence levels
        for conf in [0.6, 0.7, 0.8, 0.9] {
            db.store_memory(
                Some(project_id),
                Some(&format!("health-{}", conf)),
                "alert",
                "health",
                None,
                conf,
            )
            .unwrap();
        }

        let alerts = db.get_health_alerts(Some(project_id), 10).unwrap();
        // Only 0.7 and above should be included
        assert!(alerts.len() >= 2);
    }

    // ═══════════════════════════════════════
    // Global Memory Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_store_global_memory() {
        let db = Database::open_in_memory().unwrap();

        let id = db
            .store_global_memory("user prefers dark mode", "ui", None, None)
            .unwrap();

        assert!(id > 0);

        let results = db.get_global_memories(None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_type, "personal");
    }

    #[test]
    fn test_get_global_memories_with_category() {
        let db = Database::open_in_memory().unwrap();

        db.store_global_memory("fact 1", "category-a", None, None)
            .unwrap();
        db.store_global_memory("fact 2", "category-a", None, None)
            .unwrap();
        db.store_global_memory("fact 3", "category-b", None, None)
            .unwrap();

        let cat_a = db.get_global_memories(Some("category-a"), 10).unwrap();
        assert_eq!(cat_a.len(), 2);

        let cat_b = db.get_global_memories(Some("category-b"), 10).unwrap();
        assert_eq!(cat_b.len(), 1);
    }

    #[test]
    fn test_get_user_profile() {
        let db = Database::open_in_memory().unwrap();

        db.store_global_memory("name: Alice", "profile", None, None)
            .unwrap();
        db.store_global_memory("role: Developer", "profile", None, None)
            .unwrap();
        db.store_global_memory("likes coffee", "preference", None, None)
            .unwrap();

        let profile = db.get_user_profile().unwrap();
        assert_eq!(profile.len(), 2);
        assert!(profile.iter().all(|p| p.category.as_deref() == Some("profile")));
    }

    // ═══════════════════════════════════════
    // Persona Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_base_persona() {
        let db = Database::open_in_memory().unwrap();

        assert!(db.get_base_persona().unwrap().is_none());

        db.set_base_persona("You are a helpful assistant").unwrap();

        let persona = db.get_base_persona().unwrap().unwrap();
        assert_eq!(persona, "You are a helpful assistant");
    }

    #[test]
    fn test_project_persona() {
        let (db, project_id) = setup_test_db();

        assert!(db
            .get_project_persona(project_id)
            .unwrap()
            .is_none());

        db.set_project_persona(project_id, "Project-specific persona")
            .unwrap();

        let persona = db.get_project_persona(project_id).unwrap().unwrap();
        assert_eq!(persona, "Project-specific persona");
    }

    #[test]
    fn test_clear_project_persona() {
        let (db, project_id) = setup_test_db();

        db.set_project_persona(project_id, "Persona").unwrap();
        assert!(db
            .get_project_persona(project_id)
            .unwrap()
            .is_some());

        let cleared = db.clear_project_persona(project_id).unwrap();
        assert!(cleared);
        assert!(db
            .get_project_persona(project_id)
            .unwrap()
            .is_none());
    }

    // ═══════════════════════════════════════
    // Embedding Status Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_mark_and_find_facts_without_embeddings() {
        let (db, project_id) = setup_test_db();

        // Add some memories
        for i in 0..3 {
            db.store_memory(
                Some(project_id),
                Some(&format!("key-{}", i)),
                &format!("content {}", i),
                "general",
                None,
                0.5,
            )
            .unwrap();
        }

        // All should be without embeddings
        let facts = db.find_facts_without_embeddings(10).unwrap();
        assert_eq!(facts.len(), 3);

        // Mark one as having embedding
        db.mark_fact_has_embedding(facts[0].id).unwrap();

        let remaining = db.find_facts_without_embeddings(10).unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn test_count_facts_without_embeddings() {
        let (db, project_id) = setup_test_db();

        for i in 0..5 {
            db.store_memory(
                Some(project_id),
                Some(&format!("key-{}", i)),
                "content",
                "general",
                None,
                0.5,
            )
            .unwrap();
        }

        let count = db.count_facts_without_embeddings().unwrap();
        assert_eq!(count, 5);

        // Mark some
        let facts = db.find_facts_without_embeddings(3).unwrap();
        for fact in &facts {
            db.mark_fact_has_embedding(fact.id).unwrap();
        }

        let count = db.count_facts_without_embeddings().unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_store_fact_embedding() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory(
                Some(project_id),
                Some("embed-key"),
                "content to embed",
                "general",
                None,
                0.5,
            )
            .unwrap();

        // Create a 1536-dimensional embedding (matching text-embedding-3-small)
        let embedding: Vec<f32> = (0..1536).map(|i| (i as f32) * 0.001).collect();
        db.store_fact_embedding(id, "content to embed", &embedding)
            .unwrap();

        // Should no longer be in facts without embeddings
        let facts = db.find_facts_without_embeddings(10).unwrap();
        assert!(!facts.iter().any(|f| f.id == id));

        // Memory should be marked
        let results = db.search_memories(Some(project_id), "embed", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ═══════════════════════════════════════
    // Scope Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_project_isolation() {
        let (db, project1) = setup_test_db();
        let (project2, _) = db.get_or_create_project("/other/path", Some("other")).unwrap();

        db.store_memory(
            Some(project1),
            Some("key"),
            "project 1 content",
            "general",
            None,
            0.5,
        )
        .unwrap();

        db.store_memory(
            Some(project2),
            Some("key"),
            "project 2 content",
            "general",
            None,
            0.5,
        )
        .unwrap();

        let results1 = db.search_memories(Some(project1), "content", 10).unwrap();
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].content, "project 1 content");

        let results2 = db.search_memories(Some(project2), "content", 10).unwrap();
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].content, "project 2 content");
    }

    // ═══════════════════════════════════════
    // Row Parsing Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_parse_memory_fact_row() {
        let db = Database::open_in_memory().unwrap();

        db.store_memory(
            None,
            Some("parse-test"),
            "test content for parsing",
            "general",
            Some("category"),
            0.75,
        )
        .unwrap();

        // search_memories searches content field, not key
        let results = db.search_memories(None, "test content for parsing", 1).unwrap();
        assert_eq!(results.len(), 1);

        let fact = &results[0];
        assert_eq!(fact.content, "test content for parsing");
        assert_eq!(fact.key, Some("parse-test".to_string()));
        assert_eq!(fact.fact_type, "general");
        assert_eq!(fact.category, Some("category".to_string()));
        assert!((fact.confidence - 0.5).abs() < 0.01); // Capped for candidates
        assert_eq!(fact.status, "candidate");
        assert_eq!(fact.scope, "project"); // Default
    }

    // ═══════════════════════════════════════
    // Confidence Capping Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_new_memory_confidence_capped() {
        let (db, project_id) = setup_test_db();

        // High confidence should be capped to 0.5 for new memories
        db.store_memory(
            Some(project_id),
            Some("key"),
            "content",
            "general",
            None,
            1.0,
        )
        .unwrap();

        let results = db.search_memories(Some(project_id), "content", 10).unwrap();
        assert!((results[0].confidence - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_low_confidence_preserved() {
        let (db, project_id) = setup_test_db();

        db.store_memory(
            Some(project_id),
            Some("key"),
            "content",
            "general",
            None,
            0.3,
        )
        .unwrap();

        let results = db.search_memories(Some(project_id), "content", 10).unwrap();
        assert!((results[0].confidence - 0.3).abs() < 0.01);
    }

    // ═══════════════════════════════════════
    // Update Timestamp Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_updated_at_on_upsert() {
        let (db, project_id) = setup_test_db();

        let id = db
            .store_memory(
                Some(project_id),
                Some("update-key"),
                "initial",
                "general",
                None,
                0.5,
            )
            .unwrap();

        // Get created_at
        let results = db.search_memories(Some(project_id), "initial", 10).unwrap();
        let _created_at = results[0].created_at.clone();

        // Wait a bit and update
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.store_memory(
            Some(project_id),
            Some("update-key"),
            "updated",
            "general",
            None,
            0.6,
        )
        .unwrap();

        let updated = db.search_memories(Some(project_id), "updated", 10).unwrap();
        assert_eq!(updated[0].id, id);
        assert_eq!(updated[0].content, "updated");
        // updated_at should be newer than created_at
    }

    // ═══════════════════════════════════════
    // Empty/Edge Cases
    // ═══════════════════════════════════════

    #[test]
    fn test_empty_search() {
        let db = Database::open_in_memory().unwrap();

        let results = db.search_memories(None, "nonexistent", 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_empty_preferences() {
        let (db, project_id) = setup_test_db();

        let prefs = db.get_preferences(Some(project_id)).unwrap();
        assert_eq!(prefs.len(), 0);
    }

    #[test]
    fn test_empty_health_alerts() {
        let (db, project_id) = setup_test_db();

        let alerts = db.get_health_alerts(Some(project_id), 10).unwrap();
        assert_eq!(alerts.len(), 0);
    }

    #[test]
    fn test_empty_global_memories() {
        let db = Database::open_in_memory().unwrap();

        let memories = db.get_global_memories(None, 10).unwrap();
        assert_eq!(memories.len(), 0);
    }
}
