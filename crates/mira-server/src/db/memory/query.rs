// db/memory/query.rs
// Memory query operations: get, delete, stats, preferences, health, personas, export

use rusqlite::OptionalExtension;

use super::ranking::RankedMemory;
use super::{parse_memory_fact_row, scope_filter_sql};

/// Scope info for a memory: (project_id, scope, user_id, team_id)
pub type MemoryScopeInfo = (Option<i64>, String, Option<String>, Option<i64>);

/// Get scope/ownership info for a memory by ID (sync version for pool.interact())
pub fn get_memory_scope_sync(
    conn: &rusqlite::Connection,
    id: i64,
) -> rusqlite::Result<Option<MemoryScopeInfo>> {
    conn.query_row(
        "SELECT project_id, COALESCE(scope, 'project'), user_id, team_id FROM memory_facts WHERE id = ?",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
}

/// Delete a memory and its embedding (sync version for pool.interact())
pub fn delete_memory_sync(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<bool> {
    let tx = conn.unchecked_transaction()?;
    // Delete from vector table first
    tx.execute(
        "DELETE FROM vec_memory WHERE fact_id = ?",
        rusqlite::params![id],
    )?;
    // Delete from facts table
    let deleted = tx.execute(
        "DELETE FROM memory_facts WHERE id = ?",
        rusqlite::params![id],
    )? > 0;
    tx.commit()?;
    Ok(deleted)
}

/// Get memory statistics for a project (sync version for pool.interact())
/// Returns (candidate_count, confirmed_count)
///
/// Uses scalar subqueries instead of OR to let each COUNT hit the index directly.
pub fn get_memory_stats_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<(i64, i64)> {
    match project_id {
        Some(pid) => conn.query_row(
            "SELECT \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id = ?1 AND status = 'candidate') + \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id IS NULL AND status = 'candidate'), \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id = ?1 AND status = 'confirmed') + \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id IS NULL AND status = 'confirmed')",
            rusqlite::params![pid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ),
        None => conn.query_row(
            "SELECT \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id IS NULL AND status = 'candidate'), \
                 (SELECT COUNT(*) FROM memory_facts WHERE project_id IS NULL AND status = 'confirmed')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ),
    }
}

/// Get preferences for a project (sync version for pool.interact())
///
/// When `user_id` and `team_id` are provided, uses full scope filtering.
/// Otherwise falls back to simple project_id filtering.
pub fn get_preferences_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    user_id: Option<&str>,
    team_id: Option<i64>,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
    let sql = format!(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id, updated_at, branch
         FROM memory_facts
         WHERE {}
           AND fact_type = 'preference'
         ORDER BY category, created_at DESC",
        scope_filter_sql("")
            .replace("?{pid}", "?1")
            .replace("?{uid}", "?2")
            .replace("?{tid}", "?3")
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(
        rusqlite::params![project_id, user_id, team_id],
        parse_memory_fact_row,
    )?;
    rows.collect()
}

/// Get health alerts for a project (sync version for pool.interact())
///
/// Reads from system_observations (health findings live there now).
/// Returns MemoryFact-shaped results for backward compatibility.
pub fn get_health_alerts_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
    _user_id: Option<&str>,
    _team_id: Option<i64>,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, observation_type, category, confidence,
                COALESCE(updated_at, created_at), scope, team_id
         FROM system_observations
         WHERE project_id IS ?1
           AND observation_type = 'health'
           AND confidence >= 0.7
         ORDER BY confidence DESC, COALESCE(updated_at, created_at) DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
        Ok(mira_types::MemoryFact {
            id: row.get(0)?,
            project_id: row.get(1)?,
            key: row.get(2)?,
            content: row.get(3)?,
            fact_type: row.get(4)?,
            category: row.get(5)?,
            confidence: row.get(6)?,
            created_at: row.get(7)?,
            session_count: 1,
            first_session_id: None,
            last_session_id: None,
            status: "confirmed".to_string(),
            user_id: None,
            scope: row.get(8)?,
            team_id: row.get(9)?,
            updated_at: None,
            branch: None,
        })
    })?;
    rows.collect()
}

/// Get global memories by category (sync version for pool.interact())
pub fn get_global_memories_sync(
    conn: &rusqlite::Connection,
    category: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
    let (query, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(cat) = category {
        (
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id, updated_at, branch
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
                    user_id, scope, team_id, updated_at, branch
             FROM memory_facts
             WHERE project_id IS NULL AND fact_type = 'personal'
             ORDER BY confidence DESC, updated_at DESC
             LIMIT ?",
            vec![Box::new(limit as i64)],
        )
    };

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), parse_memory_fact_row)?;
    rows.collect()
}

/// Mark a fact as having an embedding (sync version for pool.interact())
pub fn mark_fact_has_embedding_sync(
    conn: &rusqlite::Connection,
    fact_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE memory_facts SET has_embedding = 1 WHERE id = ?",
        [fact_id],
    )?;
    Ok(())
}

/// Find facts without embeddings (sync version for pool.interact())
pub fn find_facts_without_embeddings_sync(
    conn: &rusqlite::Connection,
    limit: usize,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id, updated_at, branch
         FROM memory_facts
         WHERE has_embedding = 0
           AND fact_type IN ('general','preference','decision','pattern','context','persona')
         ORDER BY created_at ASC
         LIMIT ?",
    )?;

    let rows = stmt.query_map(rusqlite::params![limit as i64], parse_memory_fact_row)?;
    rows.collect()
}

/// Count facts without embeddings (sync version for pool.interact())
pub fn count_facts_without_embeddings_sync(conn: &rusqlite::Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM memory_facts WHERE has_embedding = 0",
        [],
        |row| row.get(0),
    )
}

/// Get base persona (sync version for pool.interact())
pub fn get_base_persona_sync(conn: &rusqlite::Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT content FROM memory_facts WHERE key = 'base_persona' AND project_id IS NULL AND fact_type = 'persona'",
        [],
        |row| row.get(0),
    ).optional()
}

/// Get project persona (sync version for pool.interact())
pub fn get_project_persona_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT content FROM memory_facts WHERE key = 'project_persona' AND project_id = ? AND fact_type = 'persona'",
        [project_id],
        |row| row.get(0),
    ).optional()
}

/// Clear project persona (sync version for pool.interact())
pub fn clear_project_persona_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> rusqlite::Result<bool> {
    // Clean up vec_memory embeddings first
    conn.execute(
        "DELETE FROM vec_memory WHERE fact_id IN (SELECT id FROM memory_facts WHERE key = 'project_persona' AND project_id = ? AND fact_type = 'persona')",
        [project_id],
    )?;
    let deleted = conn.execute(
        "DELETE FROM memory_facts WHERE key = 'project_persona' AND project_id = ? AND fact_type = 'persona'",
        [project_id],
    )? > 0;
    Ok(deleted)
}

/// Fetch memories ranked by hotness for CLAUDE.local.md export
///
/// Hotness formula (computed in SQL):
///   hotness = session_count * confidence * status_mult * category_mult / recency_penalty
///
/// - status_mult: confirmed = 1.5, candidate = 1.0
/// - category_mult: preference = 1.4, decision = 1.3, pattern/convention = 1.1, context = 1.0, general = 0.9
/// - recency_penalty: 1.0 + (days_since_update / 90.0) — gentle linear decay
/// - Confidence floor: 0.5
/// - Scope: project-only
pub fn fetch_ranked_memories_for_export_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<RankedMemory>> {
    let sql = r#"
        SELECT content, fact_type, category,
            (
                session_count
                * MAX(confidence, 0.5)
                * CASE status WHEN 'confirmed' THEN 1.5 ELSE 1.0 END
                * CASE
                    WHEN category = 'preference' THEN 1.4
                    WHEN category = 'decision' THEN 1.3
                    WHEN category IN ('pattern', 'convention') THEN 1.1
                    WHEN category = 'context' THEN 1.0
                    ELSE 0.9
                  END
                / (1.0 + (CAST(julianday('now') - julianday(COALESCE(updated_at, created_at)) AS REAL) / 90.0))
            ) AS hotness
        FROM memory_facts
        WHERE project_id = ?1
          AND scope = 'project'
          AND confidence >= 0.5
          AND fact_type NOT IN ('health', 'persona', 'system', 'session_event', 'extracted', 'tool_outcome', 'convergence_alert', 'distilled')
          AND COALESCE(suspicious, 0) = 0
        ORDER BY hotness DESC
        LIMIT ?2
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
        Ok(RankedMemory {
            content: row.get(0)?,
            fact_type: row.get(1)?,
            category: row.get(2)?,
            hotness: row.get(3)?,
        })
    })?;

    rows.collect()
}
