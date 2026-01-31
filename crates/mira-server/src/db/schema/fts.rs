// crates/mira-server/src/db/schema/fts.rs
// Full-text search (FTS5) migrations

use anyhow::Result;
use rusqlite::Connection;

/// Rebuild the FTS5 index from code_chunks
/// Call this after indexing or when FTS index needs refreshing
pub fn rebuild_code_fts(conn: &Connection) -> Result<()> {
    tracing::info!("Rebuilding FTS5 code search index");

    // Clear existing FTS data
    conn.execute("DELETE FROM code_fts", [])?;

    // Populate from code_chunks (canonical chunk store)
    // Use code_chunks.id as rowid so FTS rowid matches code_chunks.id for joins
    let inserted = conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT id, file_path, chunk_content, project_id, start_line FROM code_chunks",
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

    // Re-insert from code_chunks (canonical chunk store)
    // Use code_chunks.id as rowid so FTS rowid matches code_chunks.id for joins
    conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT id, file_path, chunk_content, project_id, start_line
         FROM code_chunks WHERE project_id = ?",
        [project_id],
    )?;

    Ok(())
}
