// db/memory/mod.rs
// Memory storage and retrieval operations

mod query;
mod ranking;
mod recall;
mod store;

use mira_types::MemoryFact;

pub use query::{
    MemoryScopeInfo, clear_project_persona_sync, count_facts_without_embeddings_sync,
    delete_memory_sync, fetch_ranked_memories_for_export_sync, find_facts_without_embeddings_sync,
    get_base_persona_sync, get_global_memories_sync, get_health_alerts_sync,
    get_memory_scope_sync, get_memory_stats_sync, get_preferences_sync,
    get_project_persona_sync, mark_fact_has_embedding_sync,
};
pub use ranking::{RankedMemory, RecallRow};
pub use recall::{recall_semantic_with_entity_boost_sync, record_memory_access_sync, search_memories_sync};
pub use store::{StoreMemoryParams, import_confirmed_memory_sync, store_fact_embedding_sync, store_memory_sync};

/// Parse MemoryFact from a rusqlite Row with standard column order:
/// (id, project_id, key, content, fact_type, category, confidence, created_at,
///  session_count, first_session_id, last_session_id, status, user_id, scope, team_id,
///  updated_at, branch)
pub fn parse_memory_fact_row(row: &rusqlite::Row) -> rusqlite::Result<MemoryFact> {
    Ok(MemoryFact {
        id: row.get(0)?,
        project_id: row.get(1)?,
        key: row.get(2)?,
        content: row.get(3)?,
        fact_type: row.get(4)?,
        category: row.get(5)?,
        confidence: row.get(6)?,
        created_at: row.get(7)?,
        session_count: row.get(8).unwrap_or(1),
        first_session_id: row.get(9).ok(),
        last_session_id: row.get(10).ok(),
        status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
        user_id: row.get(12).ok(),
        scope: row.get(13).unwrap_or_else(|_| "project".to_string()),
        team_id: row.get(14).ok(),
        updated_at: row.get(15).ok(),
        branch: row.get(16).ok(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// SHARED SQL FRAGMENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Scope-filtering WHERE clause for memory queries.
///
/// Returns SQL fragment with parameter placeholders for (project_id, user_id, team_id).
/// `prefix` is the table alias (e.g. "f." for JOINed queries, "" for direct).
/// The caller must bind: project_id as `?{pid}`, user_id as `?{uid}`, team_id as `?{tid}`.
pub fn scope_filter_sql(prefix: &str) -> String {
    format!(
        "({p}project_id = ?{{pid}} OR {p}project_id IS NULL)
           AND (
             {p}scope = 'project'
             OR {p}scope IS NULL
             OR ({p}scope = 'personal' AND {p}user_id = ?{{uid}})
             OR ({p}scope = 'team' AND {p}team_id = ?{{tid}})
           )",
        p = prefix,
    )
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    /// Insert a project row and return its ID
    fn insert_project(conn: &rusqlite::Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (path, name) VALUES ('/test/scope', 'scope-test')",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// Helper: store a memory with given scope parameters, return its ID
    fn store(
        conn: &rusqlite::Connection,
        content: &str,
        scope: &str,
        project_id: Option<i64>,
        user_id: Option<&str>,
        team_id: Option<i64>,
    ) -> i64 {
        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id,
                key: None,
                content,
                fact_type: "general",
                category: None,
                confidence: 0.8,
                session_id: Some("test-session"),
                user_id,
                scope,
                branch: None,
                team_id,
                suspicious: false,
            },
        )
        .expect("store_memory_sync failed")
    }

    /// Helper: store a preference memory
    fn store_pref(
        conn: &rusqlite::Connection,
        content: &str,
        scope: &str,
        project_id: Option<i64>,
        user_id: Option<&str>,
        team_id: Option<i64>,
    ) -> i64 {
        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id,
                key: None,
                content,
                fact_type: "preference",
                category: Some("style"),
                confidence: 0.8,
                session_id: Some("test-session"),
                user_id,
                scope,
                branch: None,
                team_id,
                suspicious: false,
            },
        )
        .expect("store_memory_sync failed")
    }

    // ═══════════════════════════════════════════════════════════════════════
    // search_memories_sync isolation
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn search_alice_sees_project_and_personal() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store(
            &conn,
            "shared scope config",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "alice scope config",
            "personal",
            Some(pid),
            Some("alice"),
            None,
        );
        store(
            &conn,
            "team scope config",
            "team",
            Some(pid),
            None,
            Some(100),
        );

        let results =
            search_memories_sync(&conn, Some(pid), "scope", Some("alice"), None, 10).unwrap();
        let contents: Vec<&str> = results.iter().map(|m| m.content.as_str()).collect();

        assert!(
            contents.contains(&"alice scope config"),
            "alice should see her personal memory"
        );
        assert!(
            contents.contains(&"shared scope config"),
            "alice should see project memory"
        );
        assert!(
            !contents.contains(&"team scope config"),
            "alice (no team) should not see team memory"
        );
    }

    #[test]
    fn search_bob_sees_only_project() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store(
            &conn,
            "shared scope config",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "alice scope config",
            "personal",
            Some(pid),
            Some("alice"),
            None,
        );
        store(
            &conn,
            "team scope config",
            "team",
            Some(pid),
            None,
            Some(100),
        );

        let results =
            search_memories_sync(&conn, Some(pid), "scope", Some("bob"), None, 10).unwrap();
        let contents: Vec<&str> = results.iter().map(|m| m.content.as_str()).collect();

        assert!(
            contents.contains(&"shared scope config"),
            "bob should see project memory"
        );
        assert!(
            !contents.contains(&"alice scope config"),
            "bob should not see alice's personal memory"
        );
        assert!(
            !contents.contains(&"team scope config"),
            "bob (no team) should not see team memory"
        );
    }

    #[test]
    fn search_team_member_sees_project_and_team() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store(
            &conn,
            "shared scope config",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "alice scope config",
            "personal",
            Some(pid),
            Some("alice"),
            None,
        );
        store(
            &conn,
            "team scope config",
            "team",
            Some(pid),
            None,
            Some(100),
        );

        let results =
            search_memories_sync(&conn, Some(pid), "scope", Some("charlie"), Some(100), 10)
                .unwrap();
        let contents: Vec<&str> = results.iter().map(|m| m.content.as_str()).collect();

        assert!(
            contents.contains(&"shared scope config"),
            "team member should see project memory"
        );
        assert!(
            contents.contains(&"team scope config"),
            "team member should see their team memory"
        );
        assert!(
            !contents.contains(&"alice scope config"),
            "charlie should not see alice's personal memory"
        );
    }

    #[test]
    fn search_different_team_sees_only_project() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store(
            &conn,
            "shared scope config",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "team scope config",
            "team",
            Some(pid),
            None,
            Some(100),
        );

        let results =
            search_memories_sync(&conn, Some(pid), "scope", Some("dave"), Some(200), 10).unwrap();
        let contents: Vec<&str> = results.iter().map(|m| m.content.as_str()).collect();

        assert!(
            contents.contains(&"shared scope config"),
            "team-200 member should see project memory"
        );
        assert!(
            !contents.contains(&"team scope config"),
            "team-200 member should not see team-100 memory"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // get_memory_scope_sync roundtrip
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn scope_roundtrip_project() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store(&conn, "project mem", "project", Some(pid), None, None);
        let info = get_memory_scope_sync(&conn, id).unwrap().unwrap();
        assert_eq!(info, (Some(pid), "project".to_string(), None, None));
    }

    #[test]
    fn scope_roundtrip_personal() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store(
            &conn,
            "personal mem",
            "personal",
            Some(pid),
            Some("alice"),
            None,
        );
        let info = get_memory_scope_sync(&conn, id).unwrap().unwrap();
        assert_eq!(
            info,
            (
                Some(pid),
                "personal".to_string(),
                Some("alice".to_string()),
                None
            )
        );
    }

    #[test]
    fn scope_roundtrip_team() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store(&conn, "team mem", "team", Some(pid), None, Some(42));
        let info = get_memory_scope_sync(&conn, id).unwrap().unwrap();
        assert_eq!(info, (Some(pid), "team".to_string(), None, Some(42)));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // get_preferences_sync scope filtering
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn preferences_filtered_by_user() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store_pref(
            &conn,
            "project pref: use tabs",
            "project",
            Some(pid),
            None,
            None,
        );
        store_pref(
            &conn,
            "alice pref: dark mode",
            "personal",
            Some(pid),
            Some("alice"),
            None,
        );
        store_pref(
            &conn,
            "bob pref: light mode",
            "personal",
            Some(pid),
            Some("bob"),
            None,
        );

        let prefs = get_preferences_sync(&conn, Some(pid), Some("alice"), None).unwrap();
        let contents: Vec<&str> = prefs.iter().map(|m| m.content.as_str()).collect();

        assert!(contents.contains(&"project pref: use tabs"));
        assert!(contents.contains(&"alice pref: dark mode"));
        assert!(!contents.contains(&"bob pref: light mode"));
    }

    #[test]
    fn preferences_filtered_by_team() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        store_pref(
            &conn,
            "project pref: use tabs",
            "project",
            Some(pid),
            None,
            None,
        );
        store_pref(
            &conn,
            "team pref: 4-space indent",
            "team",
            Some(pid),
            None,
            Some(10),
        );
        store_pref(
            &conn,
            "other team pref: 2-space",
            "team",
            Some(pid),
            None,
            Some(20),
        );

        let prefs = get_preferences_sync(&conn, Some(pid), None, Some(10)).unwrap();
        let contents: Vec<&str> = prefs.iter().map(|m| m.content.as_str()).collect();

        assert!(contents.contains(&"project pref: use tabs"));
        assert!(contents.contains(&"team pref: 4-space indent"));
        assert!(!contents.contains(&"other team pref: 2-space"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // store_memory_sync key upsert respects scope boundary
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn key_upsert_same_key_different_scopes_separate_rows() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        let id_project = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("theme"),
                content: "project theme: blue",
                fact_type: "preference",
                category: None,
                confidence: 0.8,
                session_id: Some("s1"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        let id_personal = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("theme"),
                content: "alice theme: red",
                fact_type: "preference",
                category: None,
                confidence: 0.8,
                session_id: Some("s1"),
                user_id: Some("alice"),
                scope: "personal",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        let id_team = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("theme"),
                content: "team theme: green",
                fact_type: "preference",
                category: None,
                confidence: 0.8,
                session_id: Some("s1"),
                user_id: None,
                scope: "team",
                branch: None,
                team_id: Some(10),
                suspicious: false,
            },
        )
        .unwrap();

        // All three should be distinct rows
        assert_ne!(id_project, id_personal);
        assert_ne!(id_personal, id_team);
        assert_ne!(id_project, id_team);

        // Verify content is preserved (no cross-scope overwrite)
        let scope_project = get_memory_scope_sync(&conn, id_project).unwrap().unwrap();
        assert_eq!(scope_project.1, "project");

        let scope_personal = get_memory_scope_sync(&conn, id_personal).unwrap().unwrap();
        assert_eq!(scope_personal.1, "personal");
        assert_eq!(scope_personal.2, Some("alice".to_string()));

        let scope_team = get_memory_scope_sync(&conn, id_team).unwrap().unwrap();
        assert_eq!(scope_team.1, "team");
        assert_eq!(scope_team.3, Some(10));
    }

    #[test]
    fn key_upsert_same_scope_updates_in_place() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        let id1 = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("setting"),
                content: "original value",
                fact_type: "general",
                category: None,
                confidence: 0.8,
                session_id: Some("s1"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        // Same key, same scope — should update, not create new row
        let id2 = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("setting"),
                content: "updated value",
                fact_type: "general",
                category: None,
                confidence: 0.9,
                session_id: Some("s2"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        assert_eq!(id1, id2, "same key + same scope should upsert in place");
    }

    #[test]
    fn test_multi_keyword_search_ranks_by_match_count() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        // Store memories with different keyword overlap
        store(
            &conn,
            "database connection pooling is important",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "connection timeout configuration",
            "project",
            Some(pid),
            None,
            None,
        );
        store(
            &conn,
            "database schema migration strategy for connection pooling",
            "project",
            Some(pid),
            None,
            None,
        );

        // Multi-word query: "database connection pooling"
        // Keywords > 3 chars: ["database", "connection", "pooling"]
        let results = search_memories_sync(
            &conn,
            Some(pid),
            "database connection pooling",
            None,
            None,
            10,
        )
        .unwrap();

        assert!(
            !results.is_empty(),
            "multi-keyword search should find results"
        );

        // Memory matching all 3 keywords should rank first
        assert!(
            results[0].content.contains("database") && results[0].content.contains("pooling"),
            "first result should match the most keywords"
        );
    }

    #[test]
    fn test_short_query_falls_back_to_full_string() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store(
            &conn,
            "use bun for package management",
            "project",
            Some(pid),
            None,
            None,
        );

        // Query "use bun" has no words > 3 chars, falls back to full-string LIKE
        let results = search_memories_sync(&conn, Some(pid), "use bun", None, None, 10).unwrap();

        assert!(
            !results.is_empty(),
            "short-word query should use full-string fallback"
        );
        assert!(results[0].content.contains("use bun"));
    }
}
