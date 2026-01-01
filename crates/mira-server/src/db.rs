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
        conn.execute_batch(SCHEMA)?;
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
-- VECTOR TABLES (sqlite-vec)
-- ═══════════════════════════════════════
CREATE VIRTUAL TABLE IF NOT EXISTS vec_memory USING vec0(
    embedding float[3072],
    +fact_id INTEGER,
    +content TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
    embedding float[3072],
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
