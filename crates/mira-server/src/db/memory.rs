// crates/mira-server/src/db/memory.rs
// Memory storage and retrieval operations

use anyhow::Result;
use mira_types::MemoryFact;
use rusqlite::params;

use super::Database;
use crate::search::embedding_to_bytes;

// Branch-aware boosting constants (tunable)
// Lower multiplier = better score (distances are minimized)

/// Boost factor for memories on the same branch (15% boost)
const SAME_BRANCH_BOOST: f32 = 0.85;

/// Boost factor for memories on main/master branch (5% boost)
const MAIN_BRANCH_BOOST: f32 = 0.95;

/// Apply branch-aware boosting to a distance score
///
/// Returns a boosted (lower) distance for:
/// - Same branch: 15% reduction (multiply by 0.85)
/// - main/master: 5% reduction (multiply by 0.95)
/// - NULL branch (legacy data): no change
/// - Different branch: no change (keeps cross-branch knowledge accessible)
pub fn apply_branch_boost(
    distance: f32,
    memory_branch: Option<&str>,
    current_branch: Option<&str>,
) -> f32 {
    match (memory_branch, current_branch) {
        // Same branch: strongest boost
        (Some(m), Some(c)) if m == c => distance * SAME_BRANCH_BOOST,
        // main/master memories get a small boost (stable/shared knowledge)
        (Some(m), _) if m == "main" || m == "master" => distance * MAIN_BRANCH_BOOST,
        // NULL branch (legacy data) or different branch: no boost
        // Cross-branch knowledge remains accessible, just not prioritized
        _ => distance,
    }
}

/// Parse MemoryFact from a rusqlite Row with standard column order:
/// (id, project_id, key, content, fact_type, category, confidence, created_at,
///  session_count, first_session_id, last_session_id, status, user_id, scope, team_id)
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
        user_id: row.get(12).ok(),
        scope: row.get(13).unwrap_or_else(|_| "project".to_string()),
        team_id: row.get(14).ok(),
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
        // Do all DB operations in a block to release lock before promotion check
        let (result_id, needs_promotion) = {
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
                    let is_new_session =
                        session_id.map(|s| last_session.as_deref() != Some(s)).unwrap_or(false);

                    if is_new_session {
                        // Increment session count and update last_session_id
                        conn.execute(
                            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                             session_count = session_count + 1, last_session_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                            params![content, fact_type, category, confidence, session_id, id],
                        )?;
                        (id, true)
                    } else {
                        conn.execute(
                            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                            params![content, fact_type, category, confidence, id],
                        )?;
                        (id, false)
                    }
                } else {
                    // Key provided but no existing record - insert new
                    // New candidates start with capped confidence (max 0.5),
                    // except for health alerts which use their original confidence
                    let initial_confidence = if fact_type == "health" {
                        confidence
                    } else {
                        confidence.min(0.5)
                    };
                    conn.execute(
                        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
                         session_count, first_session_id, last_session_id, status)
                         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate')",
                        params![project_id, key, content, fact_type, category, initial_confidence, session_id, session_id],
                    )?;
                    (conn.last_insert_rowid(), false)
                }
            } else {
                // No key - always insert new
                // New candidates start with capped confidence (max 0.5),
                // except for health alerts which use their original confidence
                let initial_confidence = if fact_type == "health" {
                    confidence
                } else {
                    confidence.min(0.5)
                };
                conn.execute(
                    "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
                     session_count, first_session_id, last_session_id, status)
                     VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate')",
                    params![project_id, key, content, fact_type, category, initial_confidence, session_id, session_id],
                )?;
                (conn.last_insert_rowid(), false)
            }
        };

        // Check for promotion after lock is released
        if needs_promotion {
            self.maybe_promote_memory(result_id)?;
        }

        Ok(result_id)
    }

    /// Record that a memory was accessed in a session (for recall tracking)
    pub fn record_memory_access(&self, memory_id: i64, session_id: &str) -> Result<()> {
        let needs_promotion = {
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
                true
            } else {
                false
            }
        };

        // Check for promotion after lock is released
        if needs_promotion {
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
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id
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
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id
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
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id
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
                        session_count, first_session_id, last_session_id, status,
                        user_id, scope, team_id
                 FROM memory_facts
                 WHERE project_id IS NULL AND category = ?
                 ORDER BY confidence DESC, updated_at DESC
                 LIMIT ?",
                vec![Box::new(cat.to_string()), Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                        session_count, first_session_id, last_session_id, status,
                        user_id, scope, team_id
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
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id
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

// ═══════════════════════════════════════════════════════════════════════════════
// SYNC FUNCTIONS FOR POOL.INTERACT() USAGE
// These take &Connection directly for use with DatabasePool::interact()
// ═══════════════════════════════════════════════════════════════════════════════

/// Parameters for storing a memory with full scope support
pub struct StoreMemoryParams<'a> {
    pub project_id: Option<i64>,
    pub key: Option<&'a str>,
    pub content: &'a str,
    pub fact_type: &'a str,
    pub category: Option<&'a str>,
    pub confidence: f64,
    pub session_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub scope: &'a str,
    pub branch: Option<&'a str>,
}

/// Store a memory with full scope/user support (sync version for pool.interact())
/// Returns the memory ID
pub fn store_memory_sync(conn: &rusqlite::Connection, params: StoreMemoryParams) -> rusqlite::Result<i64> {
    // Upsert by key if provided
    if let Some(key) = params.key {
        let existing: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, last_session_id FROM memory_facts WHERE key = ? AND (project_id = ? OR project_id IS NULL)",
                rusqlite::params![key, params.project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((id, last_session)) = existing {
            let is_new_session = params
                .session_id
                .map(|s| last_session.as_deref() != Some(s))
                .unwrap_or(false);

            if is_new_session {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                     session_count = session_count + 1, last_session_id = ?, user_id = COALESCE(user_id, ?),
                     scope = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.session_id, params.user_id, params.scope, id
                    ],
                )?;
                // Check for promotion
                conn.execute(
                    "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
                     WHERE id = ? AND status = 'candidate' AND session_count >= 3",
                    [id],
                )?;
            } else {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                     user_id = COALESCE(user_id, ?), scope = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.user_id, params.scope, id
                    ],
                )?;
            }
            return Ok(id);
        }
    }

    // New memory - starts as candidate with capped confidence
    let initial_confidence = if params.confidence < 1.0 { params.confidence } else { 0.5 };
    conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
         session_count, first_session_id, last_session_id, status, user_id, scope, branch)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate', ?, ?, ?)",
        rusqlite::params![
            params.project_id, params.key, params.content, params.fact_type, params.category,
            initial_confidence, params.session_id, params.session_id, params.user_id, params.scope,
            params.branch
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Search for capabilities and issues by vector similarity (sync version for pool.interact())
/// Returns (fact_id, content, fact_type, distance) tuples
pub fn search_capabilities_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String, String, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.content, f.fact_type, vec_distance_cosine(v.embedding, ?1) as distance
         FROM vec_memory v
         JOIN memory_facts f ON v.fact_id = f.id
         WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
           AND f.fact_type IN ('capability', 'issue')
         ORDER BY distance
         LIMIT ?3",
    )?;

    let results = stmt
        .query_map(rusqlite::params![embedding_bytes, project_id, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Import a confirmed memory (bypasses evidence-based promotion)
/// Used for importing from CLAUDE.local.md where entries are already high-confidence
pub fn import_confirmed_memory_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    confidence: f64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
         session_count, first_session_id, last_session_id, status)
         VALUES (?, ?, ?, ?, ?, ?, 1, NULL, NULL, 'confirmed')",
        rusqlite::params![project_id, key, content, fact_type, category, confidence],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Store embedding for a memory (sync version for pool.interact())
pub fn store_embedding_sync(
    conn: &rusqlite::Connection,
    fact_id: i64,
    content: &str,
    embedding_bytes: &[u8],
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
        rusqlite::params![fact_id, embedding_bytes, fact_id, content],
    )?;
    Ok(())
}

/// Store embedding for a fact and mark as embedded (sync version for pool.interact())
pub fn store_fact_embedding_sync(
    conn: &rusqlite::Connection,
    fact_id: i64,
    content: &str,
    embedding_bytes: &[u8],
) -> rusqlite::Result<()> {
    // Insert or update embedding
    conn.execute(
        "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
        rusqlite::params![fact_id, embedding_bytes, fact_id, content],
    )?;

    // Mark fact as having embedding
    conn.execute(
        "UPDATE memory_facts SET has_embedding = 1 WHERE id = ?",
        [fact_id],
    )?;

    Ok(())
}

/// Semantic search for memories with scope filtering (sync version for pool.interact())
/// Returns (fact_id, content, distance) tuples
pub fn recall_semantic_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    user_id: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String, f32)>> {
    // Delegate to branch-aware version with no branch (no boosting)
    recall_semantic_with_branch_sync(conn, embedding_bytes, project_id, user_id, None, limit)
}

/// Branch-aware semantic recall with boosting
///
/// Returns (fact_id, content, boosted_distance, branch) tuples.
/// When current_branch is provided, memories on the same branch get boosted,
/// and main/master memories get a smaller boost.
pub fn recall_semantic_with_branch_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    user_id: Option<&str>,
    current_branch: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String, f32)>> {
    // Fetch more results than needed to allow for re-ranking after boosting
    let fetch_limit = (limit * 2).min(100);

    let mut stmt = conn.prepare(
        "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance, f.branch
         FROM vec_memory v
         JOIN memory_facts f ON v.fact_id = f.id
         WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
           AND (
             f.scope = 'project'
             OR f.scope IS NULL
             OR (f.scope = 'personal' AND f.user_id = ?4)
             OR f.user_id IS NULL
           )
         ORDER BY distance
         LIMIT ?3",
    )?;

    let results: Vec<(i64, String, f32, Option<String>)> = stmt
        .query_map(rusqlite::params![embedding_bytes, project_id, fetch_limit as i64, user_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Apply branch boosting and re-sort
    let mut boosted: Vec<(i64, String, f32)> = results
        .into_iter()
        .map(|(id, content, distance, branch)| {
            let boosted_distance = apply_branch_boost(distance, branch.as_deref(), current_branch);
            (id, content, boosted_distance)
        })
        .collect();

    // Re-sort by boosted distance (ascending - lower is better)
    boosted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Truncate to requested limit
    boosted.truncate(limit);

    Ok(boosted)
}

/// Branch-aware semantic recall that also returns the branch for display
///
/// Returns (fact_id, content, boosted_distance, branch) tuples.
pub fn recall_semantic_with_branch_info_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    user_id: Option<&str>,
    current_branch: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String, f32, Option<String>)>> {
    // Fetch more results than needed to allow for re-ranking after boosting
    let fetch_limit = (limit * 2).min(100);

    let mut stmt = conn.prepare(
        "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance, f.branch
         FROM vec_memory v
         JOIN memory_facts f ON v.fact_id = f.id
         WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
           AND (
             f.scope = 'project'
             OR f.scope IS NULL
             OR (f.scope = 'personal' AND f.user_id = ?4)
             OR f.user_id IS NULL
           )
         ORDER BY distance
         LIMIT ?3",
    )?;

    let results: Vec<(i64, String, f32, Option<String>)> = stmt
        .query_map(rusqlite::params![embedding_bytes, project_id, fetch_limit as i64, user_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Apply branch boosting and re-sort
    let mut boosted: Vec<(i64, String, f32, Option<String>)> = results
        .into_iter()
        .map(|(id, content, distance, branch)| {
            let boosted_distance = apply_branch_boost(distance, branch.as_deref(), current_branch);
            (id, content, boosted_distance, branch)
        })
        .collect();

    // Re-sort by boosted distance (ascending - lower is better)
    boosted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Truncate to requested limit
    boosted.truncate(limit);

    Ok(boosted)
}

/// Search memories by text with scope filtering (sync version for pool.interact())
pub fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    user_id: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<MemoryFact>> {
    // Escape SQL LIKE wildcards to prevent injection
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
         WHERE (project_id = ? OR project_id IS NULL)
           AND content LIKE ? ESCAPE '\\'
           AND (
             scope = 'project'
             OR scope IS NULL
             OR (scope = 'personal' AND user_id = ?)
             OR user_id IS NULL
           )
         ORDER BY updated_at DESC
         LIMIT ?",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![project_id, pattern, user_id, limit as i64],
        parse_memory_fact_row,
    )?;

    rows.collect()
}

/// Record that a memory was accessed in a session (sync version for pool.interact())
/// Handles session count increment and automatic promotion
pub fn record_memory_access_sync(
    conn: &rusqlite::Connection,
    memory_id: i64,
    session_id: &str,
) -> rusqlite::Result<()> {
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
            rusqlite::params![session_id, memory_id],
        )?;

        // Check for promotion
        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
             WHERE id = ? AND status = 'candidate' AND session_count >= 3",
            [memory_id],
        )?;
    }

    Ok(())
}

/// Delete a memory and its embedding (sync version for pool.interact())
pub fn delete_memory_sync(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<bool> {
    // Delete from vector table first
    conn.execute("DELETE FROM vec_memory WHERE fact_id = ?", rusqlite::params![id])?;
    // Delete from facts table
    let deleted = conn.execute("DELETE FROM memory_facts WHERE id = ?", rusqlite::params![id])? > 0;
    Ok(deleted)
}

#[cfg(test)]
mod branch_boost_tests {
    use super::*;

    #[test]
    fn test_same_branch_boost() {
        // Same branch should get 15% boost (multiply by 0.85)
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("feature-x"), Some("feature-x"));
        assert!((boosted - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_main_branch_boost() {
        // main branch should get 5% boost (multiply by 0.95)
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("main"), Some("feature-x"));
        assert!((boosted - 0.95).abs() < 0.001);

        // master branch should also get 5% boost
        let boosted_master = apply_branch_boost(distance, Some("master"), Some("feature-x"));
        assert!((boosted_master - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_different_branch_no_boost() {
        // Different branch should get no boost
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("feature-y"), Some("feature-x"));
        assert!((boosted - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_null_branch_no_boost() {
        // NULL branch (legacy data) should get no boost
        let distance = 1.0;

        // Memory has no branch
        let boosted1 = apply_branch_boost(distance, None, Some("feature-x"));
        assert!((boosted1 - 1.0).abs() < 0.001);

        // Current context has no branch
        let boosted2 = apply_branch_boost(distance, Some("feature-x"), None);
        assert!((boosted2 - 1.0).abs() < 0.001);

        // Both have no branch
        let boosted3 = apply_branch_boost(distance, None, None);
        assert!((boosted3 - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_boost_preserves_ordering() {
        // Boosting should improve relative ranking of same-branch memories
        let base_distance = 0.5;

        let same_branch = apply_branch_boost(base_distance, Some("feature-x"), Some("feature-x"));
        let different_branch = apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

        // Same branch should have lower (better) distance
        assert!(same_branch < different_branch);
    }

    #[test]
    fn test_main_branch_beats_different_branch() {
        // main/master branch should rank better than different branch
        let base_distance = 0.5;

        let main_branch = apply_branch_boost(base_distance, Some("main"), Some("feature-x"));
        let different_branch = apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

        // main should have lower (better) distance
        assert!(main_branch < different_branch);
    }

    #[test]
    fn test_same_branch_beats_main() {
        // Same branch should rank better than main
        let base_distance = 0.5;

        let same_branch = apply_branch_boost(base_distance, Some("feature-x"), Some("feature-x"));
        let main_branch = apply_branch_boost(base_distance, Some("main"), Some("feature-x"));

        // Same branch should have lower (better) distance
        assert!(same_branch < main_branch);
    }
}
