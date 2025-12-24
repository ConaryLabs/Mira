//! Shared memory operations for Mira ecosystem
//!
//! Provides common memory fact operations used by both MCP server and mira-chat:
//! - Key generation from content
//! - Upsert with project scoping
//! - Recall with semantic-first + text fallback
//! - Query-time freshness decay (exponential)
//! - Weighted scoring (qdrant * 0.7 + freshness * 0.2 + confidence * 0.1)
//! - Batch times_used updates (fixes N+1 query issue)

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::semantic::SemanticSearch;

/// Oversample factor for semantic search (fetch more, rerank locally)
const SEMANTIC_OVERSAMPLE: usize = 100;

// ============================================================================
// Types
// ============================================================================

/// Scope for memory facts - controls project isolation
#[derive(Debug, Clone)]
pub enum MemoryScope {
    /// Scoped to a specific project by ID
    ProjectId(i64),
    /// Global (no project scoping)
    Global,
}

/// Configuration for recall operations
#[derive(Debug, Clone, Default)]
pub struct RecallConfig<'a> {
    /// Qdrant collection name for semantic search
    pub collection: &'a str,
    /// Filter by fact_type (preference, decision, context, general)
    pub fact_type: Option<&'a str>,
    /// Filter by category
    pub category: Option<&'a str>,
}

/// Validity state for a memory fact
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Validity {
    #[default]
    Active,
    Stale,
    Superseded,
}

impl Validity {
    pub fn from_str(s: &str) -> Self {
        match s {
            "stale" => Validity::Stale,
            "superseded" => Validity::Superseded,
            _ => Validity::Active,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Validity::Active => "active",
            Validity::Stale => "stale",
            Validity::Superseded => "superseded",
        }
    }

    fn penalty(&self) -> f32 {
        match self {
            Validity::Active => 1.0,
            Validity::Stale => 0.1,
            Validity::Superseded => 0.05,
        }
    }
}

/// A memory fact result
#[derive(Debug, Clone)]
pub struct MemoryFact {
    pub id: String,
    pub key: String,
    pub value: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub project_id: Option<i64>,
    pub file_path: Option<String>,
    pub validity: Validity,
    pub confidence: f32,
    pub created_at: i64,
    /// Raw semantic similarity score (before decay)
    pub raw_score: Option<f32>,
    /// Final score after decay/weighting
    pub score: Option<f32>,
    /// How the result was found
    pub search_type: SearchType,
}

/// How a memory fact was found
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    Semantic,
    Text,
}

impl SearchType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchType::Semantic => "semantic",
            SearchType::Text => "text",
        }
    }
}

// ============================================================================
// Freshness & Scoring (Query-Time Decay)
// ============================================================================

/// Compute freshness weight based on age and category
///
/// Uses exponential decay with category-specific half-lives.
/// Decisions and preferences never decay.
pub fn compute_freshness(created_at: i64, category: Option<&str>, fact_type: &str) -> f32 {
    // Decisions and preferences don't decay
    if fact_type == "decision" || fact_type == "preference" {
        return 1.0;
    }

    let now = Utc::now().timestamp();
    let days_old = ((now - created_at) as f32) / 86400.0;

    // Half-life in days (exponential decay)
    let half_life = match category {
        Some("session_activity") => 3.0,  // Ephemeral
        Some("research") => 14.0,         // Research insights last longer
        Some("compaction") => 21.0,       // Distilled content
        _ => 10.0,                         // Default/general
    };

    let floor = 0.01;
    let decay = 0.5_f32.powf(days_old / half_life);
    floor + (1.0 - floor) * decay
}

/// Compute final score with weighted blend
///
/// Formula: (qdrant_score * 0.7 + freshness * 0.2 + confidence * 0.1) * validity_penalty
pub fn compute_final_score(
    qdrant_score: f32,
    confidence: f32,
    freshness: f32,
    validity: Validity,
) -> f32 {
    let base = (qdrant_score * 0.7) + (freshness * 0.2) + (confidence * 0.1);
    base * validity.penalty()
}

// ============================================================================
// Key Generation
// ============================================================================

/// Generate a normalized key from content
///
/// Takes first 50 chars, lowercases, keeps only alphanumeric + spaces, trims.
/// Used for upsert conflict detection.
pub fn make_memory_key(content: &str) -> String {
    content
        .chars()
        .take(50)
        .collect::<String>()
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
        .trim()
        .to_string()
}

// ============================================================================
// Upsert
// ============================================================================

/// Upsert a memory fact into the database
///
/// Returns the fact ID (either new or existing on conflict).
/// Optional confidence parameter (defaults to 1.0). Use 0.8 for compaction summaries.
pub async fn upsert_memory_fact(
    db: &SqlitePool,
    scope: MemoryScope,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    source: &str,
) -> Result<String> {
    upsert_memory_fact_with_confidence(db, scope, key, content, fact_type, category, source, None).await
}

/// Upsert a memory fact with explicit confidence value
pub async fn upsert_memory_fact_with_confidence(
    db: &SqlitePool,
    scope: MemoryScope,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    source: &str,
    confidence: Option<f64>,
) -> Result<String> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let conf = confidence.unwrap_or(1.0);

    let project_id = match scope {
        MemoryScope::ProjectId(pid) => Some(pid),
        MemoryScope::Global => None,
    };

    sqlx::query(
        r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at, project_id, validity)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $8, $9, 'active')
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            fact_type = excluded.fact_type,
            category = COALESCE(excluded.category, memory_facts.category),
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            confidence = excluded.confidence,
            validity = 'active',
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(fact_type)
    .bind(key)
    .bind(content)
    .bind(category)
    .bind(source)
    .bind(conf)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    Ok(id)
}

// ============================================================================
// Recall
// ============================================================================

/// Recall memory facts matching a query
///
/// Uses semantic search first (if available), falls back to text LIKE search.
/// Applies query-time freshness decay and weighted scoring, then reranks.
/// Automatically updates times_used in a single batch query.
pub async fn recall_memory_facts(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    cfg: RecallConfig<'_>,
    query: &str,
    limit: usize,
    project_id: Option<i64>,
) -> Result<Vec<MemoryFact>> {
    // Try semantic search first
    if let Some(sem) = semantic {
        if sem.is_available() {
            let filter = cfg.fact_type.map(|ft| {
                qdrant_client::qdrant::Filter::must([qdrant_client::qdrant::Condition::matches(
                    "fact_type",
                    ft.to_string(),
                )])
            });

            // Oversample: fetch more candidates for reranking
            let fetch_limit = SEMANTIC_OVERSAMPLE.max(limit);

            match sem.search(cfg.collection, query, fetch_limit, filter).await {
                Ok(results) if !results.is_empty() => {
                    // Extract keys for DB lookup
                    let keys: Vec<String> = results
                        .iter()
                        .filter_map(|r| {
                            r.metadata
                                .get("key")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .collect();

                    // Fetch decay-relevant fields from DB
                    let db_facts = fetch_decay_metadata(db, &keys).await?;

                    // Build scored results with freshness decay
                    let mut scored: Vec<(MemoryFact, f32)> = results
                        .into_iter()
                        .filter_map(|r| {
                            let key = r
                                .metadata
                                .get("key")
                                .and_then(|v| v.as_str())?
                                .to_string();

                            // Get DB metadata for this key
                            let db_meta = db_facts.get(&key);

                            let fact_type = r
                                .metadata
                                .get("fact_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("general")
                                .to_string();

                            let category = r
                                .metadata
                                .get("category")
                                .and_then(|v| v.as_str())
                                .map(String::from);

                            // Use DB values if available, otherwise defaults
                            let (id, confidence, validity, created_at, file_path) =
                                if let Some(meta) = db_meta {
                                    (
                                        meta.id.clone(),
                                        meta.confidence,
                                        meta.validity,
                                        meta.created_at,
                                        meta.file_path.clone(),
                                    )
                                } else {
                                    (key.clone(), 1.0, Validity::Active, Utc::now().timestamp(), None)
                                };

                            // Compute freshness and final score
                            let freshness =
                                compute_freshness(created_at, category.as_deref(), &fact_type);
                            let final_score =
                                compute_final_score(r.score, confidence, freshness, validity);

                            let fact = MemoryFact {
                                id,
                                key,
                                value: r.content,
                                fact_type,
                                category,
                                project_id: r
                                    .metadata
                                    .get("project_id")
                                    .and_then(|v| v.as_i64()),
                                file_path,
                                validity,
                                confidence,
                                created_at,
                                raw_score: Some(r.score),
                                score: Some(final_score),
                                search_type: SearchType::Semantic,
                            };

                            Some((fact, final_score))
                        })
                        .collect();

                    // Rerank by final score (descending)
                    scored.sort_by(|a, b| {
                        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Truncate to requested limit
                    scored.truncate(limit);

                    // Extract keys for times_used update (only the ones we're returning)
                    let final_keys: Vec<String> =
                        scored.iter().map(|(f, _)| f.key.clone()).collect();
                    if !final_keys.is_empty() {
                        batch_update_times_used_by_keys(db, &final_keys).await?;
                    }

                    return Ok(scored.into_iter().map(|(f, _)| f).collect());
                }
                Ok(_) => {
                    tracing::debug!("No semantic results for query: {}", query);
                }
                Err(e) => {
                    tracing::warn!("Semantic search failed, falling back to text: {}", e);
                }
            }
        }
    }

    // Fallback to text search
    recall_text_search(db, cfg, query, limit, project_id).await
}

/// Metadata needed for decay calculations
struct DecayMetadata {
    id: String,
    confidence: f32,
    validity: Validity,
    created_at: i64,
    file_path: Option<String>,
}

/// Fetch decay-relevant metadata from DB for a set of keys
async fn fetch_decay_metadata(
    db: &SqlitePool,
    keys: &[String],
) -> Result<std::collections::HashMap<String, DecayMetadata>> {
    use std::collections::HashMap;

    if keys.is_empty() {
        return Ok(HashMap::new());
    }

    // Build query with placeholders
    let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("${}", i)).collect();
    let query = format!(
        "SELECT id, key, confidence, validity, created_at, file_path FROM memory_facts WHERE key IN ({})",
        placeholders.join(", ")
    );

    let mut q = sqlx::query_as::<_, (String, String, Option<f64>, Option<String>, i64, Option<String>)>(&query);
    for key in keys {
        q = q.bind(key);
    }

    let rows = q.fetch_all(db).await?;

    Ok(rows
        .into_iter()
        .map(|(id, key, confidence, validity, created_at, file_path)| {
            (
                key,
                DecayMetadata {
                    id,
                    confidence: confidence.unwrap_or(1.0) as f32,
                    validity: Validity::from_str(validity.as_deref().unwrap_or("active")),
                    created_at,
                    file_path,
                },
            )
        })
        .collect())
}

/// Recall using text LIKE search only (no semantic)
///
/// Also applies freshness decay for consistent scoring.
pub async fn recall_text_search(
    db: &SqlitePool,
    cfg: RecallConfig<'_>,
    query: &str,
    limit: usize,
    project_id: Option<i64>,
) -> Result<Vec<MemoryFact>> {
    let search_pattern = format!("%{}%", query);

    // Fetch more fields for decay calculation
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<f64>,
        Option<String>,
        i64,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT id, fact_type, key, value, category, project_id,
               confidence, validity, created_at, file_path
        FROM memory_facts
        WHERE (value LIKE $1 OR key LIKE $1 OR category LIKE $1)
          AND ($2 IS NULL OR fact_type = $2)
          AND ($3 IS NULL OR category = $3)
          AND (project_id IS NULL OR $4 IS NULL OR project_id = $4)
        ORDER BY times_used DESC, updated_at DESC
        LIMIT $5
        "#,
    )
    .bind(&search_pattern)
    .bind(cfg.fact_type)
    .bind(cfg.category)
    .bind(project_id)
    .bind(limit as i64)
    .fetch_all(db)
    .await?;

    // Batch update times_used
    let ids: Vec<String> = rows.iter().map(|(id, ..)| id.clone()).collect();
    if !ids.is_empty() {
        batch_update_times_used(db, &ids).await?;
    }

    Ok(rows
        .into_iter()
        .map(
            |(id, fact_type, key, value, category, proj_id, confidence, validity, created_at, file_path)| {
                let confidence = confidence.unwrap_or(1.0) as f32;
                let validity = Validity::from_str(validity.as_deref().unwrap_or("active"));
                let freshness = compute_freshness(created_at, category.as_deref(), &fact_type);

                MemoryFact {
                    id,
                    key,
                    value,
                    fact_type,
                    category,
                    project_id: proj_id,
                    file_path,
                    validity,
                    confidence,
                    created_at,
                    raw_score: None,
                    score: Some(freshness), // Use freshness as score for text search
                    search_type: SearchType::Text,
                }
            },
        )
        .collect())
}

// ============================================================================
// Forget
// ============================================================================

/// Delete a memory fact by ID
///
/// Returns true if deleted, false if not found.
pub async fn forget_memory_fact(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    collection: &str,
    id: &str,
) -> Result<bool> {
    let result = sqlx::query("DELETE FROM memory_facts WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;

    // Also delete from Qdrant if available
    if let Some(sem) = semantic {
        if sem.is_available() {
            if let Err(e) = sem.delete(collection, id).await {
                tracing::warn!("Failed to delete from Qdrant: {}", e);
            }
        }
    }

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Batch Updates (fixes N+1 issue)
// ============================================================================

/// Batch update times_used for multiple fact IDs
async fn batch_update_times_used(db: &SqlitePool, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let now = Utc::now().timestamp();

    // Build placeholders: $2, $3, $4, ...
    let placeholders: Vec<String> = (2..=ids.len() + 1).map(|i| format!("${}", i)).collect();
    let placeholder_str = placeholders.join(", ");

    let query = format!(
        "UPDATE memory_facts SET times_used = times_used + 1, last_used_at = $1 WHERE id IN ({})",
        placeholder_str
    );

    let mut q = sqlx::query(&query).bind(now);
    for id in ids {
        q = q.bind(id);
    }

    q.execute(db).await?;
    Ok(())
}

/// Batch update times_used for multiple fact keys
async fn batch_update_times_used_by_keys(db: &SqlitePool, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let now = Utc::now().timestamp();

    // Build placeholders
    let placeholders: Vec<String> = (2..=keys.len() + 1).map(|i| format!("${}", i)).collect();
    let placeholder_str = placeholders.join(", ");

    let query = format!(
        "UPDATE memory_facts SET times_used = times_used + 1, last_used_at = $1 WHERE key IN ({})",
        placeholder_str
    );

    let mut q = sqlx::query(&query).bind(now);
    for key in keys {
        q = q.bind(key);
    }

    q.execute(db).await?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_memory_key() {
        assert_eq!(make_memory_key("Hello, World!"), "hello world");
        assert_eq!(make_memory_key("Test 123 @#$"), "test 123");
        assert_eq!(
            make_memory_key("This is a very long string that exceeds fifty characters limit"),
            "this is a very long string that exceeds fifty char"
        );
        assert_eq!(make_memory_key("  spaces  "), "spaces");
    }

    #[test]
    fn test_search_type_as_str() {
        assert_eq!(SearchType::Semantic.as_str(), "semantic");
        assert_eq!(SearchType::Text.as_str(), "text");
    }
}
