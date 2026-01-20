// crates/mira-server/src/search/semantic.rs
// Semantic code search with hybrid fallback

use super::context::expand_context;
use super::keyword::keyword_search;
use super::utils::{distance_to_score, embedding_to_bytes};
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::Result;
use rusqlite::params;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Search result with file path, content, and similarity score
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_path: String,
    pub content: String,
    pub score: f32,
    pub start_line: u32,
}

/// Search type indicator
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchType {
    Semantic,
    Keyword,
}

impl std::fmt::Display for SearchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchType::Semantic => write!(f, "semantic"),
            SearchType::Keyword => write!(f, "keyword"),
        }
    }
}

/// Result of hybrid search including which method was used
pub struct HybridSearchResult {
    pub results: Vec<SearchResult>,
    pub search_type: SearchType,
}

/// Semantic search using embeddings
pub async fn semantic_search(
    db: &Arc<Database>,
    embeddings: &Arc<Embeddings>,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let query_embedding = embeddings.embed(query).await?;

    let embedding_bytes = embedding_to_bytes(&query_embedding);

    // Run vector search on blocking thread pool to avoid blocking tokio
    let db_clone = db.clone();
    Database::run_blocking(db_clone, move |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?2) as distance, start_line
                 FROM vec_code
                 WHERE project_id = ?1 OR ?1 IS NULL
                 ORDER BY distance
                 LIMIT ?3",
            )
            ?;

        let results: Vec<SearchResult> = stmt
            .query_map(params![project_id, embedding_bytes, limit as i64], |row| {
                let start_line: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
                Ok(SearchResult {
                    file_path: row.get(0)?,
                    content: row.get(1)?,
                    score: distance_to_score(row.get(2)?),
                    start_line: start_line as u32,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    })
    .await
}

// ============================================================================
// Query Intent Detection
// ============================================================================

/// Detected query intent
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryIntent {
    /// Default: find relevant code
    General,
    /// "how does X work" - looking for implementation details
    Implementation,
    /// "example of X" - looking for usage patterns
    Examples,
    /// "docs for X" - looking for documentation
    Documentation,
}

/// Detect intent from query text
fn detect_query_intent(query: &str) -> QueryIntent {
    let q = query.to_lowercase();

    // Documentation intent
    if q.contains("docs for")
        || q.contains("documentation")
        || q.contains("what is")
        || q.contains("explain")
    {
        return QueryIntent::Documentation;
    }

    // Examples/usage intent
    if q.contains("example of")
        || q.contains("usage of")
        || q.contains("how to use")
        || q.contains("where is")
        || q.contains("who calls")
        || q.contains("callers of")
    {
        return QueryIntent::Examples;
    }

    // Implementation intent
    if q.contains("how does")
        || q.contains("implementation of")
        || q.contains("how is")
        || q.contains("source of")
        || q.contains("definition of")
    {
        return QueryIntent::Implementation;
    }

    QueryIntent::General
}

// ============================================================================
// Result Reranking
// ============================================================================

/// Rerank with intent-specific boosts
fn rerank_results_with_intent(
    results: &mut [SearchResult],
    project_path: Option<&str>,
    intent: QueryIntent,
) {
    use std::time::{Duration, SystemTime};

    let now = SystemTime::now();
    let one_day = Duration::from_secs(86400);
    let one_week = Duration::from_secs(86400 * 7);

    for result in results.iter_mut() {
        let mut boost = 1.0f32;

        // Boost complete symbols (not "(continued)" chunks)
        if !result.content.contains("(continued)") {
            boost *= 1.10;
        }

        // Boost documented code (has docstrings)
        let has_docs = result.content.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("///")
                || trimmed.starts_with("/**")
                || trimmed.starts_with("\"\"\"")
                || trimmed.starts_with("#")
        });
        if has_docs {
            boost *= 1.10;
        }

        // Intent-specific boosts
        match intent {
            QueryIntent::Documentation => {
                // Extra boost for documented code
                if has_docs {
                    boost *= 1.20;
                }
            }
            QueryIntent::Implementation => {
                // Boost function definitions (complete symbols with signatures)
                if result.content.contains("fn ")
                    || result.content.contains("def ")
                    || result.content.contains("function ")
                    || result.content.contains("pub fn ")
                {
                    boost *= 1.15;
                }
            }
            QueryIntent::Examples => {
                // Boost test files and usage patterns
                if result.file_path.contains("test")
                    || result.file_path.contains("example")
                    || result.file_path.contains("spec")
                {
                    boost *= 1.25;
                }
            }
            QueryIntent::General => {}
        }

        // Boost recent files (up to 20% for files modified in last day)
        if let Some(proj_path) = project_path {
            let full_path = Path::new(proj_path).join(&result.file_path);
            if let Ok(metadata) = std::fs::metadata(&full_path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        let recency_boost = if age < one_day {
                            1.20 // Modified today
                        } else if age < one_week {
                            1.10 // Modified this week
                        } else {
                            1.0 // Older
                        };
                        boost *= recency_boost;
                    }
                }
            }
        }

        result.score *= boost;
    }

    // Re-sort by adjusted score
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
}

// ============================================================================
// Hybrid Search
// ============================================================================

/// Merge search results from multiple sources with deduplication
/// Deduplicates by (file_path, start_line) and keeps the higher-scoring result
fn merge_results(
    semantic_results: Vec<SearchResult>,
    keyword_results: Vec<SearchResult>,
) -> (Vec<SearchResult>, SearchType) {
    // Use HashMap for deduplication by (file_path, start_line)
    let mut merged: HashMap<(String, u32), SearchResult> = HashMap::new();

    // Track which search type contributed more
    let semantic_count = semantic_results.len();
    let keyword_count = keyword_results.len();

    // Add semantic results first
    for result in semantic_results {
        let key = (result.file_path.clone(), result.start_line);
        merged.insert(key, result);
    }

    // Add keyword results, keeping higher score on collision
    for result in keyword_results {
        let key = (result.file_path.clone(), result.start_line);
        merged
            .entry(key)
            .and_modify(|existing| {
                if result.score > existing.score {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }

    // Convert to vec and sort by score descending
    let mut results: Vec<SearchResult> = merged.into_values().collect();
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Determine primary search type based on contribution
    let search_type = if semantic_count >= keyword_count && semantic_count > 0 {
        SearchType::Semantic
    } else {
        SearchType::Keyword
    };

    (results, search_type)
}

/// Hybrid search: runs semantic and keyword searches in parallel, merges results
/// Falls back to keyword-only if embeddings unavailable
pub async fn hybrid_search(
    db: &Arc<Database>,
    embeddings: Option<&Arc<Embeddings>>,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Result<HybridSearchResult> {
    // Fetch more results from each backend to account for deduplication
    let fetch_limit = limit * 2;

    // Prepare keyword search future (always runs)
    let db_for_keyword = db.clone();
    let query_for_keyword = query.to_string();
    let project_path_for_keyword = project_path.map(|s| s.to_string());
    let keyword_future = async move {
        Database::run_blocking(db_for_keyword, move |conn| {
            keyword_search(
                conn,
                &query_for_keyword,
                project_id,
                project_path_for_keyword.as_deref(),
                fetch_limit,
            )
        })
        .await
    };

    // Run searches in parallel
    let (semantic_results, keyword_results) = if let Some(emb) = embeddings {
        let emb = emb.clone();
        let db_for_semantic = db.clone();
        let query_for_semantic = query.to_string();

        let semantic_future = async move {
            semantic_search(&db_for_semantic, &emb, &query_for_semantic, project_id, fetch_limit).await
        };

        let (semantic_res, keyword_res) = tokio::join!(semantic_future, keyword_future);

        // Handle semantic search failure gracefully - continue with keyword results
        let semantic = semantic_res.unwrap_or_else(|e| {
            tracing::warn!("Semantic search failed, using keyword only: {}", e);
            Vec::new()
        });

        (semantic, keyword_res)
    } else {
        // No embeddings available, keyword only
        let keyword_res = keyword_future.await;
        (Vec::new(), keyword_res)
    };

    // Convert keyword results to SearchResult
    let keyword_results: Vec<SearchResult> = keyword_results
        .into_iter()
        .map(|(file_path, content, score, start_line)| SearchResult {
            file_path,
            content,
            score,
            start_line: start_line as u32,
        })
        .collect();

    tracing::debug!(
        "Hybrid search: {} semantic, {} keyword results before merge",
        semantic_results.len(),
        keyword_results.len()
    );

    // Merge and deduplicate results
    let (mut results, search_type) = merge_results(semantic_results, keyword_results);

    // Apply intent-based reranking
    let intent = detect_query_intent(query);
    rerank_results_with_intent(&mut results, project_path, intent);

    // Truncate to requested limit
    results.truncate(limit);

    tracing::debug!(
        "Hybrid search: {} results after merge (type: {})",
        results.len(),
        search_type
    );

    Ok(HybridSearchResult {
        results,
        search_type,
    })
}

// ============================================================================
// Result Formatting
// ============================================================================

/// Format search results for display
pub fn format_results(
    results: &[SearchResult],
    search_type: SearchType,
    project_path: Option<&str>,
    expand: bool,
) -> String {
    if results.is_empty() {
        return "No code matches found. Have you run 'index' yet?".to_string();
    }

    let mut response = format!("{} results ({} search):\n\n", results.len(), search_type);

    for result in results {
        let location = if result.start_line > 0 {
            format!("{}:{}", result.file_path, result.start_line)
        } else {
            result.file_path.clone()
        };
        response.push_str(&format!(
            "## {} (score: {:.2})\n",
            location, result.score
        ));

        let display_content = if expand {
            if let Some((symbol_info, expanded)) =
                expand_context(&result.file_path, &result.content, project_path)
            {
                if let Some(info) = symbol_info {
                    response.push_str(&format!("{}\n", info));
                }
                if expanded.len() > 1500 {
                    format!("{}...\n[truncated]", &expanded[..1500])
                } else {
                    expanded
                }
            } else {
                result.content.clone()
            }
        } else if result.content.len() > 500 {
            format!("{}...", &result.content[..500])
        } else {
            result.content.clone()
        };

        response.push_str(&format!("```\n{}\n```\n\n", display_content));
    }

    response
}
