// db/memory.rs
// Memory storage and retrieval operations

use anyhow::Result;
use mira_types::MemoryFact;
use rusqlite::params;

use super::Database;

/// Parse MemoryFact from a rusqlite Row with standard column order:
/// (id, project_id, key, content, fact_type, category, confidence, created_at)
pub fn parse_memory_fact_row(row: &rusqlite::Row) -> rusqlite::Result<MemoryFact> {
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
}

impl Database {
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

        let rows = stmt.query_map(params![project_id, pattern, limit as i64], parse_memory_fact_row)?;

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

        let rows = stmt.query_map(params![project_id], parse_memory_fact_row)?;

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
        let rows = stmt.query_map(rusqlite::params_from_iter(params), parse_memory_fact_row)?;

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
}
