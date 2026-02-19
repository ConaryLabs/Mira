// db/memory/store.rs
// Memory storage operations: create, import, embed

use rusqlite::OptionalExtension;

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
    pub team_id: Option<i64>,
    pub suspicious: bool,
}

/// Store a memory with full scope/user support (sync version for pool.interact())
/// Returns the memory ID
pub fn store_memory_sync(
    conn: &rusqlite::Connection,
    params: StoreMemoryParams,
) -> rusqlite::Result<i64> {
    // Upsert by key if provided — includes scope principal to prevent cross-scope overwrites
    if let Some(key) = params.key {
        let existing: Option<(i64, Option<String>)> = match conn.query_row(
            "SELECT id, last_session_id FROM memory_facts
                 WHERE key = ?1 AND project_id IS ?2
                   AND COALESCE(scope, 'project') = ?3
                   AND COALESCE(team_id, 0) = COALESCE(?4, 0)
                   AND (?3 != 'personal' OR COALESCE(user_id, '') = COALESCE(?5, ''))",
            rusqlite::params![
                key,
                params.project_id,
                params.scope,
                params.team_id,
                params.user_id
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                tracing::warn!("Failed to update memory embedding: {e}");
                None
            }
        };

        if let Some((id, last_session)) = existing {
            let is_new_session = params
                .session_id
                .map(|s| last_session.as_deref() != Some(s))
                .unwrap_or(false);

            if is_new_session {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                     session_count = session_count + 1, last_session_id = ?, user_id = COALESCE(user_id, ?),
                     scope = ?, branch = ?, team_id = ?, suspicious = ?,
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.session_id, params.user_id, params.scope, params.branch, params.team_id,
                        params.suspicious as i32, id
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
                     user_id = COALESCE(user_id, ?), scope = ?, branch = ?, team_id = ?, suspicious = ?,
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.user_id, params.scope, params.branch, params.team_id,
                        params.suspicious as i32, id
                    ],
                )?;
            }
            return Ok(id);
        }
    }

    // New memory - starts as candidate with capped confidence
    let initial_confidence = if params.confidence < 1.0 {
        params.confidence
    } else {
        0.5
    };
    conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
         session_count, first_session_id, last_session_id, status, user_id, scope, branch, team_id, suspicious)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate', ?, ?, ?, ?, ?)",
        rusqlite::params![
            params.project_id,
            params.key,
            params.content,
            params.fact_type,
            params.category,
            initial_confidence,
            params.session_id,
            params.session_id,
            params.user_id,
            params.scope,
            params.branch,
            params.team_id,
            params.suspicious as i32
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Import a confirmed memory (bypasses evidence-based promotion)
/// Used for importing from CLAUDE.local.md where entries are already high-confidence.
/// On re-import, updates existing memories matched by (key, project_id) instead of duplicating.
pub fn import_confirmed_memory_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    confidence: f64,
) -> rusqlite::Result<i64> {
    // Check for existing memory with same key and project_id
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM memory_facts WHERE key = ?1 AND project_id IS ?2",
            rusqlite::params![key, project_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    if let Some(id) = existing {
        conn.execute(
            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
             status = 'confirmed', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            rusqlite::params![content, fact_type, category, confidence, id],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
             session_count, first_session_id, last_session_id, status)
             VALUES (?, ?, ?, ?, ?, ?, 1, NULL, NULL, 'confirmed')",
            rusqlite::params![project_id, key, content, fact_type, category, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }
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
