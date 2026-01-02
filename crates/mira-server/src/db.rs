// src/db.rs
// Unified database layer with rusqlite + sqlite-vec

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;
use std::sync::Mutex;

/// Database wrapper with sqlite-vec support
pub struct Database {
    conn: Mutex<Connection>,
    path: Option<String>,
}

impl Database {
    /// Open database at path, creating if needed
    pub fn open(path: &Path) -> Result<Self> {
        // Register sqlite-vec extension before opening
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {:?}", path))?;

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

    /// Initialize schema (idempotent)
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn();

        // Check if vec tables need migration (dimension change)
        self.migrate_vec_tables(&conn)?;

        conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// Migrate vector tables if dimensions changed
    fn migrate_vec_tables(&self, conn: &Connection) -> Result<()> {
        // Check if vec_memory exists and has wrong dimensions
        let needs_migration: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_memory_info'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if needs_migration {
            // Check current dimension by looking at chunk info
            let current_dim: Result<i64, _> = conn.query_row(
                "SELECT vector_column_size FROM vec_memory_info WHERE vector_column_name='embedding'",
                [],
                |row| row.get(0),
            );

            if let Ok(dim) = current_dim {
                if dim != 1536 {
                    tracing::info!("Migrating vector tables from {} to 1536 dimensions", dim);
                    // Drop old tables - CASCADE not supported, drop in order
                    let _ = conn.execute_batch(
                        "DROP TABLE IF EXISTS vec_memory;
                         DROP TABLE IF EXISTS vec_code;"
                    );
                }
            }
        }

        Ok(())
    }
}

/// Minimal schema for Mira
const SCHEMA: &str = r#"
-- ═══════════════════════════════════════
-- CORE: Projects
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- MEMORY: Semantic Facts
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS memory_facts (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    key TEXT,
    content TEXT NOT NULL,
    fact_type TEXT DEFAULT 'general',
    category TEXT,
    confidence REAL DEFAULT 1.0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_memory_project ON memory_facts(project_id);
CREATE INDEX IF NOT EXISTS idx_memory_key ON memory_facts(key);

CREATE TABLE IF NOT EXISTS corrections (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    what_was_wrong TEXT NOT NULL,
    what_is_right TEXT NOT NULL,
    correction_type TEXT DEFAULT 'pattern',
    scope TEXT DEFAULT 'project',
    confidence REAL DEFAULT 1.0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- CODE INTELLIGENCE
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS code_symbols (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
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
    project_id INTEGER REFERENCES projects(id),
    file_path TEXT NOT NULL,
    import_path TEXT NOT NULL,
    is_external INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(project_id, file_path);

CREATE TABLE IF NOT EXISTS codebase_modules (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
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

-- ═══════════════════════════════════════
-- SESSIONS & HISTORY
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    status TEXT DEFAULT 'active',
    summary TEXT,
    started_at TEXT DEFAULT CURRENT_TIMESTAMP,
    last_activity TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id, last_activity DESC);

CREATE TABLE IF NOT EXISTS tool_history (
    id INTEGER PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id),
    tool_name TEXT NOT NULL,
    arguments TEXT,
    result_summary TEXT,
    success INTEGER DEFAULT 1,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_history_session ON tool_history(session_id);
CREATE INDEX IF NOT EXISTS idx_history_tool ON tool_history(tool_name);

-- ═══════════════════════════════════════
-- TASKS & GOALS
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS goals (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'planning',
    priority TEXT DEFAULT 'medium',
    progress_percent INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS milestones (
    id INTEGER PRIMARY KEY,
    goal_id INTEGER REFERENCES goals(id),
    title TEXT NOT NULL,
    completed INTEGER DEFAULT 0,
    weight INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    goal_id INTEGER REFERENCES goals(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'pending',
    priority TEXT DEFAULT 'medium',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(project_id, status);

-- ═══════════════════════════════════════
-- PERMISSIONS
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS permission_rules (
    id INTEGER PRIMARY KEY,
    tool_name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    match_type TEXT DEFAULT 'prefix',
    scope TEXT DEFAULT 'global',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- BACKGROUND PROCESSING
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS pending_embeddings (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    file_path TEXT NOT NULL,
    chunk_content TEXT NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_pending_embeddings_status ON pending_embeddings(status);

CREATE TABLE IF NOT EXISTS background_batches (
    id INTEGER PRIMARY KEY,
    batch_id TEXT NOT NULL,
    item_ids TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- CHAT MESSAGES (conversation history)
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS chat_messages (
    id INTEGER PRIMARY KEY,
    role TEXT NOT NULL,  -- 'user', 'assistant'
    content TEXT NOT NULL,
    reasoning_content TEXT,  -- for deepseek reasoner responses
    summarized INTEGER DEFAULT 0,  -- 1 if included in a summary
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_created ON chat_messages(created_at DESC);

CREATE TABLE IF NOT EXISTS chat_summaries (
    id INTEGER PRIMARY KEY,
    summary TEXT NOT NULL,
    message_range_start INTEGER,  -- first message id covered
    message_range_end INTEGER,    -- last message id covered
    summary_level INTEGER DEFAULT 1,  -- 1=session, 2=daily, 3=weekly
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_chat_summaries_level ON chat_summaries(summary_level, created_at DESC);

-- ═══════════════════════════════════════
-- VECTOR TABLES (sqlite-vec)
-- ═══════════════════════════════════════
CREATE VIRTUAL TABLE IF NOT EXISTS vec_memory USING vec0(
    embedding float[1536],
    +fact_id INTEGER,
    +content TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
    embedding float[1536],
    +file_path TEXT,
    +chunk_content TEXT,
    +project_id INTEGER
);
"#;

// Helper functions for common queries

impl Database {
    /// Get or create project by path, returns (id, name)
    pub fn get_or_create_project(&self, path: &str, name: Option<&str>) -> Result<(i64, Option<String>)> {
        let conn = self.conn();

        // Try to find existing with its stored name
        let existing: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, name FROM projects WHERE path = ?",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((id, stored_name)) = existing {
            // Return stored name if we have one
            if stored_name.is_some() {
                return Ok((id, stored_name));
            }

            // No stored name - use caller's name or auto-detect
            let final_name = name.map(|s| s.to_string()).or_else(|| {
                Self::detect_project_name(path)
            });

            // Update the database with the detected name
            if final_name.is_some() {
                conn.execute(
                    "UPDATE projects SET name = ? WHERE id = ?",
                    params![&final_name, id],
                )?;
            }

            return Ok((id, final_name));
        }

        // Auto-detect name if not provided
        let detected_name = name.map(|s| s.to_string()).or_else(|| {
            Self::detect_project_name(path)
        });

        // Create new
        conn.execute(
            "INSERT INTO projects (path, name) VALUES (?, ?)",
            params![path, detected_name],
        )?;
        Ok((conn.last_insert_rowid(), detected_name))
    }

    /// Auto-detect project name from path
    fn detect_project_name(path: &str) -> Option<String> {
        use std::path::Path;

        let path = Path::new(path);
        let dir_name = || path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());

        // Try Cargo.toml for Rust projects
        let cargo_toml = path.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                // If it's a workspace, use directory name
                if content.contains("[workspace]") {
                    return dir_name();
                }

                // For single crate, find [package] section and get name
                let mut in_package = false;
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        in_package = line == "[package]";
                    } else if in_package && line.starts_with("name") {
                        if let Some(name) = line.split('=').nth(1) {
                            let name = name.trim().trim_matches('"').trim_matches('\'');
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Try package.json for Node projects
        let package_json = path.join("package.json");
        if package_json.exists() {
            if let Ok(content) = std::fs::read_to_string(&package_json) {
                // Simple JSON parsing for "name" field at top level
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with("\"name\"") {
                        if let Some(name) = line.split(':').nth(1) {
                            let name = name.trim().trim_matches(',').trim_matches('"').trim();
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Fall back to directory name
        dir_name()
    }

    /// Store a memory fact
    pub fn store_memory(
        &self,
        project_id: Option<i64>,
        key: Option<&str>,
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        confidence: f64,
    ) -> Result<i64> {
        let conn = self.conn();

        // Upsert by key if provided
        if let Some(k) = key {
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM memory_facts WHERE key = ? AND (project_id = ? OR project_id IS NULL)",
                    params![k, project_id],
                    |row| row.get(0),
                )
                .ok();

            if let Some(id) = existing {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    params![content, fact_type, category, confidence, id],
                )?;
                return Ok(id);
            }
        }

        conn.execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence) VALUES (?, ?, ?, ?, ?, ?)",
            params![project_id, key, content, fact_type, category, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Search memories by text (basic SQL LIKE)
    pub fn search_memories(&self, project_id: Option<i64>, query: &str, limit: usize) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();
        let pattern = format!("%{}%", query);

        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ?
             ORDER BY updated_at DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![project_id, pattern, limit as i64], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get all preferences for a project
    pub fn get_preferences(&self, project_id: Option<i64>) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND fact_type = 'preference'
             ORDER BY category, created_at DESC"
        )?;

        let rows = stmt.query_map(params![project_id], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Delete a memory by ID
    pub fn delete_memory(&self, id: i64) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM memory_facts WHERE id = ?", [id])?;
        Ok(deleted > 0)
    }

    // ═══════════════════════════════════════
    // GLOBAL MEMORY (for chat personal context)
    // ═══════════════════════════════════════

    /// Store a global memory (not tied to any project)
    /// Used for personal facts, user preferences, etc.
    pub fn store_global_memory(
        &self,
        content: &str,
        category: &str,
        key: Option<&str>,
        confidence: Option<f64>,
    ) -> Result<i64> {
        self.store_memory(
            None, // project_id = NULL = global
            key,
            content,
            "personal", // fact_type for global memories
            Some(category),
            confidence.unwrap_or(1.0),
        )
    }

    /// Get global memories by category
    pub fn get_global_memories(&self, category: Option<&str>, limit: usize) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();

        let (query, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(cat) = category {
            (
                "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
                 FROM memory_facts
                 WHERE project_id IS NULL AND category = ?
                 ORDER BY confidence DESC, updated_at DESC
                 LIMIT ?",
                vec![Box::new(cat.to_string()), Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
                 FROM memory_facts
                 WHERE project_id IS NULL AND fact_type = 'personal'
                 ORDER BY confidence DESC, updated_at DESC
                 LIMIT ?",
                vec![Box::new(limit as i64)],
            )
        };

        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get user profile (high-confidence core facts)
    pub fn get_user_profile(&self) -> Result<Vec<MemoryFact>> {
        self.get_global_memories(Some("profile"), 20)
    }

    /// Semantic search over global memories only
    /// Returns (fact_id, content, distance) tuples
    pub fn recall_global_semantic(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(i64, String, f32)>> {
        let conn = self.conn();

        let embedding_bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let mut stmt = conn.prepare(
            "SELECT f.id, f.content, vec_distance_cosine(v.embedding, ?1) as distance
             FROM memory_facts f
             JOIN vec_memory v ON f.id = v.fact_id
             WHERE f.project_id IS NULL
             ORDER BY distance
             LIMIT ?2"
        )?;

        let results = stmt
            .query_map(params![embedding_bytes, limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    // ═══════════════════════════════════════
    // CHAT MESSAGES
    // ═══════════════════════════════════════

    /// Store a chat message
    pub fn store_chat_message(
        &self,
        role: &str,
        content: &str,
        reasoning_content: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_messages (role, content, reasoning_content) VALUES (?, ?, ?)",
            params![role, content, reasoning_content],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent chat messages (for context window)
    pub fn get_recent_messages(&self, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, reasoning_content, created_at
             FROM chat_messages
             WHERE summarized = 0
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map([limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                reasoning_content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        // Collect and reverse to get chronological order
        let mut messages: Vec<ChatMessage> = rows.filter_map(|r| r.ok()).collect();
        messages.reverse();
        Ok(messages)
    }

    /// Get messages older than a certain ID (for summarization)
    pub fn get_messages_before(&self, before_id: i64, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, reasoning_content, created_at
             FROM chat_messages
             WHERE id < ? AND summarized = 0
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![before_id, limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                reasoning_content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        let mut messages: Vec<ChatMessage> = rows.filter_map(|r| r.ok()).collect();
        messages.reverse();
        Ok(messages)
    }

    /// Mark messages as summarized
    pub fn mark_messages_summarized(&self, start_id: i64, end_id: i64) -> Result<usize> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE chat_messages SET summarized = 1 WHERE id >= ? AND id <= ?",
            params![start_id, end_id],
        )?;
        Ok(updated)
    }

    /// Store a chat summary
    pub fn store_chat_summary(
        &self,
        summary: &str,
        range_start: i64,
        range_end: i64,
        level: i32,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_summaries (summary, message_range_start, message_range_end, summary_level)
             VALUES (?, ?, ?, ?)",
            params![summary, range_start, range_end, level],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent summaries
    pub fn get_recent_summaries(&self, level: i32, limit: usize) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, summary, message_range_start, message_range_end, summary_level, created_at
             FROM chat_summaries
             WHERE summary_level = ?
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![level, limit as i64], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                summary: row.get(1)?,
                message_range_start: row.get(2)?,
                message_range_end: row.get(3)?,
                summary_level: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        let mut summaries: Vec<ChatSummary> = rows.filter_map(|r| r.ok()).collect();
        summaries.reverse();
        Ok(summaries)
    }

    /// Get count of unsummarized messages
    pub fn count_unsummarized_messages(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE summarized = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Count summaries at a given level
    pub fn count_summaries_at_level(&self, level: i32) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chat_summaries WHERE summary_level = ?",
            [level],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get oldest summaries at a level (for promotion to next level)
    pub fn get_oldest_summaries(&self, level: i32, limit: usize) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, summary, message_range_start, message_range_end, summary_level, created_at
             FROM chat_summaries
             WHERE summary_level = ?
             ORDER BY id ASC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![level, limit as i64], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                summary: row.get(1)?,
                message_range_start: row.get(2)?,
                message_range_end: row.get(3)?,
                summary_level: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Delete summaries by IDs (after promotion)
    pub fn delete_summaries(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let placeholders: Vec<_> = ids.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM chat_summaries WHERE id IN ({})",
            placeholders.join(",")
        );

        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let deleted = conn.execute(&sql, params.as_slice())?;
        Ok(deleted)
    }

    // ═══════════════════════════════════════
    // PERSONA
    // ═══════════════════════════════════════

    /// Get the base persona (global, no project)
    pub fn get_base_persona(&self) -> Result<Option<String>> {
        let conn = self.conn();
        let result: Option<String> = conn
            .query_row(
                "SELECT content FROM memory_facts WHERE key = 'base_persona' AND project_id IS NULL AND fact_type = 'persona'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(result)
    }

    /// Set the base persona (upserts by key)
    pub fn set_base_persona(&self, content: &str) -> Result<i64> {
        self.store_memory(None, Some("base_persona"), content, "persona", None, 1.0)
    }

    /// Get project-specific persona overlay
    pub fn get_project_persona(&self, project_id: i64) -> Result<Option<String>> {
        let conn = self.conn();
        let result: Option<String> = conn
            .query_row(
                "SELECT content FROM memory_facts WHERE key = 'project_persona' AND project_id = ? AND fact_type = 'persona'",
                [project_id],
                |row| row.get(0),
            )
            .ok();
        Ok(result)
    }

    /// Set project-specific persona (upserts by key)
    pub fn set_project_persona(&self, project_id: i64, content: &str) -> Result<i64> {
        self.store_memory(Some(project_id), Some("project_persona"), content, "persona", None, 1.0)
    }

    /// Clear project-specific persona
    pub fn clear_project_persona(&self, project_id: i64) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM memory_facts WHERE key = 'project_persona' AND project_id = ? AND fact_type = 'persona'",
            [project_id],
        )?;
        Ok(deleted > 0)
    }

    // ═══════════════════════════════════════
    // SESSION & TOOL HISTORY
    // ═══════════════════════════════════════

    /// Create or update a session
    pub fn create_session(&self, session_id: &str, project_id: Option<i64>) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (id, project_id, status, started_at, last_activity)
             VALUES (?1, ?2, 'active', datetime('now'), datetime('now'))
             ON CONFLICT(id) DO UPDATE SET last_activity = datetime('now')",
            params![session_id, project_id],
        )?;
        Ok(())
    }

    /// Update session's last activity timestamp
    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET last_activity = datetime('now') WHERE id = ?",
            [session_id],
        )?;
        Ok(())
    }

    /// Log a tool call to history
    pub fn log_tool_call(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &str,
        result_summary: &str,
        success: bool,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_history (session_id, tool_name, arguments, result_summary, success, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![session_id, tool_name, arguments, result_summary, success as i32],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent tool history for a session
    pub fn get_session_history(&self, session_id: &str, limit: usize) -> Result<Vec<ToolHistoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, created_at
             FROM tool_history
             WHERE session_id = ?
             ORDER BY created_at DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(ToolHistoryEntry {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                arguments: row.get(3)?,
                result_summary: row.get(4)?,
                success: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get tool history after a specific event ID (for sync/reconnection)
    pub fn get_history_after(&self, session_id: &str, after_id: i64, limit: usize) -> Result<Vec<ToolHistoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool_name, arguments, result_summary, success, created_at
             FROM tool_history
             WHERE session_id = ? AND id > ?
             ORDER BY id ASC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![session_id, after_id, limit as i64], |row| {
            Ok(ToolHistoryEntry {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                arguments: row.get(3)?,
                result_summary: row.get(4)?,
                success: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get recent sessions for a project
    pub fn get_recent_sessions(&self, project_id: i64, limit: usize) -> Result<Vec<SessionInfo>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, status, summary, started_at, last_activity
             FROM sessions
             WHERE project_id = ?
             ORDER BY last_activity DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![project_id, limit as i64], |row| {
            Ok(SessionInfo {
                id: row.get(0)?,
                project_id: row.get(1)?,
                status: row.get(2)?,
                summary: row.get(3)?,
                started_at: row.get(4)?,
                last_activity: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get tool call count and unique tools for a session
    pub fn get_session_stats(&self, session_id: &str) -> Result<(usize, Vec<String>)> {
        let conn = self.conn();

        // Get count
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
            params![session_id],
            |row| row.get(0),
        )?;

        // Get unique tool names (top 5 most used)
        let mut stmt = conn.prepare(
            "SELECT tool_name, COUNT(*) as cnt FROM tool_history
             WHERE session_id = ?
             GROUP BY tool_name
             ORDER BY cnt DESC
             LIMIT 5",
        )?;
        let tools: Vec<String> = stmt
            .query_map(params![session_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok((count, tools))
    }

    /// Get database file path
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
}

/// Memory fact record
#[derive(Debug, Clone)]
pub struct MemoryFact {
    pub id: i64,
    pub project_id: Option<i64>,
    pub key: Option<String>,
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub created_at: String,
}

/// Tool history entry
#[derive(Debug, Clone)]
pub struct ToolHistoryEntry {
    pub id: i64,
    pub session_id: String,
    pub tool_name: String,
    pub arguments: Option<String>,
    pub result_summary: Option<String>,
    pub success: bool,
    pub created_at: String,
}

/// Session info
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub project_id: Option<i64>,
    pub status: String,
    pub summary: Option<String>,
    pub started_at: String,
    pub last_activity: String,
}

/// Chat message record
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub created_at: String,
}

/// Chat summary record
#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub id: i64,
    pub summary: String,
    pub message_range_start: i64,
    pub message_range_end: i64,
    pub summary_level: i32,
    pub created_at: String,
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
