//! Shared memory operations for Mira ecosystem
//!
//! Provides common memory fact operations used by both MCP server and mira-chat:
//! - Key generation from content
//! - Upsert with project scoping
//! - Recall with semantic-first + text fallback
//! - Batch times_used updates (fixes N+1 query issue)

use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::semantic::SemanticSearch;

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

/// A memory fact result
#[derive(Debug, Clone)]
pub struct MemoryFact {
    pub id: String,
    pub key: String,
    pub value: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub project_id: Option<i64>,
    /// Semantic similarity score (if from semantic search)
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
pub async fn upsert_memory_fact(
    db: &SqlitePool,
    scope: MemoryScope,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    source: &str,
) -> Result<String> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();

    let project_id = match scope {
        MemoryScope::ProjectId(pid) => Some(pid),
        MemoryScope::Global => None,
    };

    sqlx::query(
        r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at, project_id)
        VALUES ($1, $2, $3, $4, $5, $6, 1.0, 0, $7, $7, $8)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            fact_type = excluded.fact_type,
            category = COALESCE(excluded.category, memory_facts.category),
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(fact_type)
    .bind(key)
    .bind(content)
    .bind(category)
    .bind(source)
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

            match sem.search(cfg.collection, query, limit, filter).await {
                Ok(results) if !results.is_empty() => {
                    // Extract keys for batch update
                    let keys: Vec<String> = results
                        .iter()
                        .filter_map(|r| {
                            r.metadata
                                .get("key")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .collect();

                    // Batch update times_used
                    if !keys.is_empty() {
                        batch_update_times_used_by_keys(db, &keys).await?;
                    }

                    return Ok(results
                        .into_iter()
                        .map(|r| {
                            // Extract key from metadata (used as ID for semantic results)
                            let key = r
                                .metadata
                                .get("key")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            MemoryFact {
                                id: key.clone(), // Use key as ID for semantic results
                                key,
                                value: r.content,
                                fact_type: r
                                    .metadata
                                    .get("fact_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("general")
                                    .to_string(),
                                category: r
                                    .metadata
                                    .get("category")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                project_id: r
                                    .metadata
                                    .get("project_id")
                                    .and_then(|v| v.as_i64()),
                                score: Some(r.score),
                                search_type: SearchType::Semantic,
                            }
                        })
                        .collect());
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

/// Recall using text LIKE search only (no semantic)
pub async fn recall_text_search(
    db: &SqlitePool,
    cfg: RecallConfig<'_>,
    query: &str,
    limit: usize,
    project_id: Option<i64>,
) -> Result<Vec<MemoryFact>> {
    let search_pattern = format!("%{}%", query);

    let rows: Vec<(String, String, String, String, Option<String>, Option<i64>)> = sqlx::query_as(
        r#"
        SELECT id, fact_type, key, value, category, project_id
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
        .map(|(id, fact_type, key, value, category, proj_id)| MemoryFact {
            id,
            key,
            value,
            fact_type,
            category,
            project_id: proj_id,
            score: None,
            search_type: SearchType::Text,
        })
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
