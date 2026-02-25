// db/memory/recall.rs
// Memory recall: semantic search, keyword search, and access tracking

use std::sync::LazyLock;

use mira_types::MemoryFact;

use super::ranking::{
    RecallRow, TEAM_SCOPE_BOOST, apply_branch_boost, apply_entity_boost, apply_recency_boost,
    apply_staleness_penalty,
};
use super::{parse_memory_fact_row, scope_filter_sql};

/// User memory fact_types -- system types live in system_observations now
const USER_FACT_TYPES_SQL: &str =
    "f.fact_type IN ('general','preference','decision','pattern','context','persona')";

/// Cached semantic recall query with scope filtering.
///
/// Returns SQL that selects (fact_id, content, distance, branch, team_id, fact_type, category, status, updated_at, stale_since)
/// from vec_memory + memory_facts. Inlines metadata to avoid N+1 queries.
/// Parameters: ?1 = embedding_bytes, ?2 = project_id, ?3 = limit, ?4 = user_id, ?5 = team_id
static SEMANTIC_RECALL_SQL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT v.fact_id, f.content, vec_distance_cosine(v.embedding, ?1) as distance,
                f.branch, f.team_id, f.fact_type, f.category, f.status, f.updated_at,
                f.stale_since
         FROM vec_memory v
         JOIN memory_facts f ON v.fact_id = f.id
         WHERE {}
           AND {USER_FACT_TYPES_SQL}
           AND f.status != 'archived'
           AND COALESCE(f.suspicious, 0) = 0
         ORDER BY distance
         LIMIT ?3",
        scope_filter_sql("f.")
            .replace("?{pid}", "?2")
            .replace("?{uid}", "?4")
            .replace("?{tid}", "?5")
    )
});

fn semantic_recall_sql() -> &'static str {
    &SEMANTIC_RECALL_SQL
}

/// Semantic search with entity boost applied.
///
/// Wraps the branch-info recall to also apply entity-overlap ranking boost.
/// `query_entity_names` are the canonical names extracted from the query.
/// If empty, skips entity boost entirely (no extra query).
#[allow(clippy::too_many_arguments)]
pub fn recall_semantic_with_entity_boost_sync(
    conn: &rusqlite::Connection,
    embedding_bytes: &[u8],
    project_id: Option<i64>,
    user_id: Option<&str>,
    team_id: Option<i64>,
    current_branch: Option<&str>,
    query_entity_names: &[String],
    limit: usize,
) -> rusqlite::Result<Vec<RecallRow>> {
    use crate::db::entities::get_entity_match_counts_sync;

    // Fetch more results than needed to allow for re-ranking after boosting
    let fetch_limit = (limit * 2).min(100);

    let sql = semantic_recall_sql();
    let mut stmt = conn.prepare(sql)?;

    let results: Vec<RecallRow> = stmt
        .query_map(
            rusqlite::params![
                embedding_bytes,
                project_id,
                fetch_limit as i64,
                user_id,
                team_id
            ],
            |row| {
                Ok(RecallRow {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    distance: row.get(2)?,
                    branch: row.get(3)?,
                    team_id: row.get(4)?,
                    fact_type: row.get(5)?,
                    category: row.get(6)?,
                    status: row.get(7)?,
                    updated_at: row.get(8)?,
                    stale_since: row.get(9)?,
                })
            },
        )?
        .filter_map(crate::db::log_and_discard)
        .collect();

    // Get entity match counts (skip entirely if no query entities)
    let entity_counts = if !query_entity_names.is_empty() {
        get_entity_match_counts_sync(conn, project_id, query_entity_names).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    // Filter by raw distance before any boosts -- quality gate on true semantic similarity
    let filtered: Vec<RecallRow> = results.into_iter().filter(|r| r.distance < 0.85).collect();

    // Apply branch + entity + team + recency boosts for ranking only
    let mut boosted: Vec<RecallRow> = filtered
        .into_iter()
        .map(|mut r| {
            r.distance = apply_branch_boost(r.distance, r.branch.as_deref(), current_branch);
            if let Some(&match_count) = entity_counts.get(&r.id) {
                r.distance = apply_entity_boost(r.distance, match_count);
            }
            // Team boost: memories from the same team get a 10% distance reduction
            if team_id.is_some() && r.team_id == team_id {
                r.distance *= TEAM_SCOPE_BOOST;
            }
            // Recency boost: recent memories get up to 5% distance reduction
            r.distance = apply_recency_boost(r.distance, r.updated_at.as_deref());
            // Staleness penalty: memories whose linked code changed get deprioritized
            r.distance = apply_staleness_penalty(r.distance, r.stale_since.as_deref());
            r
        })
        .collect();

    boosted.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    boosted.truncate(limit);

    Ok(boosted)
}

/// Search memories by text with scope filtering (sync version for pool.interact())
///
/// Splits query into keywords (>3 chars, up to 5) and OR-joins LIKE clauses
/// for better multi-word matching. Results are ranked by keyword match count,
/// then by recency. Falls back to full-string LIKE for very short queries.
pub fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    user_id: Option<&str>,
    team_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<MemoryFact>> {
    /// Escape SQL LIKE wildcards in a single keyword
    fn escape_like(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    }

    // Extract keywords: words > 3 chars, take up to 5
    let keywords: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .map(|w| format!("%{}%", escape_like(w)))
        .collect();

    let scope_sql = scope_filter_sql("")
        .replace("?{pid}", "?1")
        .replace("?{uid}", "?2")
        .replace("?{tid}", "?3");

    // If no keywords > 3 chars, fall back to full-string LIKE
    if keywords.is_empty() {
        let escaped = escape_like(query);
        let pattern = format!("%{}%", escaped);

        let sql = format!(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id, updated_at, branch
             FROM memory_facts
             WHERE {scope_sql}
               AND fact_type IN ('general','preference','decision','pattern','context','persona')
               AND status != 'archived'
               AND COALESCE(suspicious, 0) = 0
               AND content LIKE ?4 ESCAPE '\\'
             ORDER BY updated_at DESC
             LIMIT ?5"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params![project_id, user_id, team_id, pattern, limit as i64],
            parse_memory_fact_row,
        )?;
        return rows.collect();
    }

    // Build OR-joined WHERE filter and match-count ORDER BY scoring
    // Keywords appear twice in params: once for WHERE filter, once for ORDER BY scoring
    let where_clauses: Vec<String> = (0..keywords.len())
        .map(|i| format!("content LIKE ?{} ESCAPE '\\'", 4 + i))
        .collect();
    let where_sql = where_clauses.join(" OR ");

    let score_cases: Vec<String> = (0..keywords.len())
        .map(|i| {
            format!(
                "CASE WHEN content LIKE ?{} ESCAPE '\\' THEN 1 ELSE 0 END",
                4 + keywords.len() + i
            )
        })
        .collect();
    let score_sql = score_cases.join(" + ");

    let sql = format!(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                session_count, first_session_id, last_session_id, status,
                user_id, scope, team_id, updated_at, branch
         FROM memory_facts
         WHERE {scope_sql}
           AND fact_type IN ('general','preference','decision','pattern','context','persona')
           AND status != 'archived'
           AND COALESCE(suspicious, 0) = 0
           AND ({where_sql})
         ORDER BY ({score_sql}) DESC, updated_at DESC
         LIMIT ?{}",
        4 + keywords.len() * 2
    );

    let mut stmt = conn.prepare(&sql)?;

    // Build params: project_id, user_id, team_id, [keywords for WHERE], [keywords for ORDER BY], limit
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(project_id));
    params.push(Box::new(user_id.map(|s| s.to_string())));
    params.push(Box::new(team_id));
    // Keywords for WHERE filter
    for kw in &keywords {
        params.push(Box::new(kw.clone()));
    }
    // Same keywords again for ORDER BY scoring
    for kw in &keywords {
        params.push(Box::new(kw.clone()));
    }
    params.push(Box::new(limit as i64));

    let rows = stmt.query_map(rusqlite::params_from_iter(params), parse_memory_fact_row)?;
    rows.collect()
}

/// Record that a memory was accessed in a session (sync version for pool.interact())
/// Handles session count increment and automatic promotion
pub fn record_memory_access_sync(
    conn: &rusqlite::Connection,
    memory_id: i64,
    session_id: &str,
) -> rusqlite::Result<()> {
    // Atomic conditional update avoids read-then-write races under concurrency.
    let updated = conn.execute(
        "UPDATE memory_facts
         SET session_count = session_count + 1,
             last_session_id = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2
           AND COALESCE(last_session_id, '') != ?1",
        rusqlite::params![session_id, memory_id],
    )?;

    if updated > 0 {
        // Check for promotion
        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
             WHERE id = ? AND status = 'candidate' AND session_count >= 3",
            [memory_id],
        )?;
    }

    Ok(())
}
