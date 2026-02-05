// crates/mira-server/src/db/schema/code.rs
// Schema and migrations for the code index database (mira-code.db)
//
// This database holds all code intelligence tables, separated from the main
// database to reduce write contention during indexing operations.

use crate::db::migration_helpers::{add_column_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// SQL to create the vec_code virtual table with optimized chunk_size.
///
/// chunk_size=256 reduces per-chunk waste from 6 MB (default 1024) to 1.5 MB.
/// sqlite-vec uses brute-force scan for KNN, so chunk_size doesn't meaningfully
/// affect query performance at our scale (~5K vectors).
/// TODO: Benchmark if vector count grows significantly past 50K.
pub const VEC_CODE_CREATE_SQL: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
    embedding float[1536],
    +file_path TEXT,
    +chunk_content TEXT,
    +project_id INTEGER,
    +start_line INTEGER,
    chunk_size=256
)";

/// Code index database schema SQL
///
/// These tables were originally in the main database but are now isolated
/// in their own database file for better write concurrency.
pub const CODE_SCHEMA: &str = r#"
-- =======================================
-- CODE INTELLIGENCE
-- =======================================
CREATE TABLE IF NOT EXISTS code_symbols (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    start_line INTEGER,
    end_line INTEGER,
    signature TEXT,
    indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON code_symbols(project_id, file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);

CREATE TABLE IF NOT EXISTS call_graph (
    id INTEGER PRIMARY KEY,
    caller_id INTEGER REFERENCES code_symbols(id),
    callee_name TEXT NOT NULL,
    callee_id INTEGER REFERENCES code_symbols(id),
    call_count INTEGER DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_calls_caller ON call_graph(caller_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee ON call_graph(callee_id);

CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    import_path TEXT NOT NULL,
    is_external INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(project_id, file_path);

CREATE TABLE IF NOT EXISTS codebase_modules (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    module_id TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    purpose TEXT,
    exports TEXT,
    depends_on TEXT,
    symbol_count INTEGER DEFAULT 0,
    line_count INTEGER DEFAULT 0,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, module_id)
);
CREATE INDEX IF NOT EXISTS idx_modules_project ON codebase_modules(project_id);

-- =======================================
-- BACKGROUND PROCESSING
-- =======================================
CREATE TABLE IF NOT EXISTS pending_embeddings (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    file_path TEXT NOT NULL,
    chunk_content TEXT NOT NULL,
    start_line INTEGER NOT NULL DEFAULT 1,
    status TEXT DEFAULT 'pending',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_pending_embeddings_status ON pending_embeddings(status);

-- =======================================
-- CODE CHUNKS (canonical chunk store)
-- =======================================
CREATE TABLE IF NOT EXISTS code_chunks (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    file_path TEXT NOT NULL,
    chunk_content TEXT NOT NULL,
    start_line INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_code_chunks_project ON code_chunks(project_id);
CREATE INDEX IF NOT EXISTS idx_code_chunks_file ON code_chunks(project_id, file_path);

-- =======================================
-- VECTOR TABLES (sqlite-vec)
-- =======================================
-- vec_code is created separately via VEC_CODE_CREATE_SQL constant
-- to maintain a single source of truth for its DDL.

-- =======================================
-- FULL-TEXT SEARCH (FTS5)
-- =======================================
-- tokenize: unicode61 without porter stemmer, keeping '_' as a token character
-- so snake_case identifiers like database_pool are indexed as single tokens.
CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
    file_path,
    chunk_content,
    project_id UNINDEXED,
    start_line UNINDEXED,
    content='',
    tokenize="unicode61 remove_diacritics 1 tokenchars '_'"
);
"#;

/// Run all code database schema setup and migrations.
///
/// Called during code database initialization. Idempotent.
pub fn run_code_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(CODE_SCHEMA)?;

    // Create vec_code using the single source of truth constant
    conn.execute(VEC_CODE_CREATE_SQL, [])?;

    // Run code-specific migrations
    migrate_vec_code_line_numbers(conn)?;
    migrate_vec_code_chunk_size(conn)?;
    migrate_pending_embeddings_line_numbers(conn)?;
    migrate_imports_unique(conn)?;
    migrate_fts_tokenizer(conn)?;
    migrate_code_chunks(conn)?;
    migrate_module_dependencies(conn)?;
    migrate_detected_patterns(conn)?;
    migrate_conventions_extracted_at(conn)?;

    Ok(())
}

/// Add conventions_extracted_at column to codebase_modules for incremental convention extraction
fn migrate_conventions_extracted_at(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "codebase_modules", "conventions_extracted_at", "TEXT")
}

/// Add module_dependencies table for cross-module dependency analysis
fn migrate_module_dependencies(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::create_table_if_missing;
    create_table_if_missing(
        conn,
        "module_dependencies",
        r#"
        CREATE TABLE IF NOT EXISTS module_dependencies (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL,
            source_module_id TEXT NOT NULL,
            target_module_id TEXT NOT NULL,
            dependency_type TEXT NOT NULL,
            call_count INTEGER DEFAULT 0,
            import_count INTEGER DEFAULT 0,
            is_circular INTEGER DEFAULT 0,
            computed_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, source_module_id, target_module_id)
        );
        CREATE INDEX IF NOT EXISTS idx_module_deps_project ON module_dependencies(project_id);
        CREATE INDEX IF NOT EXISTS idx_module_deps_circular ON module_dependencies(project_id, is_circular) WHERE is_circular = 1;
    "#,
    )
}

/// Add detected_patterns column to codebase_modules for architectural pattern detection
fn migrate_detected_patterns(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::add_column_if_missing;
    add_column_if_missing(conn, "codebase_modules", "detected_patterns", "TEXT")
}

/// Migrate vec_code to add start_line column (v2.1 schema)
fn migrate_vec_code_line_numbers(conn: &Connection) -> Result<()> {
    let vec_code_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !vec_code_exists {
        return Ok(());
    }

    let has_start_line: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("start_line"))
            },
        )
        .unwrap_or(false);

    if !has_start_line {
        tracing::info!("Migrating vec_code to add start_line column");
        conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
        conn.execute(VEC_CODE_CREATE_SQL, [])?;
    }

    Ok(())
}

/// Migrate vec_code to use chunk_size=256 for reduced storage bloat.
///
/// sqlite-vec's default chunk_size=1024 pre-allocates 6 MB per chunk. With
/// deletions (re-indexing, file watcher updates), empty chunks accumulate
/// and are never freed. This migration compacts vec_code by extracting all
/// rows, dropping the table, and recreating with chunk_size=256.
///
/// Idempotent: after compact, the recreated table SQL contains "chunk_size"
/// so subsequent runs are no-ops.
fn migrate_vec_code_chunk_size(conn: &Connection) -> Result<()> {
    let vec_code_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !vec_code_exists {
        return Ok(());
    }

    // Check if vec_code already has chunk_size in its schema
    let has_chunk_size: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("chunk_size"))
            },
        )
        .unwrap_or(false);

    if has_chunk_size {
        return Ok(());
    }

    tracing::info!("Migrating vec_code to chunk_size=256 (compacting storage bloat)");

    // Use compact_vec_code_sync from db::index for the heavy lifting
    match crate::db::index::compact_vec_code_sync(conn) {
        Ok(stats) => {
            tracing::info!(
                "vec_code compacted: {} rows preserved, ~{:.1} MB estimated savings",
                stats.rows_preserved,
                stats.estimated_savings_mb
            );
        }
        Err(e) => {
            // Fallback: DROP and recreate empty — embeddings regenerate on next index
            tracing::warn!(
                "vec_code compact failed ({}), dropping and recreating empty — \
                 embeddings will regenerate on next index operation",
                e
            );
            conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
            conn.execute(VEC_CODE_CREATE_SQL, [])?;
        }
    }

    Ok(())
}

/// Migrate pending_embeddings to add start_line column
fn migrate_pending_embeddings_line_numbers(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "pending_embeddings") {
        return Ok(());
    }
    add_column_if_missing(
        conn,
        "pending_embeddings",
        "start_line",
        "INTEGER NOT NULL DEFAULT 1",
    )
}

/// Migrate FTS5 tokenizer from porter-stemmed to code-aware.
///
/// Detects if code_fts uses the old `porter unicode61` tokenizer and rebuilds
/// with `unicode61 tokenchars '_'` which preserves snake_case as single tokens
/// and avoids incorrect stemming of code identifiers.
fn migrate_fts_tokenizer(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "code_fts") {
        return Ok(());
    }

    let uses_old_tokenizer: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='code_fts'",
            [],
            |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("porter"))
            },
        )
        .unwrap_or(false);

    if !uses_old_tokenizer {
        return Ok(());
    }

    tracing::info!("Migrating code_fts: replacing porter tokenizer with code-aware tokenizer");

    conn.execute("DROP TABLE IF EXISTS code_fts", [])?;
    conn.execute_batch(
        r#"CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
            file_path,
            chunk_content,
            project_id UNINDEXED,
            start_line UNINDEXED,
            content='',
            tokenize="unicode61 remove_diacritics 1 tokenchars '_'"
        );"#,
    )?;

    // Re-populate from vec_code if it has data
    let vec_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_code", [], |row| row.get(0))
        .unwrap_or(0);

    if vec_count > 0 {
        let inserted = conn.execute(
            "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
             SELECT rowid, file_path, chunk_content, project_id, start_line FROM vec_code",
            [],
        )?;
        tracing::info!("FTS5 tokenizer migration: re-indexed {} chunks", inserted);
    }

    Ok(())
}

/// Migrate code_chunks table for existing users.
///
/// If code_chunks is empty but vec_code has data, populate code_chunks from
/// vec_code so FTS and LIKE search work immediately without re-indexing.
/// Then rebuild code_fts from code_chunks.
fn migrate_code_chunks(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "code_chunks") {
        return Ok(());
    }

    let chunks_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_chunks", [], |row| row.get(0))
        .unwrap_or(0);

    if chunks_count > 0 {
        return Ok(());
    }

    // Check if vec_code has data to migrate from
    let vec_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_code", [], |row| row.get(0))
        .unwrap_or(0);

    if vec_count == 0 {
        return Ok(());
    }

    tracing::info!(
        "Migrating {} chunks from vec_code to code_chunks",
        vec_count
    );

    conn.execute(
        "INSERT INTO code_chunks (project_id, file_path, chunk_content, start_line)
         SELECT project_id, file_path, chunk_content, start_line FROM vec_code",
        [],
    )?;

    // Rebuild FTS from code_chunks
    if table_exists(conn, "code_fts") {
        conn.execute("DELETE FROM code_fts", [])?;
        conn.execute(
            "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
             SELECT id, file_path, chunk_content, project_id, start_line FROM code_chunks",
            [],
        )?;
        tracing::info!("Rebuilt code_fts from code_chunks after migration");
    }

    Ok(())
}

/// Deduplicate imports and add unique constraint
fn migrate_imports_unique(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "imports") {
        return Ok(());
    }

    let index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='index' AND name='uniq_imports'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if index_exists {
        return Ok(());
    }

    tracing::info!("Deduplicating imports and adding unique constraint");

    conn.execute_batch(
        "DELETE FROM imports
         WHERE id NOT IN (
             SELECT MIN(id) FROM imports
             GROUP BY project_id, file_path, import_path
         );
         CREATE UNIQUE INDEX IF NOT EXISTS uniq_imports
         ON imports(project_id, file_path, import_path);",
    )?;

    Ok(())
}
