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
mod search;
#[cfg(test)]
mod memory_tests;
pub mod pool;
mod project;
#[cfg(test)]
mod project_tests;
mod reviews;
mod schema;
mod session;
#[cfg(test)]
mod session_tests;
mod tasks;
#[cfg(test)]
mod tasks_tests;
mod teams;
mod types;
mod proxy;

pub use cartographer::{
    count_cached_modules_sync,
    get_cached_modules_sync,
    get_module_exports_sync,
    count_symbols_in_path_sync,
    get_module_dependencies_sync,
    upsert_module_sync,
    get_external_deps_sync,
    get_modules_needing_summaries_sync,
    update_module_purposes_sync,
};
pub use config::{EmbeddingModelCheck, ExpertConfig};
pub use index::{
    clear_project_index_sync,
    clear_file_index_sync,
    count_symbols_sync,
    count_embedded_chunks_sync,
    clear_modules_without_purpose_sync,
    // Batch insert operations
    SymbolInsert, ImportInsert, CallInsert,
    insert_symbol_sync, insert_import_sync, insert_call_sync,
    insert_chunk_embedding_sync, queue_pending_embedding_sync,
};
pub use search::{
    CrossRefResult,
    find_callers_sync,
    find_callees_sync,
    get_symbol_bounds_sync,
    FtsSearchResult,
    fts_search_sync,
    ChunkSearchResult,
    chunk_like_search_sync,
    SymbolSearchResult,
    symbol_like_search_sync,
    SemanticCodeResult,
    semantic_code_search_sync,
};
pub use diff_analysis::{DiffAnalysis, store_diff_analysis_sync, get_cached_diff_analysis_sync, get_recent_diff_analyses_sync};
pub use proxy::{EmbeddingUsageRecord, EmbeddingUsageSummary, UsageSummaryRow, UsageTotals, insert_proxy_usage_sync, insert_embedding_usage_sync};
pub use documentation::{DocGap, DocInventory, DocTask, get_inventory_for_stale_check};
pub use embeddings::{PendingEmbedding, get_pending_embeddings_sync};
pub use memory::{
    parse_memory_fact_row,
    // Sync functions for pool.interact() usage
    store_memory_sync, StoreMemoryParams,
    store_embedding_sync, store_fact_embedding_sync,
    import_confirmed_memory_sync,
    search_capabilities_sync,
    recall_semantic_sync,
    search_memories_sync,
    record_memory_access_sync,
    delete_memory_sync,
};
pub use reviews::{Correction, ReviewFinding};
pub use teams::{Team, TeamMember};
pub use types::*;
pub use tasks::{
    parse_task_row, parse_goal_row,
    get_pending_tasks_sync, get_task_by_id_sync, get_active_goals_sync,
    create_task_sync, get_tasks_sync, update_task_sync, delete_task_sync,
    get_goal_by_id_sync, create_goal_sync, get_goals_sync, update_goal_sync, delete_goal_sync,
};
pub use session::{
    create_session_sync, get_recent_sessions_sync, get_session_history_sync,
};
pub use project::{
    get_or_create_project_sync,
    update_project_name_sync,
    upsert_session_sync,
    get_indexed_projects_sync,
    search_memories_text_sync,
    get_preferences_sync,
    get_health_alerts_sync,
    get_projects_for_briefing_check_sync,
    update_project_briefing_sync,
    set_server_state_sync,
    get_server_state_sync,
};
pub use background::{
    get_scan_info_sync,
    is_time_older_than_sync,
    memory_key_exists_sync,
    delete_memory_by_key_sync,
    insert_system_marker_sync,
    clear_old_capabilities_sync,
    mark_health_scanned_sync,
    clear_old_health_issues_sync,
    get_documented_by_category_sync,
    get_lib_symbols_sync,
    get_modules_for_doc_gaps_sync,
    get_symbols_for_file_sync,
    store_code_embedding_sync,
    delete_pending_embedding_sync,
    // Code health analysis
    get_large_functions_sync,
    get_error_heavy_functions_sync,
    get_unused_functions_sync,
    // Diff analysis
    map_files_to_symbols_sync,
    // Summaries processor
    get_projects_with_pending_summaries_sync,
    // Permission hooks
    get_permission_rules_sync,
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

        // Create tables first
        conn.execute_batch(schema::SCHEMA)?;

        // Check if vec tables need migration (dimension change)
        schema::migrate_vec_tables(&conn)?;

        // Add start_line column to pending_embeddings if missing
        schema::migrate_pending_embeddings_line_numbers(&conn)?;

        // Add start_line column to vec_code if missing (drops and recreates)
        schema::migrate_vec_code_line_numbers(&conn)?;

        // Add full_result column to tool_history if missing
        schema::migrate_tool_history_full_result(&conn)?;

        // Add project_id column to chat_summaries if missing
        schema::migrate_chat_summaries_project_id(&conn)?;

        // Add summary_id column to chat_messages for reversible summarization
        schema::migrate_chat_messages_summary_id(&conn)?;

        // Add has_embedding column to memory_facts for tracking embedding status
        schema::migrate_memory_facts_has_embedding(&conn)?;

        // Add evidence-based tracking columns to memory_facts
        schema::migrate_memory_facts_evidence_tracking(&conn)?;

        // Add provider and model columns to system_prompts for multi-LLM support
        schema::migrate_system_prompts_provider(&conn)?;
        // Strip old TOOL_USAGE_PROMPT suffix from system prompts for KV cache optimization
        schema::migrate_system_prompts_strip_tool_suffix(&conn)?;

        // Add FTS5 full-text search table if missing
        schema::migrate_code_fts(&conn)?;

        // Add unique constraint to imports table for deduplication
        schema::migrate_imports_unique(&conn)?;

        // Add documentation tracking tables
        schema::migrate_documentation_tables(&conn)?;

        // Add users table for multi-user support
        schema::migrate_users_table(&conn)?;

        // Add user_id, scope, team_id columns to memory_facts
        schema::migrate_memory_user_scope(&conn)?;

        // Add teams tables for team-based memory sharing
        schema::migrate_teams_tables(&conn)?;

        // Add review findings table for code review learning loop
        schema::migrate_review_findings_table(&conn)?;

        // Add learning columns to corrections table
        schema::migrate_corrections_learning_columns(&conn)?;

        // Add diff analyses table for semantic diff analysis
        schema::migrate_diff_analyses_table(&conn)?;

        Ok(())
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
        let (project_id, name) = db.get_or_create_project("/test/path", Some("test")).unwrap();
        assert!(project_id > 0);
        assert_eq!(name, Some("test".to_string()));
    }

    #[test]
    fn test_memory_operations() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _name) = db.get_or_create_project("/test", None).unwrap();

        // Store
        let id = db.store_memory(Some(project_id), Some("test-key"), "test content", "general", None, 1.0).unwrap();
        assert!(id > 0);

        // Search
        let results = db.search_memories(Some(project_id), "test", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test content");

        // Delete
        assert!(db.delete_memory(id).unwrap());
    }
}
