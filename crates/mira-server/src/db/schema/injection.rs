// crates/mira-server/src/db/schema/injection.rs
// Schema migration for context injection tracking

use crate::db::migration_helpers::create_table_if_missing;
use anyhow::Result;
use rusqlite::Connection;

/// Create context_injections table for tracking what Mira injects into hooks.
pub fn migrate_context_injections_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "context_injections",
        r#"
        CREATE TABLE IF NOT EXISTS context_injections (
            id INTEGER PRIMARY KEY,
            hook_name TEXT NOT NULL,
            session_id TEXT,
            project_id INTEGER REFERENCES projects(id),
            chars_injected INTEGER NOT NULL DEFAULT 0,
            sources_kept TEXT,
            sources_dropped TEXT,
            latency_ms INTEGER,
            was_deduped INTEGER NOT NULL DEFAULT 0,
            was_cached INTEGER NOT NULL DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_session ON context_injections(session_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_hook ON context_injections(hook_name, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_project ON context_injections(project_id, created_at DESC);
    "#,
    )
}
