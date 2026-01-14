// db/mod.rs
// Unified database layer with rusqlite + sqlite-vec

mod chat;
mod embeddings;
mod memory;
mod project;
mod schema;
mod session;
mod tasks;
mod types;

pub use embeddings::PendingEmbedding;
pub use memory::parse_memory_fact_row;
pub use types::*;
pub use tasks::{parse_task_row, parse_goal_row};

use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

    /// Get a lock on the connection
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("Database mutex poisoned")
    }

    /// Run a blocking database operation on tokio's blocking thread pool.
    /// Use this for heavy DB operations in async code to avoid blocking tokio worker threads.
    ///
    /// Example:
    /// ```ignore
    /// let result = Database::run_blocking(db.clone(), |conn| {
    ///     conn.execute("INSERT INTO ...", params![...])?;
    ///     Ok(())
    /// }).await?;
    /// ```
    pub async fn run_blocking<F, R>(db: Arc<Database>, f: F) -> R
    where
        F: FnOnce(&Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        tokio::task::spawn_blocking(move || {
            let conn = db.conn();
            f(&conn)
        })
        .await
        .expect("Database spawn_blocking task panicked")
    }

    /// Initialize schema (idempotent)
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn();

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

        conn.execute_batch(schema::SCHEMA)?;

        // Add FTS5 full-text search table if missing
        schema::migrate_code_fts(&conn)?;

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
