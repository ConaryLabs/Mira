// crates/mira-server/src/search/semantic.rs
// Semantic code search with hybrid fallback

use super::context::expand_context;
use super::keyword::keyword_search;
use super::utils::{distance_to_score, embedding_to_bytes};
use crate::db::Database;
use crate::embeddings::EmbeddingClient;
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
    embeddings: &Arc<EmbeddingClient>,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let query_embedding = embeddings.embed(query).await?;

    let embedding_bytes = embedding_to_bytes(&query_embedding);

    // Run vector search on blocking thread pool to avoid blocking tokio
    let db_clone = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
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
    .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {}", e))?
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
    embeddings: Option<&Arc<EmbeddingClient>>,
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
        tokio::task::spawn_blocking(move || {
            let conn = db_for_keyword.conn();
            keyword_search(
                &conn,
                &query_for_keyword,
                project_id,
                project_path_for_keyword.as_deref(),
                fetch_limit,
            )
        })
        .await
        .expect("keyword search spawn_blocking panicked")
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

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // SearchType tests
    // ============================================================================

    #[test]
    fn test_search_type_display() {
        assert_eq!(format!("{}", SearchType::Semantic), "semantic");
        assert_eq!(format!("{}", SearchType::Keyword), "keyword");
    }

    #[test]
    fn test_search_type_equality() {
        assert_eq!(SearchType::Semantic, SearchType::Semantic);
        assert_eq!(SearchType::Keyword, SearchType::Keyword);
        assert_ne!(SearchType::Semantic, SearchType::Keyword);
    }

    // ============================================================================
    // QueryIntent detection tests
    // ============================================================================

    #[test]
    fn test_detect_intent_documentation() {
        assert_eq!(detect_query_intent("docs for Database"), QueryIntent::Documentation);
        assert_eq!(detect_query_intent("documentation for API"), QueryIntent::Documentation);
        assert_eq!(detect_query_intent("what is SearchResult"), QueryIntent::Documentation);
        assert_eq!(detect_query_intent("explain the hybrid search"), QueryIntent::Documentation);
    }

    #[test]
    fn test_detect_intent_examples() {
        assert_eq!(detect_query_intent("example of using search"), QueryIntent::Examples);
        assert_eq!(detect_query_intent("usage of Database"), QueryIntent::Examples);
        assert_eq!(detect_query_intent("how to use embeddings"), QueryIntent::Examples);
        assert_eq!(detect_query_intent("where is the config"), QueryIntent::Examples);
        assert_eq!(detect_query_intent("who calls this function"), QueryIntent::Examples);
        assert_eq!(detect_query_intent("callers of semantic_search"), QueryIntent::Examples);
    }

    #[test]
    fn test_detect_intent_implementation() {
        assert_eq!(detect_query_intent("how does search work"), QueryIntent::Implementation);
        assert_eq!(detect_query_intent("implementation of caching"), QueryIntent::Implementation);
        assert_eq!(detect_query_intent("how is the score calculated"), QueryIntent::Implementation);
        assert_eq!(detect_query_intent("source of error handling"), QueryIntent::Implementation);
        assert_eq!(detect_query_intent("definition of SearchResult"), QueryIntent::Implementation);
    }

    #[test]
    fn test_detect_intent_general() {
        assert_eq!(detect_query_intent("search code"), QueryIntent::General);
        assert_eq!(detect_query_intent("Database struct"), QueryIntent::General);
        assert_eq!(detect_query_intent("error handling"), QueryIntent::General);
    }

    // ============================================================================
    // merge_results tests
    // ============================================================================

    #[test]
    fn test_merge_results_empty() {
        let (results, search_type) = merge_results(vec![], vec![]);
        assert!(results.is_empty());
        assert_eq!(search_type, SearchType::Keyword);
    }

    #[test]
    fn test_merge_results_semantic_only() {
        let semantic = vec![
            SearchResult {
                file_path: "src/main.rs".to_string(),
                content: "fn main()".to_string(),
                score: 0.9,
                start_line: 1,
            },
        ];
        let (results, search_type) = merge_results(semantic, vec![]);
        assert_eq!(results.len(), 1);
        assert_eq!(search_type, SearchType::Semantic);
    }

    #[test]
    fn test_merge_results_keyword_only() {
        let keyword = vec![
            SearchResult {
                file_path: "src/lib.rs".to_string(),
                content: "pub fn search()".to_string(),
                score: 0.8,
                start_line: 10,
            },
        ];
        let (results, search_type) = merge_results(vec![], keyword);
        assert_eq!(results.len(), 1);
        assert_eq!(search_type, SearchType::Keyword);
    }

    #[test]
    fn test_merge_results_deduplication() {
        let semantic = vec![
            SearchResult {
                file_path: "src/main.rs".to_string(),
                content: "fn main()".to_string(),
                score: 0.9,
                start_line: 1,
            },
        ];
        let keyword = vec![
            SearchResult {
                file_path: "src/main.rs".to_string(),
                content: "fn main()".to_string(),
                score: 0.7,
                start_line: 1,
            },
        ];
        let (results, _) = merge_results(semantic, keyword);
        assert_eq!(results.len(), 1);
        // Should keep higher score
        assert_eq!(results[0].score, 0.9);
    }

    #[test]
    fn test_merge_results_sorted_by_score() {
        let semantic = vec![
            SearchResult {
                file_path: "src/low.rs".to_string(),
                content: "low score".to_string(),
                score: 0.5,
                start_line: 1,
            },
        ];
        let keyword = vec![
            SearchResult {
                file_path: "src/high.rs".to_string(),
                content: "high score".to_string(),
                score: 0.95,
                start_line: 1,
            },
        ];
        let (results, _) = merge_results(semantic, keyword);
        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
        assert_eq!(results[0].file_path, "src/high.rs");
    }

    // ============================================================================
    // format_results tests
    // ============================================================================

    #[test]
    fn test_format_results_empty() {
        let output = format_results(&[], SearchType::Semantic, None, false);
        assert!(output.contains("No code matches found"));
        assert!(output.contains("index"));
    }

    #[test]
    fn test_format_results_basic() {
        let results = vec![
            SearchResult {
                file_path: "src/main.rs".to_string(),
                content: "fn main() { }".to_string(),
                score: 0.85,
                start_line: 1,
            },
        ];
        let output = format_results(&results, SearchType::Semantic, None, false);
        assert!(output.contains("1 results (semantic search)"));
        assert!(output.contains("src/main.rs:1"));
        assert!(output.contains("score: 0.85"));
        assert!(output.contains("fn main()"));
    }

    #[test]
    fn test_format_results_no_line_number() {
        let results = vec![
            SearchResult {
                file_path: "src/lib.rs".to_string(),
                content: "pub mod search;".to_string(),
                score: 0.75,
                start_line: 0,
            },
        ];
        let output = format_results(&results, SearchType::Keyword, None, false);
        assert!(output.contains("keyword search"));
        // Should not have line number when start_line is 0
        assert!(output.contains("## src/lib.rs ("));
        assert!(!output.contains("src/lib.rs:0"));
    }

    #[test]
    fn test_format_results_truncates_long_content() {
        let long_content = "x".repeat(600);
        let results = vec![
            SearchResult {
                file_path: "src/big.rs".to_string(),
                content: long_content,
                score: 0.9,
                start_line: 1,
            },
        ];
        let output = format_results(&results, SearchType::Semantic, None, false);
        assert!(output.contains("..."));
        // Output should be truncated, not contain all 600 chars
        assert!(output.len() < 700);
    }

    // ============================================================================
    // SearchResult tests
    // ============================================================================

    #[test]
    fn test_search_result_clone() {
        let result = SearchResult {
            file_path: "test.rs".to_string(),
            content: "content".to_string(),
            score: 0.5,
            start_line: 10,
        };
        let cloned = result.clone();
        assert_eq!(result.file_path, cloned.file_path);
        assert_eq!(result.content, cloned.content);
        assert_eq!(result.score, cloned.score);
        assert_eq!(result.start_line, cloned.start_line);
    }
}
