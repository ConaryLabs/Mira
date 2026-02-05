// crates/mira-server/src/db/memory.rs
// Memory storage and retrieval operations

use mira_types::MemoryFact;
use rusqlite::OptionalExtension;
use std::sync::LazyLock;

/// (fact_id, content, distance, branch)
type RecallRow = (i64, String, f32, Option<String>);

/// Lightweight memory struct for ranked export to CLAUDE.local.md
#[derive(Debug, Clone)]
pub struct RankedMemory {
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub hotness: f64,
}

// Branch-aware boosting constants (tunable)
// Lower multiplier = better score (distances are minimized)

/// Per-entity match boost factor (10% per match, applied as 0.90^n)
const ENTITY_MATCH_BOOST: f32 = 0.90;

/// Maximum number of entity matches to apply boost for (floor = 0.90^3 = 0.729)
const MAX_ENTITY_BOOST_MATCHES: u32 = 3;

/// Boost factor for memories on the same branch (15% boost)
const SAME_BRANCH_BOOST: f32 = 0.85;

/// Boost factor for memories on main/master branch (5% boost)
const MAIN_BRANCH_BOOST: f32 = 0.95;

/// Apply entity-overlap boosting to a distance score.
///
/// Each matching entity reduces distance by 10%, up to 3 matches (floor 0.729).
/// Returns the original distance if match_count is 0.
pub fn apply_entity_boost(distance: f32, match_count: u32) -> f32 {
    if match_count == 0 {
        return distance;
    }
    let capped = match_count.min(MAX_ENTITY_BOOST_MATCHES);
    distance * ENTITY_MATCH_BOOST.powi(capped as i32)
}

/// Apply branch-aware boosting to a distance score
///
/// Returns a boosted (lower) distance for:
/// - Same branch: 15% reduction (multiply by 0.85)
/// - main/master: 5% reduction (multiply by 0.95)
/// - NULL branch (pre-branch-tracking data): no change
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
        // NULL branch (pre-branch-tracking data) or different branch: no boost
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

// ═══════════════════════════════════════════════════════════════════════════════
// SHARED SQL FRAGMENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Scope-filtering WHERE clause for memory queries.
///
/// Returns SQL fragment with parameter placeholders for (project_id, user_id).
/// `prefix` is the table alias (e.g. "f." for JOINed queries, "" for direct).
/// The caller must bind: project_id as the first param, user_id as the second.
pub fn scope_filter_sql(prefix: &str) -> String {
    format!(
        "({p}project_id = ?{{pid}} OR {p}project_id IS NULL OR ?{{pid}} IS NULL)
           AND (
             {p}scope = 'project'
             OR {p}scope IS NULL
             OR ({p}scope = 'personal' AND {p}user_id = ?{{uid}})
             OR ({p}scope = 'team' AND {p}user_id = ?{{uid}})
           )",
        p = prefix,
    )
}

/// Cached semantic recall query with scope filtering.
///
/// Returns SQL that selects (fact_id, content, distance, branch) from vec_memory + memory_facts.
/// Parameters: ?1 = embedding_bytes, ?2 = project_id, ?3 = limit, ?4 = user_id
static SEMANTIC_RECALL_SQL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance, f.branch
         FROM vec_memory v
         JOIN memory_facts f ON v.fact_id = f.id
         WHERE {}
         ORDER BY distance
         LIMIT ?3",
        scope_filter_sql("f.")
            .replace("?{pid}", "?2")
            .replace("?{uid}", "?4")
    )
});

fn semantic_recall_sql() -> &'static str {
    &SEMANTIC_RECALL_SQL
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
pub fn store_memory_sync(
    conn: &rusqlite::Connection,
    params: StoreMemoryParams,
) -> rusqlite::Result<i64> {
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
    let initial_confidence = if params.confidence < 1.0 {
        params.confidence
    } else {
        0.5
    };
    conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
         session_count, first_session_id, last_session_id, status, user_id, scope, branch)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate', ?, ?, ?)",
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
            params.branch
        ],
    )?;
    Ok(conn.last_insert_rowid())
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

/// Semantic search with entity boost applied.
///
/// Wraps the branch-info recall to also apply entity-overlap ranking boost.
/// `query_entity_names` are the canonical names extracted from the query.
/// If empty, skips entity boost entirely (no extra query).
pub fn recall_semantic_with_entity_boost_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    user_id: Option<&str>,
    current_branch: Option<&str>,
    query_entity_names: &[String],
    limit: usize,
) -> rusqlite::Result<Vec<RecallRow>> {
    use super::entities::get_entity_match_counts_sync;

    // Fetch more results than needed to allow for re-ranking after boosting
    let fetch_limit = (limit * 2).min(100);

    let sql = semantic_recall_sql();
    let mut stmt = conn.prepare(sql)?;

    let results: Vec<RecallRow> = stmt
        .query_map(
            rusqlite::params![embedding_bytes, project_id, fetch_limit as i64, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?
        .filter_map(|r| r.ok())
        .collect();

    // Get entity match counts (skip entirely if no query entities)
    let entity_counts = if !query_entity_names.is_empty() {
        get_entity_match_counts_sync(conn, project_id, query_entity_names).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    // Apply branch boost + entity boost, then re-sort
    let mut boosted: Vec<(i64, String, f32, Option<String>)> = results
        .into_iter()
        .map(|(id, content, distance, branch)| {
            let mut d = apply_branch_boost(distance, branch.as_deref(), current_branch);
            if let Some(&match_count) = entity_counts.get(&id) {
                d = apply_entity_boost(d, match_count);
            }
            (id, content, d, branch)
        })
        .collect();

    boosted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    boosted.truncate(limit);

    Ok(boosted)
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

    let sql = semantic_recall_sql();
    let mut stmt = conn.prepare(sql)?;

    let results: Vec<(i64, String, f32, Option<String>)> = stmt
        .query_map(
            rusqlite::params![embedding_bytes, project_id, fetch_limit as i64, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?
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
) -> rusqlite::Result<Vec<RecallRow>> {
    // Fetch more results than needed to allow for re-ranking after boosting
    let fetch_limit = (limit * 2).min(100);

    let sql = semantic_recall_sql();
    let mut stmt = conn.prepare(sql)?;

    let results: Vec<RecallRow> = stmt
        .query_map(
            rusqlite::params![embedding_bytes, project_id, fetch_limit as i64, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?
        .filter_map(|r| r.ok())
        .collect();

    // Apply branch boosting and re-sort
    let mut boosted: Vec<RecallRow> = results
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

/// Batch-lookup fact_type and category for a set of memory IDs.
/// Used to post-filter semantic recall results which only return (id, content, distance, branch).
pub fn get_memory_metadata_sync(
    conn: &rusqlite::Connection,
    ids: &[i64],
) -> rusqlite::Result<std::collections::HashMap<i64, (String, Option<String>)>> {
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT id, fact_type, category FROM memory_facts WHERE id IN ({})",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, (row.1, row.2));
    }
    Ok(map)
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

    let sql = format!(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id
         FROM memory_facts
         WHERE {}
           AND content LIKE ?2 ESCAPE '\\'
         ORDER BY updated_at DESC
         LIMIT ?3",
        scope_filter_sql("")
            .replace("?{pid}", "?1")
            .replace("?{uid}", "?4")
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(
        rusqlite::params![project_id, pattern, limit as i64, user_id],
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
    conn.execute(
        "DELETE FROM vec_memory WHERE fact_id = ?",
        rusqlite::params![id],
    )?;
    // Delete from facts table
    let deleted = conn.execute(
        "DELETE FROM memory_facts WHERE id = ?",
        rusqlite::params![id],
    )? > 0;
    Ok(deleted)
}

/// Get memory statistics for a project (sync version for pool.interact())
/// Returns (candidate_count, confirmed_count)
pub fn get_memory_stats_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<(i64, i64)> {
    let candidates: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_facts WHERE (project_id = ? OR project_id IS NULL) AND status = 'candidate'",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;

    let confirmed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_facts WHERE (project_id = ? OR project_id IS NULL) AND status = 'confirmed'",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;

    Ok((candidates, confirmed))
}

/// Get preferences for a project (sync version for pool.interact())
pub fn get_preferences_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id
         FROM memory_facts
         WHERE (project_id = ? OR project_id IS NULL) AND fact_type = 'preference'
         ORDER BY category, created_at DESC",
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id], parse_memory_fact_row)?;
    rows.collect()
}

/// Get health alerts for a project (sync version for pool.interact())
pub fn get_health_alerts_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<mira_types::MemoryFact>> {
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

    let rows = stmt.query_map(
        rusqlite::params![project_id, limit as i64],
        parse_memory_fact_row,
    )?;
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
                user_id, scope, team_id
         FROM memory_facts
         WHERE has_embedding = 0
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
          AND fact_type NOT IN ('health', 'persona')
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
        // NULL branch (pre-branch-tracking data) should get no boost
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
        let different_branch =
            apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

        // Same branch should have lower (better) distance
        assert!(same_branch < different_branch);
    }

    #[test]
    fn test_main_branch_beats_different_branch() {
        // main/master branch should rank better than different branch
        let base_distance = 0.5;

        let main_branch = apply_branch_boost(base_distance, Some("main"), Some("feature-x"));
        let different_branch =
            apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

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
