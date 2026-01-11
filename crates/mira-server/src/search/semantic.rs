// crates/mira-server/src/search/semantic.rs
// Semantic code search with hybrid fallback

use super::context::expand_context;
use super::keyword::keyword_search;
use super::utils::{distance_to_score, embedding_to_bytes};
use crate::db::Database;
use crate::embeddings::Embeddings;
use rusqlite::params;
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
) -> Result<Vec<SearchResult>, String> {
    let query_embedding = embeddings
        .embed(query)
        .await
        .map_err(|e| format!("Embedding failed: {}", e))?;

    let embedding_bytes = embedding_to_bytes(&query_embedding);
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?2) as distance, start_line
             FROM vec_code
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY distance
             LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;

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

/// Hybrid search: semantic first, keyword fallback if poor results
/// Threshold: fall back if best semantic score < 0.25
pub async fn hybrid_search(
    db: &Arc<Database>,
    embeddings: Option<&Arc<Embeddings>>,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Result<HybridSearchResult, String> {
    const FALLBACK_THRESHOLD: f32 = 0.25;

    // Try semantic search first if embeddings available
    let semantic_results = if let Some(emb) = embeddings {
        semantic_search(db, emb, query, project_id, limit).await?
    } else {
        Vec::new()
    };

    // Check quality
    let best_score = semantic_results
        .iter()
        .map(|r| r.score)
        .fold(0.0f32, |a, b| a.max(b));

    // Decide if we need keyword fallback
    if semantic_results.is_empty() || best_score < FALLBACK_THRESHOLD {
        let conn = db.conn();
        let keyword_results = keyword_search(&conn, query, project_id, project_path, limit);

        if !keyword_results.is_empty() {
            tracing::debug!(
                "Semantic search poor (best_score={:.2}), using {} keyword results",
                best_score,
                keyword_results.len()
            );
            let mut results: Vec<SearchResult> = keyword_results
                .into_iter()
                .map(|(file_path, content, score)| SearchResult {
                    file_path,
                    content,
                    score,
                    start_line: 0, // Keyword search doesn't have line numbers
                })
                .collect();

            // Apply reranking boosts with intent
            let intent = detect_query_intent(query);
            rerank_results_with_intent(&mut results, project_path, intent);

            return Ok(HybridSearchResult {
                results,
                search_type: SearchType::Keyword,
            });
        }
    }

    // Apply reranking boosts with intent to semantic results
    let mut results = semantic_results;
    let intent = detect_query_intent(query);
    rerank_results_with_intent(&mut results, project_path, intent);

    Ok(HybridSearchResult {
        results,
        search_type: SearchType::Semantic,
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
