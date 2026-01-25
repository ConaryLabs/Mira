// crates/mira-server/src/db/schema/fts.rs
// Full-text search (FTS5) migrations

use anyhow::Result;
use rusqlite::Connection;

/// Migrate to add FTS5 full-text search table
pub fn migrate_code_fts(conn: &Connection) -> Result<()> {
    // Check if code_fts exists
    let fts_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='code_fts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !fts_exists {
        tracing::info!("Creating FTS5 full-text search table for code");
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
                file_path,
                chunk_content,
                project_id UNINDEXED,
                start_line UNINDEXED,
                content='',
                tokenize='porter unicode61 remove_diacritics 1'
            );",
        )?;

        // Populate from existing vec_code data
        rebuild_code_fts(conn)?;
    }

    Ok(())
}

/// Rebuild the FTS5 index from vec_code
/// Call this after indexing or when FTS index needs refreshing
pub fn rebuild_code_fts(conn: &Connection) -> Result<()> {
    tracing::info!("Rebuilding FTS5 code search index");

    // Clear existing FTS data
    conn.execute("DELETE FROM code_fts", [])?;

    // Populate from vec_code
    let inserted = conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT rowid, file_path, chunk_content, project_id, start_line FROM vec_code",
        [],
    )?;

    tracing::info!("FTS5 index rebuilt with {} entries", inserted);
    Ok(())
}

/// Rebuild FTS5 index for a specific project
pub fn rebuild_code_fts_for_project(conn: &Connection, project_id: i64) -> Result<()> {
    tracing::debug!("Rebuilding FTS5 index for project {}", project_id);

    // Delete existing entries for this project
    conn.execute("DELETE FROM code_fts WHERE project_id = ?", [project_id])?;

    // Re-insert from vec_code
    conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT rowid, file_path, chunk_content, project_id, start_line
         FROM vec_code WHERE project_id = ?",
        [project_id],
    )?;

    Ok(())
}
