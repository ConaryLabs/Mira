// db/mod.rs
// Unified database layer with rusqlite + sqlite-vec

mod background;
mod cartographer;
mod chat;
mod config;
mod diff_analysis;
pub mod documentation;
mod embeddings;
mod index;
mod memory;
#[cfg(test)]
mod memory_tests;
mod migration_helpers;
mod milestones;
pub mod pool;
mod project;
#[cfg(test)]
mod project_tests;
mod reviews;
mod schema;
mod search;
mod session;
#[cfg(test)]
mod session_tests;
mod tasks;
#[cfg(test)]
mod tasks_tests;
mod teams;
mod types;
mod usage;

pub use background::{
    clear_old_capabilities_sync,
    clear_old_health_issues_sync,
    delete_memory_by_key_sync,
    delete_pending_embedding_sync,
    get_documented_by_category_sync,
    get_error_heavy_functions_sync,
    // Code health analysis
    get_large_functions_sync,
    get_lib_symbols_sync,
    get_modules_for_doc_gaps_sync,
    // Permission hooks
    get_permission_rules_sync,
    // Summaries processor
    get_projects_with_pending_summaries_sync,
    get_scan_info_sync,
    get_symbols_for_file_sync,
    get_unused_functions_sync,
    insert_system_marker_sync,
    is_time_older_than_sync,
    // Diff analysis
    map_files_to_symbols_sync,
    mark_health_scanned_sync,
    memory_key_exists_sync,
    store_code_embedding_sync,
};
pub use cartographer::{
    count_cached_modules_sync, count_symbols_in_path_sync, get_cached_modules_sync,
    get_external_deps_sync, get_module_dependencies_sync, get_module_exports_sync,
    get_modules_needing_summaries_sync, update_module_purposes_sync, upsert_module_sync,
};
pub use chat::get_last_chat_time_sync;
pub use config::{
    EmbeddingModelCheck, ExpertConfig, delete_custom_prompt_sync, get_expert_config_sync,
    list_custom_prompts_sync, set_expert_config_sync,
};
pub use diff_analysis::{
    DiffAnalysis, get_cached_diff_analysis_sync, get_recent_diff_analyses_sync,
    store_diff_analysis_sync,
};
pub use documentation::{DocGap, DocInventory, DocTask, get_inventory_for_stale_check};
pub use embeddings::{PendingEmbedding, get_pending_embeddings_sync};
pub use index::{
    CallInsert,
    ImportInsert,
    // Batch insert operations
    SymbolInsert,
    clear_file_index_sync,
    clear_modules_without_purpose_sync,
    clear_project_index_sync,
    count_embedded_chunks_sync,
    count_symbols_sync,
    insert_call_sync,
    insert_chunk_embedding_sync,
    insert_import_sync,
    insert_symbol_sync,
    queue_pending_embedding_sync,
};
pub use memory::{
    StoreMemoryParams,
    delete_memory_sync,
    import_confirmed_memory_sync,
    parse_memory_fact_row,
    recall_semantic_sync,
    recall_semantic_with_branch_info_sync,
    record_memory_access_sync,
    search_capabilities_sync,
    search_memories_sync,
    store_embedding_sync,
    store_fact_embedding_sync,
    // Sync functions for pool.interact() usage
    store_memory_sync,
};
pub use milestones::{
    calculate_goal_progress_sync, complete_milestone_sync, create_milestone_sync,
    delete_milestone_sync, get_milestone_by_id_sync, get_milestones_for_goal_sync,
    parse_milestone_row, update_goal_progress_from_milestones_sync, update_milestone_sync,
};
pub use project::{
    get_health_alerts_sync, get_indexed_projects_sync, get_last_active_project_sync,
    get_or_create_project_sync, get_preferences_sync, get_project_briefing_sync,
    get_project_info_sync, get_projects_for_briefing_check_sync, get_server_state_sync,
    mark_session_for_briefing_sync, save_active_project_sync, search_memories_text_sync,
    set_server_state_sync, update_project_briefing_sync, update_project_name_sync,
    upsert_session_sync, upsert_session_with_branch_sync,
};
pub use reviews::{
    Correction, ReviewFinding, bulk_update_finding_status_sync,
    extract_patterns_from_findings_sync, get_finding_stats_sync, get_finding_sync,
    get_findings_sync, get_relevant_corrections_sync, store_review_finding_sync,
    update_finding_status_sync,
};
pub use search::{
    ChunkSearchResult, CrossRefResult, FtsSearchResult, SemanticCodeResult, SymbolSearchResult,
    chunk_like_search_sync, find_callees_sync, find_callers_sync, fts_search_sync,
    get_symbol_bounds_sync, semantic_code_search_sync, symbol_like_search_sync,
};
pub use session::{
    build_session_recap_sync, close_session_sync, create_session_sync, get_recent_sessions_sync,
    get_session_history_sync, get_session_stats_sync, get_session_tool_summary_sync,
    get_sessions_needing_summary_sync, get_stale_sessions_sync, log_tool_call_sync,
    update_session_summary_sync,
};
pub use tasks::{
    create_goal_sync, create_task_sync, delete_goal_sync, delete_task_sync, get_active_goals_sync,
    get_goal_by_id_sync, get_goals_sync, get_pending_tasks_sync, get_task_by_id_sync,
    get_tasks_sync, parse_goal_row, parse_task_row, update_goal_sync, update_task_sync,
};
pub use teams::{
    Team, TeamMember, add_team_member_sync, create_team_sync, get_team_by_name_sync, get_team_sync,
    is_team_member_sync, list_team_members_sync, list_user_teams_sync, remove_team_member_sync,
};
pub use types::*;
pub use usage::{
    EmbeddingUsageRecord, LlmUsageRecord, UsageStats, get_llm_usage_summary,
    insert_embedding_usage_sync, insert_llm_usage_sync, query_llm_usage_stats,
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;
use std::sync::Mutex;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Database wrapper with sqlite-vec support
pub struct Database {
    conn: Mutex<Connection>,
    path: Option<String>,
}

impl Database {
    /// Open database at path, creating if needed
    #[allow(clippy::missing_transmute_annotations)]
    pub fn open(path: &Path) -> Result<Self> {
        // Register sqlite-vec extension before opening
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }

        // Ensure parent directory exists with secure permissions
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(parent)?.permissions();
                perms.set_mode(0o700); // rwx------
                std::fs::set_permissions(parent, perms)?;
            }
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {:?}", path))?;

        // Set database file permissions to prevent other users from reading
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600); // rw-------
            std::fs::set_permissions(path, perms)?;
        }

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
            path: Some(path.to_string_lossy().into_owned()),
        };

        // Initialize schema
        db.init_schema()?;

        Ok(db)
    }

    /// Open in-memory database (for testing)
    #[allow(clippy::missing_transmute_annotations)]
    pub fn open_in_memory() -> Result<Self> {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Open a shared in-memory database by URI (for testing, to share with pool)
    #[allow(clippy::missing_transmute_annotations)]
    pub fn open_in_memory_shared(uri: &str) -> Result<Self> {
        use rusqlite::OpenFlags;

        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_SHARED_CACHE,
        )?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        // Schema should already be initialized by the pool, but ensure it's there
        db.init_schema()?;
        Ok(db)
    }

    /// Get a lock on the connection
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Initialize schema (idempotent)
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn();
        schema::run_all_migrations(&conn)
    }

    /// Rebuild FTS5 search index from vec_code
    /// Call after indexing completes
    pub fn rebuild_fts(&self) -> Result<()> {
        let conn = self.conn();
        schema::rebuild_code_fts(&conn)
    }

    /// Rebuild FTS5 search index for a specific project
    pub fn rebuild_fts_for_project(&self, project_id: i64) -> Result<()> {
        let conn = self.conn();
        schema::rebuild_code_fts_for_project(&conn, project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().expect("Failed to open in-memory db");
        let (project_id, name) = db
            .get_or_create_project("/test/path", Some("test"))
            .unwrap();
        assert!(project_id > 0);
        assert_eq!(name, Some("test".to_string()));
    }

    #[test]
    fn test_memory_operations() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _name) = db.get_or_create_project("/test", None).unwrap();

        // Store
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

        // Search
        let results = db.search_memories(Some(project_id), "test", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test content");

        // Delete
        assert!(db.delete_memory(id).unwrap());
    }
}
