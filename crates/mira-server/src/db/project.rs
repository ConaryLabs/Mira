// db/project.rs
// Project management operations

use anyhow::Result;
use mira_types::MemoryFact;
use rusqlite::{params, Connection, OptionalExtension};

use super::{parse_memory_fact_row, Database};

// ═══════════════════════════════════════════════════════════════════════════════
// Sync functions for pool.interact() usage
// ═══════════════════════════════════════════════════════════════════════════════

/// Get or create a project, returning (id, name) - sync version for pool.interact()
pub fn get_or_create_project_sync(
    conn: &Connection,
    path: &str,
    name: Option<&str>,
) -> rusqlite::Result<(i64, Option<String>)> {
    conn.query_row(
        "INSERT INTO projects (path, name) VALUES (?, ?)
         ON CONFLICT(path) DO UPDATE SET
             name = COALESCE(projects.name, excluded.name),
             created_at = projects.created_at
         RETURNING id, name",
        params![path, name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// Update project name - sync version
pub fn update_project_name_sync(
    conn: &Connection,
    project_id: i64,
    name: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE projects SET name = ? WHERE id = ?",
        params![name, project_id],
    )?;
    Ok(())
}

/// Get project info by ID (name, path) - sync version for pool.interact()
pub fn get_project_info_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Option<(Option<String>, String)>> {
    let result = conn.query_row(
        "SELECT name, path FROM projects WHERE id = ?",
        [project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok(info) => Ok(Some(info)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Create or update a session - sync version for pool.interact()
pub fn upsert_session_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, project_id, status, started_at, last_activity)
         VALUES (?1, ?2, 'active', datetime('now'), datetime('now'))
         ON CONFLICT(id) DO UPDATE SET last_activity = datetime('now')",
        params![session_id, project_id],
    )?;
    Ok(())
}

/// Get indexed projects (projects with codebase_modules) - sync version
pub fn get_indexed_projects_sync(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id, p.path
         FROM projects p
         JOIN codebase_modules m ON m.project_id = p.id",
    )?;

    let projects = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(projects)
}

/// Search memories by text pattern - sync version for pool.interact()
pub fn search_memories_text_sync(
    conn: &Connection,
    project_id: Option<i64>,
    query: &str,
    limit: usize,
) -> rusqlite::Result<Vec<MemoryFact>> {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{}%", escaped);

    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id
         FROM memory_facts
         WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ? ESCAPE '\\'
         ORDER BY updated_at DESC
         LIMIT ?",
    )?;

    let rows = stmt
        .query_map(params![project_id, pattern, limit as i64], |row| {
            parse_memory_fact_row(row)
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

/// Get preferences for a project - sync version for pool.interact()
pub fn get_preferences_sync(
    conn: &Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<Vec<MemoryFact>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id
         FROM memory_facts
         WHERE (project_id = ? OR project_id IS NULL) AND fact_type = 'preference'
         ORDER BY category, created_at DESC",
    )?;

    let rows = stmt
        .query_map(params![project_id], |row| parse_memory_fact_row(row))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

/// Get health alerts for a project - sync version for pool.interact()
pub fn get_health_alerts_sync(
    conn: &Connection,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<MemoryFact>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id
         FROM memory_facts
         WHERE (project_id = ? OR project_id IS NULL)
           AND fact_type = 'health'
           AND confidence >= 0.7
         ORDER BY confidence DESC, updated_at DESC
         LIMIT ?",
    )?;

    let rows = stmt
        .query_map(params![project_id, limit as i64], |row| {
            parse_memory_fact_row(row)
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

/// Get projects that need briefing checks - sync version for pool.interact()
pub fn get_projects_for_briefing_check_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id, p.path, pb.last_known_commit
         FROM projects p
         LEFT JOIN project_briefings pb ON p.id = pb.project_id
         WHERE p.path IS NOT NULL
         ORDER BY p.id"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Update project briefing with new git state and summary - sync version for pool.interact()
pub fn update_project_briefing_sync(
    conn: &Connection,
    project_id: i64,
    last_known_commit: &str,
    briefing_text: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO project_briefings (project_id, last_known_commit, briefing_text, generated_at)
         VALUES (?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            last_known_commit = excluded.last_known_commit,
            briefing_text = excluded.briefing_text,
            generated_at = CURRENT_TIMESTAMP",
        params![project_id, last_known_commit, briefing_text],
    )?;
    Ok(())
}

/// Set a server state value (upsert) - sync version for pool.interact()
pub fn set_server_state_sync(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO server_state (key, value, updated_at)
         VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

/// Get a server state value by key - sync version for pool.interact()
pub fn get_server_state_sync(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    match conn.query_row(
        "SELECT value FROM server_state WHERE key = ?",
        [key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Get project briefing (What's New since last session) - sync version
pub fn get_project_briefing_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Option<super::types::ProjectBriefing>> {
    let result = conn.query_row(
        "SELECT project_id, last_known_commit, last_session_at, briefing_text, generated_at
         FROM project_briefings WHERE project_id = ?",
        [project_id],
        |row| Ok(super::types::ProjectBriefing {
            project_id: row.get(0)?,
            last_known_commit: row.get(1)?,
            last_session_at: row.get(2)?,
            briefing_text: row.get(3)?,
            generated_at: row.get(4)?,
        }),
    );

    match result {
        Ok(briefing) => Ok(Some(briefing)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Mark that a session occurred for this project (clears the briefing) - sync version
pub fn mark_session_for_briefing_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO project_briefings (project_id, last_session_at)
         VALUES (?, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            last_session_at = CURRENT_TIMESTAMP,
            briefing_text = NULL",
        [project_id],
    )?;
    Ok(())
}

/// Save active project path for restart recovery - sync version
pub fn save_active_project_sync(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO server_state (key, value) VALUES ('active_project', ?)",
        [path],
    )?;
    Ok(())
}

/// Get last active project path for restart recovery - sync version
pub fn get_last_active_project_sync(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM server_state WHERE key = 'active_project'",
        [],
        |row| row.get(0),
    )
    .optional()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Database impl methods
// ═══════════════════════════════════════════════════════════════════════════════

impl Database {
    /// Get or create project by path, returns (id, name)
    ///
    /// Uses UPSERT pattern (INSERT ... ON CONFLICT) to be safe under concurrent access.
    /// If a name is stored, returns it. Otherwise, auto-detects from project files.
    pub fn get_or_create_project(&self, path: &str, name: Option<&str>) -> Result<(i64, Option<String>)> {
        let conn = self.conn();

        // UPSERT: insert or get existing.
        // COALESCE(projects.name, excluded.name) keeps existing name if present,
        // otherwise uses the provided name.
        let (id, stored_name): (i64, Option<String>) = conn.query_row(
            "INSERT INTO projects (path, name) VALUES (?, ?)
             ON CONFLICT(path) DO UPDATE SET
                 name = COALESCE(projects.name, excluded.name),
                 created_at = projects.created_at
             RETURNING id, name",
            params![path, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // If we have a name (either stored or just provided), return it
        if stored_name.is_some() {
            return Ok((id, stored_name));
        }

        // Auto-detect name from project files (Cargo.toml, package.json, etc.)
        let detected_name = Self::detect_project_name(path);

        if detected_name.is_some() {
            // Update with detected name (idempotent, safe to race)
            conn.execute(
                "UPDATE projects SET name = ? WHERE id = ?",
                params![&detected_name, id],
            )?;
        }

        Ok((id, detected_name))
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

    /// Get project info by ID (name, path)
    pub fn get_project_info(&self, project_id: i64) -> Result<Option<(Option<String>, String)>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT name, path FROM projects WHERE id = ?",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get database file path
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// List all projects in the database
    pub fn list_projects(&self) -> Result<Vec<(i64, String, Option<String>)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, path, name FROM projects ORDER BY id DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Get project briefing (What's New since last session)
    pub fn get_project_briefing(&self, project_id: i64) -> Result<Option<super::types::ProjectBriefing>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT project_id, last_known_commit, last_session_at, briefing_text, generated_at
             FROM project_briefings WHERE project_id = ?",
            [project_id],
            |row| Ok(super::types::ProjectBriefing {
                project_id: row.get(0)?,
                last_known_commit: row.get(1)?,
                last_session_at: row.get(2)?,
                briefing_text: row.get(3)?,
                generated_at: row.get(4)?,
            }),
        );

        match result {
            Ok(briefing) => Ok(Some(briefing)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update project briefing with new git state and summary
    pub fn update_project_briefing(
        &self,
        project_id: i64,
        last_known_commit: &str,
        briefing_text: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO project_briefings (project_id, last_known_commit, briefing_text, generated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(project_id) DO UPDATE SET
                last_known_commit = excluded.last_known_commit,
                briefing_text = excluded.briefing_text,
                generated_at = CURRENT_TIMESTAMP",
            params![project_id, last_known_commit, briefing_text],
        )?;
        Ok(())
    }

    /// Mark that a session occurred for this project (clears the briefing)
    pub fn mark_session_for_briefing(&self, project_id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO project_briefings (project_id, last_session_at)
             VALUES (?, CURRENT_TIMESTAMP)
             ON CONFLICT(project_id) DO UPDATE SET
                last_session_at = CURRENT_TIMESTAMP,
                briefing_text = NULL",
            [project_id],
        )?;
        Ok(())
    }

    /// Get projects that need briefing checks (have had sessions)
    pub fn get_projects_for_briefing_check(&self) -> Result<Vec<(i64, String, Option<String>)>> {
        let conn = self.conn();
        // Get projects that have had at least one session and have a path
        let mut stmt = conn.prepare(
            "SELECT DISTINCT p.id, p.path, pb.last_known_commit
             FROM projects p
             LEFT JOIN project_briefings pb ON p.id = pb.project_id
             WHERE p.path IS NOT NULL
             ORDER BY p.id"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    // ═══════════════════════════════════════
    // SERVER STATE (for restart recovery)
    // ═══════════════════════════════════════

    /// Get a server state value by key
    pub fn get_server_state(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let result: Result<String, _> = conn.query_row(
            "SELECT value FROM server_state WHERE key = ?",
            [key],
            |row| row.get(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set a server state value (upsert)
    pub fn set_server_state(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO server_state (key, value, updated_at)
             VALUES (?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )?;
        Ok(())
    }

    /// Delete a server state value
    pub fn delete_server_state(&self, key: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM server_state WHERE key = ?", [key])?;
        Ok(deleted > 0)
    }

    /// Get last active project path (for startup recovery)
    pub fn get_last_active_project(&self) -> Result<Option<String>> {
        self.get_server_state("active_project_path")
    }

    /// Save active project path (for restart recovery)
    pub fn save_active_project(&self, path: &str) -> Result<()> {
        self.set_server_state("active_project_path", path)
    }

    /// Clear active project (when switching or closing)
    pub fn clear_active_project(&self) -> Result<()> {
        self.delete_server_state("active_project_path")?;
        Ok(())
    }
}
