// crates/mira-server/src/db/memory.rs
// Memory storage and retrieval operations

use anyhow::Result;
use mira_types::MemoryFact;
use rusqlite::params;

use super::Database;
use crate::search::embedding_to_bytes;

/// Parse MemoryFact from a rusqlite Row with standard column order:
/// (id, project_id, key, content, fact_type, category, confidence, created_at,
///  session_count, first_session_id, last_session_id, status)
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
        session_count: row.get(8).unwrap_or(1),
        first_session_id: row.get(9).ok(),
        last_session_id: row.get(10).ok(),
        status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
    })
}

impl Database {
    /// Store a memory fact with session tracking
    pub fn store_memory(
        &self,
        project_id: Option<i64>,
        key: Option<&str>,
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        confidence: f64,
    ) -> Result<i64> {
        self.store_memory_with_session(project_id, key, content, fact_type, category, confidence, None)
    }

    /// Store a memory fact with explicit session tracking
    pub fn store_memory_with_session(
        &self,
        project_id: Option<i64>,
        key: Option<&str>,
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        confidence: f64,
        session_id: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();

        // Upsert by key if provided
        if let Some(k) = key {
            let existing: Option<(i64, Option<String>)> = conn
                .query_row(
                    "SELECT id, last_session_id FROM memory_facts WHERE key = ? AND (project_id = ? OR project_id IS NULL)",
                    params![k, project_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();

            if let Some((id, last_session)) = existing {
                // Check if this is a new session
                let is_new_session = session_id.map(|s| last_session.as_deref() != Some(s)).unwrap_or(false);

                if is_new_session {
                    // Increment session count and update last_session_id
                    conn.execute(
                        "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                         session_count = session_count + 1, last_session_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        params![content, fact_type, category, confidence, session_id, id],
                    )?;
                    // Check for promotion
                    self.maybe_promote_memory(id)?;
                } else {
                    conn.execute(
                        "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        params![content, fact_type, category, confidence, id],
                    )?;
                }
                return Ok(id);
            }
        }

        // New memory - starts as candidate with low confidence
        let initial_confidence = if confidence < 1.0 { confidence } else { 0.5 };
        conn.execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
             session_count, first_session_id, last_session_id, status)
             VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate')",
            params![project_id, key, content, fact_type, category, initial_confidence, session_id, session_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Record that a memory was accessed in a session (for recall tracking)
    pub fn record_memory_access(&self, memory_id: i64, session_id: &str) -> Result<()> {
        let conn = self.conn();

        // Get current session info
        let current: Option<String> = conn
            .query_row(
                "SELECT last_session_id FROM memory_facts WHERE id = ?",
                [memory_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        // Only increment if this is a new session
        if current.as_deref() != Some(session_id) {
            conn.execute(
                "UPDATE memory_facts SET session_count = session_count + 1, last_session_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                params![session_id, memory_id],
            )?;
            self.maybe_promote_memory(memory_id)?;
        }

        Ok(())
    }

    /// Check if memory should be promoted from candidate to confirmed
    fn maybe_promote_memory(&self, memory_id: i64) -> Result<()> {
        let conn = self.conn();

        // Promote to confirmed if session_count >= 3 and still candidate
        let updated = conn.execute(
            "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
             WHERE id = ? AND status = 'candidate' AND session_count >= 3",
            [memory_id],
        )?;

        if updated > 0 {
            tracing::info!("Memory {} promoted from candidate to confirmed", memory_id);
        }

        Ok(())
    }

    /// Get memory statistics
    pub fn get_memory_stats(&self, project_id: Option<i64>) -> Result<(i64, i64)> {
        let conn = self.conn();

        let candidates: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_facts WHERE (project_id = ? OR project_id IS NULL) AND status = 'candidate'",
            params![project_id],
            |row| row.get(0),
        )?;

        let confirmed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_facts WHERE (project_id = ? OR project_id IS NULL) AND status = 'confirmed'",
            params![project_id],
            |row| row.get(0),
        )?;

        Ok((candidates, confirmed))
    }

    /// Search memories by text (basic SQL LIKE)
    pub fn search_memories(&self, project_id: Option<i64>, query: &str, limit: usize) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();
        // Escape SQL LIKE wildcards to prevent injection
        let escaped = query
            .replace('\\', "\\\\") // Escape backslash first
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{}%", escaped);

        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ? ESCAPE '\\'
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
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
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

    /// Get health alerts (high-confidence issues found by background scanner)
    /// Returns issues with fact_type='health' sorted by confidence and recency
    pub fn get_health_alerts(&self, project_id: Option<i64>, limit: usize) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL)
               AND fact_type = 'health'
               AND confidence >= 0.7
             ORDER BY confidence DESC, updated_at DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![project_id, limit as i64], parse_memory_fact_row)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
                "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                        session_count, first_session_id, last_session_id, status
                 FROM memory_facts
                 WHERE project_id IS NULL AND category = ?
                 ORDER BY confidence DESC, updated_at DESC
                 LIMIT ?",
                vec![Box::new(cat.to_string()), Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                        session_count, first_session_id, last_session_id, status
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

        let embedding_bytes = embedding_to_bytes(embedding);

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

    // ═══════════════════════════════════════
    // EMBEDDING STATUS TRACKING
    // ═══════════════════════════════════════

    /// Mark a fact as having an embedding
    pub fn mark_fact_has_embedding(&self, fact_id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE memory_facts SET has_embedding = 1 WHERE id = ?",
            [fact_id],
        )?;
        Ok(())
    }

    /// Find facts that lack embeddings (for background processing)
    pub fn find_facts_without_embeddings(&self, limit: usize) -> Result<Vec<MemoryFact>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE has_embedding = 0
             ORDER BY created_at ASC
             LIMIT ?"
        )?;

        let rows = stmt.query_map([limit as i64], parse_memory_fact_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get count of facts lacking embeddings
    pub fn count_facts_without_embeddings(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_facts WHERE has_embedding = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Store embedding for a fact and mark as embedded
    pub fn store_fact_embedding(&self, fact_id: i64, content: &str, embedding: &[f32]) -> Result<()> {
        let conn = self.conn();

        let embedding_bytes = embedding_to_bytes(embedding);

        // Insert or update embedding
        conn.execute(
            "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
            params![fact_id, embedding_bytes, fact_id, content],
        )?;

        // Mark fact as having embedding
        conn.execute(
            "UPDATE memory_facts SET has_embedding = 1 WHERE id = ?",
            [fact_id],
        )?;

        Ok(())
    }
}
