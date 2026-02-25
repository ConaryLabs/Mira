// crates/mira-server/src/search/semantic.rs
// Semantic code search with hybrid parallel search

use super::context::expand_context;
use super::keyword::keyword_search;
use super::skeleton::skeletonize_content;
use super::utils::{Locatable, deduplicate_by_location, distance_to_score, embedding_to_bytes};
use crate::Result;
use crate::db::pool::DatabasePool;
use crate::db::semantic_code_search_sync;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::utils::{safe_join, truncate, truncate_at_boundary};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Bounds concurrent fuzzy search tasks to prevent unbounded background work.
static FUZZY_SEMAPHORE: Semaphore = Semaphore::const_new(1);

/// Search result with file path, content, and similarity score
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_path: String,
    pub content: String,
    pub score: f32,
    pub start_line: u32,
}

impl Locatable for SearchResult {
    fn file_path(&self) -> &str {
        &self.file_path
    }
    fn start_line(&self) -> i64 {
        self.start_line as i64
    }
    fn score(&self) -> f32 {
        self.score
    }
}

/// Search type indicator
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchType {
    Semantic,
    Keyword,
    Fuzzy,
}

impl std::fmt::Display for SearchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchType::Semantic => write!(f, "semantic"),
            SearchType::Keyword => write!(f, "keyword"),
            SearchType::Fuzzy => write!(f, "fuzzy"),
        }
    }
}

/// Result of hybrid search including which method was used
pub struct HybridSearchResult {
    pub results: Vec<SearchResult>,
    pub search_type: SearchType,
    /// Detected query intent, exposed so callers can apply intent-specific
    /// post-processing (e.g. caller enrichment for Refactor intent)
    pub intent: QueryIntent,
}

/// Semantic search using embeddings
pub async fn semantic_search(
    pool: &Arc<DatabasePool>,
    embeddings: &Arc<EmbeddingClient>,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let query_embedding = embeddings.embed(query).await?;

    let embedding_bytes = embedding_to_bytes(&query_embedding);

    // Run vector search on pool's blocking thread
    let db_results = pool
        .interact(move |conn| {
            semantic_code_search_sync(conn, &embedding_bytes, project_id, limit)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await?;

    let results: Vec<SearchResult> = db_results
        .into_iter()
        .map(|r| SearchResult {
            file_path: r.file_path,
            content: r.chunk_content,
            score: distance_to_score(r.distance),
            start_line: r.start_line as u32,
        })
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
    /// "fix bug", "error handling", "failing test" - debugging/troubleshooting
    Debug,
    /// "refactor", "restructure", "move", "extract" - code restructuring
    Refactor,
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

    // Debug intent - fix/troubleshoot/error patterns
    if q.contains("fix")
        || q.contains("bug")
        || q.contains("error")
        || q.contains("broken")
        || q.contains("failing")
        || q.contains("crash")
        || q.contains("debug")
        || q.contains("troubleshoot")
        || q.contains("panic")
        || q.contains("issue with")
    {
        return QueryIntent::Debug;
    }

    // Refactor intent - restructure/reorganize patterns
    if q.contains("refactor")
        || q.contains("restructure")
        || q.contains("reorganize")
        || q.contains("move ")
        || q.contains("rename")
        || q.contains("extract ")
        || q.contains("split ")
        || q.contains("merge ")
        || q.contains("consolidate")
        || q.contains("blast radius")
        || q.contains("impact of changing")
    {
        return QueryIntent::Refactor;
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
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};

    let now = SystemTime::now();
    let one_day = Duration::from_secs(86400);
    let one_week = Duration::from_secs(86400 * 7);

    // Pre-cache file modification times to avoid redundant stat calls
    let mod_times: HashMap<String, SystemTime> = if let Some(proj_path) = project_path {
        let mut cache = HashMap::new();
        for result in results.iter() {
            if cache.contains_key(&result.file_path) {
                continue;
            }
            let Some(full_path) = safe_join(Path::new(proj_path), &result.file_path) else {
                continue;
            };
            if let Ok(metadata) = std::fs::metadata(&full_path)
                && let Ok(modified) = metadata.modified()
            {
                cache.insert(result.file_path.clone(), modified);
            }
        }
        cache
    } else {
        HashMap::new()
    };

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
                || trimmed.starts_with("# ")
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
            QueryIntent::Debug => {
                // Boost error handling code
                if result.content.contains("Error")
                    || result.content.contains("error")
                    || result.content.contains("Result<")
                    || result.content.contains("unwrap")
                    || result.content.contains("expect(")
                    || result.content.contains("panic!")
                    || result.content.contains("bail!")
                    || result.content.contains("anyhow!")
                    || result.content.contains("catch")
                    || result.content.contains("except")
                    || result.content.contains("try")
                {
                    boost *= 1.25;
                }
                // Boost test files for debugging context
                if result.file_path.contains("test") {
                    boost *= 1.20;
                }
            }
            QueryIntent::Refactor => {
                // Boost public API surfaces (signatures matter most for refactoring)
                if result.content.contains("pub fn ")
                    || result.content.contains("pub struct ")
                    || result.content.contains("pub enum ")
                    || result.content.contains("pub trait ")
                    || result.content.contains("export ")
                {
                    boost *= 1.30;
                }
                // Boost files with impl blocks (high-impact refactor targets)
                if result.content.contains("impl ") {
                    boost *= 1.15;
                }
            }
            QueryIntent::General => {}
        }

        // Boost recent files -- stronger for Debug intent since recently changed
        // code is likely the source of bugs
        if let Some(modified) = mod_times.get(&result.file_path)
            && let Ok(age) = now.duration_since(*modified)
        {
            let recency_boost = if intent == QueryIntent::Debug {
                if age < one_day {
                    1.30 // Much stronger recency boost for debug
                } else if age < one_week {
                    1.15
                } else {
                    1.0
                }
            } else if age < one_day {
                1.20 // Modified today
            } else if age < one_week {
                1.10 // Modified this week
            } else {
                1.0 // Older
            };
            boost *= recency_boost;
        }

        result.score *= boost;
    }

    // Re-sort by adjusted score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ============================================================================
// Hybrid Search
// ============================================================================

/// Merge search results from multiple sources with deduplication
/// Deduplicates by (file_path, start_line) and keeps the higher-scoring result
fn merge_results(
    semantic_results: Vec<SearchResult>,
    keyword_results: Vec<SearchResult>,
    fuzzy_results: Vec<SearchResult>,
) -> (Vec<SearchResult>, SearchType) {
    // Track which search type contributed more
    let semantic_count = semantic_results.len();
    let keyword_count = keyword_results.len();
    let fuzzy_count = fuzzy_results.len();

    // Combine all results and deduplicate
    let mut all_results = semantic_results;
    all_results.extend(keyword_results);
    all_results.extend(fuzzy_results);
    let results = deduplicate_by_location(all_results);

    // Determine primary search type based on contribution
    let search_type =
        if semantic_count >= keyword_count && semantic_count >= fuzzy_count && semantic_count > 0 {
            SearchType::Semantic
        } else if fuzzy_count > keyword_count && fuzzy_count > 0 {
            SearchType::Fuzzy
        } else {
            SearchType::Keyword
        };

    (results, search_type)
}

/// Hybrid search: runs semantic, keyword, and fuzzy searches in parallel, merges results.
/// Each backend gracefully degrades if unavailable (no embeddings, no fuzzy cache).
pub async fn hybrid_search(
    pool: &Arc<DatabasePool>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    fuzzy: Option<&Arc<FuzzyCache>>,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Result<HybridSearchResult> {
    // Detect intent early so we can adjust search strategy
    let intent = detect_query_intent(query);

    // Debug intent benefits from more context -- increase limit by 50%
    let effective_limit = if intent == QueryIntent::Debug {
        (limit * 3).div_ceil(2) // ceil(limit * 1.5)
    } else {
        limit
    };

    // Fetch more results from each backend to account for deduplication
    let fetch_limit = effective_limit * 2;

    // Prepare keyword search future (always runs)
    let pool_for_keyword = pool.clone();
    let query_for_keyword = query.to_string();
    let project_path_for_keyword = project_path.map(|s| s.to_string());
    let keyword_future = async move {
        pool_for_keyword
            .interact(move |conn| {
                Ok(keyword_search(
                    conn,
                    &query_for_keyword,
                    project_id,
                    project_path_for_keyword.as_deref(),
                    fetch_limit,
                ))
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Keyword search pool interaction failed: {}", e);
                Vec::new()
            })
    };

    // Prepare semantic search future (runs if embeddings available)
    let semantic_future = async {
        if let Some(emb) = embeddings {
            let emb = emb.clone();
            let pool_for_semantic = pool.clone();
            let query_for_semantic = query.to_string();
            semantic_search(
                &pool_for_semantic,
                &emb,
                &query_for_semantic,
                project_id,
                fetch_limit,
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Semantic search failed: {}", e);
                Vec::new()
            })
        } else {
            Vec::new()
        }
    };

    // Prepare fuzzy search future with timeout and bounded concurrency.
    // Uses tokio::spawn so cache warmup completes in background on timeout.
    // Semaphore limits to 1 concurrent fuzzy task to prevent pileup under load.
    let pool_for_fuzzy = pool.clone();
    let query_for_fuzzy = query.to_string();
    let fuzzy_future = async move {
        if let Some(cache) = fuzzy {
            let permit = FUZZY_SEMAPHORE.try_acquire();
            if permit.is_err() {
                tracing::debug!("Fuzzy search skipped, another fuzzy task is running");
                return Vec::new();
            }
            let cache = cache.clone();
            let handle = tokio::spawn(async move {
                let result = cache
                    .search_code(&pool_for_fuzzy, project_id, &query_for_fuzzy, fetch_limit)
                    .await;
                drop(permit);
                result
            });
            match tokio::time::timeout(Duration::from_millis(500), handle).await {
                Ok(Ok(Ok(results))) => results,
                Ok(Ok(Err(e))) => {
                    tracing::warn!("Fuzzy search failed: {}", e);
                    Vec::new()
                }
                Ok(Err(e)) => {
                    tracing::warn!("Fuzzy search task panicked: {}", e);
                    Vec::new()
                }
                Err(_) => {
                    tracing::debug!("Fuzzy search timed out (500ms), cache warming in background");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    };

    // Run all three searches in parallel
    let (semantic_results, keyword_results, fuzzy_results) =
        tokio::join!(semantic_future, keyword_future, fuzzy_future);

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

    let fuzzy_results: Vec<SearchResult> = fuzzy_results
        .into_iter()
        .map(|r| SearchResult {
            file_path: r.file_path,
            content: r.content,
            score: r.score,
            start_line: r.start_line,
        })
        .collect();

    tracing::debug!(
        "Hybrid search: {} semantic, {} keyword, {} fuzzy results before merge",
        semantic_results.len(),
        keyword_results.len(),
        fuzzy_results.len()
    );

    // Merge and deduplicate results
    let (mut results, search_type) =
        merge_results(semantic_results, keyword_results, fuzzy_results);

    // Apply intent-based reranking
    rerank_results_with_intent(&mut results, project_path, intent);

    // Truncate to effective limit (increased for Debug intent)
    results.truncate(effective_limit);

    tracing::debug!(
        "Hybrid search: {} results after merge (type: {}, intent: {:?})",
        results.len(),
        search_type,
        intent,
    );

    Ok(HybridSearchResult {
        results,
        search_type,
        intent,
    })
}

// ============================================================================
// Result Formatting
// ============================================================================

/// Controls how much detail to include in formatted search results
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResultDetail {
    /// Truncate at 500 chars (legacy non-expand behavior)
    Compact,
    /// Full content for all results (legacy expand=true behavior)
    Full,
    /// Top N results full, rest skeletonized to signatures + docstrings
    Tiered,
}

/// Format search results for display
pub fn format_results(
    results: &[SearchResult],
    search_type: SearchType,
    project_path: Option<&str>,
    detail: ResultDetail,
) -> String {
    if results.is_empty() {
        return "No code matches found. Have you run 'index' yet?".to_string();
    }

    /// Number of top-ranked results that get full content in Tiered mode
    const TIERED_FULL_COUNT: usize = 2;

    let count = results.len();
    let noun = if count == 1 { "result" } else { "results" };
    let mut response = format!("{} {} ({} search):\n\n", count, noun, search_type);

    for (rank, result) in results.iter().enumerate() {
        let location = if result.start_line > 0 {
            format!("{}:{}", result.file_path, result.start_line)
        } else {
            result.file_path.clone()
        };
        response.push_str(&format!("## {} (score: {:.2})\n", location, result.score));

        let use_full = match detail {
            ResultDetail::Full => true,
            ResultDetail::Compact => false,
            ResultDetail::Tiered => rank < TIERED_FULL_COUNT,
        };

        let use_skeleton = matches!(detail, ResultDetail::Tiered) && rank >= TIERED_FULL_COUNT;

        let display_content = if use_full {
            if let Some((symbol_info, expanded)) =
                expand_context(&result.file_path, &result.content, project_path)
            {
                if let Some(info) = symbol_info {
                    response.push_str(&format!("{}\n", info));
                }
                if expanded.len() > 1500 {
                    format!("{}...\n[truncated]", truncate_at_boundary(&expanded, 1500))
                } else {
                    expanded
                }
            } else {
                result.content.clone()
            }
        } else if use_skeleton {
            skeletonize_content(&result.content)
        } else if result.content.len() > 500 {
            truncate(&result.content, 500)
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
        assert_eq!(format!("{}", SearchType::Fuzzy), "fuzzy");
    }

    #[test]
    fn test_search_type_equality() {
        assert_eq!(SearchType::Semantic, SearchType::Semantic);
        assert_eq!(SearchType::Keyword, SearchType::Keyword);
        assert_eq!(SearchType::Fuzzy, SearchType::Fuzzy);
        assert_ne!(SearchType::Semantic, SearchType::Keyword);
        assert_ne!(SearchType::Semantic, SearchType::Fuzzy);
        assert_ne!(SearchType::Keyword, SearchType::Fuzzy);
    }

    // ============================================================================
    // QueryIntent detection tests
    // ============================================================================

    #[test]
    fn test_detect_intent_documentation() {
        assert_eq!(
            detect_query_intent("docs for Database"),
            QueryIntent::Documentation
        );
        assert_eq!(
            detect_query_intent("documentation for API"),
            QueryIntent::Documentation
        );
        assert_eq!(
            detect_query_intent("what is SearchResult"),
            QueryIntent::Documentation
        );
        assert_eq!(
            detect_query_intent("explain the hybrid search"),
            QueryIntent::Documentation
        );
    }

    #[test]
    fn test_detect_intent_examples() {
        assert_eq!(
            detect_query_intent("example of using search"),
            QueryIntent::Examples
        );
        assert_eq!(
            detect_query_intent("usage of Database"),
            QueryIntent::Examples
        );
        assert_eq!(
            detect_query_intent("how to use embeddings"),
            QueryIntent::Examples
        );
        assert_eq!(
            detect_query_intent("where is the config"),
            QueryIntent::Examples
        );
        assert_eq!(
            detect_query_intent("who calls this function"),
            QueryIntent::Examples
        );
        assert_eq!(
            detect_query_intent("callers of semantic_search"),
            QueryIntent::Examples
        );
    }

    #[test]
    fn test_detect_intent_implementation() {
        assert_eq!(
            detect_query_intent("how does search work"),
            QueryIntent::Implementation
        );
        assert_eq!(
            detect_query_intent("implementation of caching"),
            QueryIntent::Implementation
        );
        assert_eq!(
            detect_query_intent("how is the score calculated"),
            QueryIntent::Implementation
        );
        assert_eq!(
            detect_query_intent("source of error handling"),
            QueryIntent::Implementation
        );
        assert_eq!(
            detect_query_intent("definition of SearchResult"),
            QueryIntent::Implementation
        );
    }

    #[test]
    fn test_detect_intent_debug() {
        assert_eq!(detect_query_intent("fix the login bug"), QueryIntent::Debug);
        assert_eq!(
            detect_query_intent("error handling in search"),
            QueryIntent::Debug
        );
        assert_eq!(
            detect_query_intent("why is this test failing"),
            QueryIntent::Debug
        );
        assert_eq!(
            detect_query_intent("debug the connection issue"),
            QueryIntent::Debug
        );
        assert_eq!(detect_query_intent("crash in indexer"), QueryIntent::Debug);
        assert_eq!(detect_query_intent("troubleshoot auth"), QueryIntent::Debug);
        assert_eq!(
            detect_query_intent("panic in database pool"),
            QueryIntent::Debug
        );
        assert_eq!(
            detect_query_intent("issue with search results"),
            QueryIntent::Debug
        );
        assert_eq!(
            detect_query_intent("broken search feature"),
            QueryIntent::Debug
        );
    }

    #[test]
    fn test_detect_intent_refactor() {
        assert_eq!(
            detect_query_intent("refactor the search module"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("restructure database layer"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("reorganize tool handlers"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("move function to separate module"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("rename the SearchResult struct"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("extract helper from hybrid_search"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("split this file into modules"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("merge these two functions"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("consolidate search backends"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("blast radius of changing pool"),
            QueryIntent::Refactor
        );
        assert_eq!(
            detect_query_intent("impact of changing the API"),
            QueryIntent::Refactor
        );
    }

    #[test]
    fn test_detect_intent_general() {
        assert_eq!(detect_query_intent("search code"), QueryIntent::General);
        assert_eq!(detect_query_intent("Database struct"), QueryIntent::General);
        assert_eq!(
            detect_query_intent("authentication logic"),
            QueryIntent::General
        );
    }

    // ============================================================================
    // merge_results tests
    // ============================================================================

    #[test]
    fn test_merge_results_empty() {
        let (results, search_type) = merge_results(vec![], vec![], vec![]);
        assert!(results.is_empty());
        assert_eq!(search_type, SearchType::Keyword);
    }

    #[test]
    fn test_merge_results_semantic_only() {
        let semantic = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main()".to_string(),
            score: 0.9,
            start_line: 1,
        }];
        let (results, search_type) = merge_results(semantic, vec![], vec![]);
        assert_eq!(results.len(), 1);
        assert_eq!(search_type, SearchType::Semantic);
    }

    #[test]
    fn test_merge_results_keyword_only() {
        let keyword = vec![SearchResult {
            file_path: "src/lib.rs".to_string(),
            content: "pub fn search()".to_string(),
            score: 0.8,
            start_line: 10,
        }];
        let (results, search_type) = merge_results(vec![], keyword, vec![]);
        assert_eq!(results.len(), 1);
        assert_eq!(search_type, SearchType::Keyword);
    }

    #[test]
    fn test_merge_results_deduplication() {
        let semantic = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main()".to_string(),
            score: 0.9,
            start_line: 1,
        }];
        let keyword = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main()".to_string(),
            score: 0.7,
            start_line: 1,
        }];
        let (results, _) = merge_results(semantic, keyword, vec![]);
        assert_eq!(results.len(), 1);
        // Should keep higher score
        assert_eq!(results[0].score, 0.9);
    }

    #[test]
    fn test_merge_results_fuzzy_only() {
        let fuzzy = vec![SearchResult {
            file_path: "src/fuzzy.rs".to_string(),
            content: "fuzzy match".to_string(),
            score: 0.75,
            start_line: 5,
        }];
        let (results, search_type) = merge_results(vec![], vec![], fuzzy);
        assert_eq!(results.len(), 1);
        assert_eq!(search_type, SearchType::Fuzzy);
    }

    #[test]
    fn test_merge_results_all_three_sources() {
        let semantic = vec![SearchResult {
            file_path: "src/semantic.rs".to_string(),
            content: "semantic match".to_string(),
            score: 0.95,
            start_line: 1,
        }];
        let keyword = vec![SearchResult {
            file_path: "src/keyword.rs".to_string(),
            content: "keyword match".to_string(),
            score: 0.80,
            start_line: 10,
        }];
        let fuzzy = vec![SearchResult {
            file_path: "src/fuzzy.rs".to_string(),
            content: "fuzzy match".to_string(),
            score: 0.70,
            start_line: 20,
        }];
        let (results, search_type) = merge_results(semantic, keyword, fuzzy);
        assert_eq!(results.len(), 3);
        assert_eq!(search_type, SearchType::Semantic);
        // Results should be sorted by score descending
        assert!(results[0].score >= results[1].score);
        assert!(results[1].score >= results[2].score);
    }

    #[test]
    fn test_merge_results_fuzzy_dedup_with_semantic() {
        let semantic = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main()".to_string(),
            score: 0.9,
            start_line: 1,
        }];
        let fuzzy = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main()".to_string(),
            score: 0.7,
            start_line: 1,
        }];
        let (results, _) = merge_results(semantic, vec![], fuzzy);
        assert_eq!(results.len(), 1);
        // Should keep higher score (semantic's 0.9)
        assert_eq!(results[0].score, 0.9);
    }

    #[test]
    fn test_merge_results_sorted_by_score() {
        let semantic = vec![SearchResult {
            file_path: "src/low.rs".to_string(),
            content: "low score".to_string(),
            score: 0.5,
            start_line: 1,
        }];
        let keyword = vec![SearchResult {
            file_path: "src/high.rs".to_string(),
            content: "high score".to_string(),
            score: 0.95,
            start_line: 1,
        }];
        let (results, _) = merge_results(semantic, keyword, vec![]);
        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
        assert_eq!(results[0].file_path, "src/high.rs");
    }

    // ============================================================================
    // Reranking tests for Debug and Refactor intents
    // ============================================================================

    #[test]
    fn test_rerank_debug_boosts_error_handling() {
        let mut results = vec![
            SearchResult {
                file_path: "src/clean.rs".to_string(),
                content: "fn clean_function() { let x = 1; }".to_string(),
                score: 0.80,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/errors.rs".to_string(),
                content: "fn handle() -> Result<(), Error> { bail!(\"fail\") }".to_string(),
                score: 0.80,
                start_line: 1,
            },
        ];
        rerank_results_with_intent(&mut results, None, QueryIntent::Debug);
        // The error-handling result should be boosted above the clean one
        assert_eq!(results[0].file_path, "src/errors.rs");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_rerank_debug_boosts_test_files() {
        let mut results = vec![
            SearchResult {
                file_path: "src/lib.rs".to_string(),
                content: "fn some_function() {}".to_string(),
                score: 0.80,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/test_search.rs".to_string(),
                content: "fn some_function() {}".to_string(),
                score: 0.80,
                start_line: 1,
            },
        ];
        rerank_results_with_intent(&mut results, None, QueryIntent::Debug);
        // Test file should be boosted for Debug intent
        assert_eq!(results[0].file_path, "src/test_search.rs");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_rerank_debug_boosts_recent_files() {
        use std::fs;
        use std::io::Write;

        // Create a temp file that will have a very recent mtime
        let dir = std::env::temp_dir().join("mira_test_debug_recency");
        let _ = fs::create_dir_all(&dir);
        let recent_file = dir.join("recent.rs");
        let mut f = fs::File::create(&recent_file).unwrap();
        writeln!(f, "fn recent() {{}}").unwrap();
        drop(f);

        let mut results = vec![
            SearchResult {
                file_path: "old_file.rs".to_string(),
                content: "fn old() {}".to_string(),
                score: 0.90,
                start_line: 1,
            },
            SearchResult {
                file_path: "recent.rs".to_string(),
                content: "fn recent() {}".to_string(),
                score: 0.80,
                start_line: 1,
            },
        ];

        // With project_path pointing to temp dir, only recent.rs resolves
        rerank_results_with_intent(
            &mut results,
            Some(dir.to_str().unwrap()),
            QueryIntent::Debug,
        );

        // The recent file should get the amplified 1.30 debug recency boost
        let recent_result = results.iter().find(|r| r.file_path == "recent.rs").unwrap();
        // 0.80 * 1.10 (complete symbol) * 1.30 (debug recency) = ~1.144
        assert!(
            recent_result.score > 1.0,
            "recent file should get strong debug recency boost, got {}",
            recent_result.score
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_rerank_refactor_boosts_public_api() {
        let mut results = vec![
            SearchResult {
                file_path: "src/internal.rs".to_string(),
                content: "fn private_helper() { do_stuff(); }".to_string(),
                score: 0.80,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/api.rs".to_string(),
                content: "pub fn search_code() -> Result<()> { Ok(()) }".to_string(),
                score: 0.80,
                start_line: 1,
            },
        ];
        rerank_results_with_intent(&mut results, None, QueryIntent::Refactor);
        // Public API should be boosted for Refactor intent
        assert_eq!(results[0].file_path, "src/api.rs");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_rerank_refactor_boosts_pub_struct_enum_trait() {
        let mut results = vec![
            SearchResult {
                file_path: "src/types.rs".to_string(),
                content: "pub struct Config { pub name: String }".to_string(),
                score: 0.70,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/other.rs".to_string(),
                content: "let config = Config::new();".to_string(),
                score: 0.75,
                start_line: 10,
            },
        ];
        rerank_results_with_intent(&mut results, None, QueryIntent::Refactor);
        // pub struct should be boosted above usage code
        assert_eq!(results[0].file_path, "src/types.rs");
    }

    #[test]
    fn test_rerank_refactor_boosts_impl_blocks() {
        let mut results = vec![
            SearchResult {
                file_path: "src/plain.rs".to_string(),
                content: "fn standalone() {}".to_string(),
                score: 0.80,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/methods.rs".to_string(),
                content: "impl SearchEngine { fn run(&self) {} }".to_string(),
                score: 0.80,
                start_line: 1,
            },
        ];
        rerank_results_with_intent(&mut results, None, QueryIntent::Refactor);
        // impl block should get boosted
        assert_eq!(results[0].file_path, "src/methods.rs");
        assert!(results[0].score > results[1].score);
    }

    // ============================================================================
    // format_results tests
    // ============================================================================

    #[test]
    fn test_format_results_empty() {
        let output = format_results(&[], SearchType::Semantic, None, ResultDetail::Compact);
        assert!(output.contains("No code matches found"));
        assert!(output.contains("index"));
    }

    #[test]
    fn test_format_results_basic() {
        let results = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            content: "fn main() { }".to_string(),
            score: 0.85,
            start_line: 1,
        }];
        let output = format_results(&results, SearchType::Semantic, None, ResultDetail::Compact);
        assert!(output.contains("1 result (semantic search)"));
        assert!(output.contains("src/main.rs:1"));
        assert!(output.contains("score: 0.85"));
        assert!(output.contains("fn main()"));
    }

    #[test]
    fn test_format_results_no_line_number() {
        let results = vec![SearchResult {
            file_path: "src/lib.rs".to_string(),
            content: "pub mod search;".to_string(),
            score: 0.75,
            start_line: 0,
        }];
        let output = format_results(&results, SearchType::Keyword, None, ResultDetail::Compact);
        assert!(output.contains("keyword search"));
        // Should not have line number when start_line is 0
        assert!(output.contains("## src/lib.rs ("));
        assert!(!output.contains("src/lib.rs:0"));
    }

    #[test]
    fn test_format_results_truncates_long_content() {
        let long_content = "x".repeat(600);
        let results = vec![SearchResult {
            file_path: "src/big.rs".to_string(),
            content: long_content,
            score: 0.9,
            start_line: 1,
        }];
        let output = format_results(&results, SearchType::Semantic, None, ResultDetail::Compact);
        assert!(output.contains("..."));
        // Output should be truncated, not contain all 600 chars
        assert!(output.len() < 700);
    }

    // ============================================================================
    // Tiered format_results tests
    // ============================================================================

    #[test]
    fn test_format_results_tiered_skeletonizes_lower_ranks() {
        let results = vec![
            SearchResult {
                file_path: "src/first.rs".to_string(),
                content: "/// First result doc\npub fn first() {\n    let x = 1;\n    let y = 2;\n}".to_string(),
                score: 0.95,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/second.rs".to_string(),
                content: "/// Second result doc\npub fn second() {\n    let a = 3;\n}".to_string(),
                score: 0.90,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/third.rs".to_string(),
                content: "/// Third result doc\npub fn third() {\n    let body_line = 42;\n    println!(\"hello\");\n}".to_string(),
                score: 0.80,
                start_line: 1,
            },
            SearchResult {
                file_path: "src/fourth.rs".to_string(),
                content: "/// Fourth result doc\npub fn fourth() {\n    let z = 99;\n}".to_string(),
                score: 0.70,
                start_line: 1,
            },
        ];
        let output = format_results(&results, SearchType::Semantic, None, ResultDetail::Tiered);

        // Results #1 and #2 should have full content (body lines present)
        assert!(output.contains("let x = 1"));
        assert!(output.contains("let a = 3"));

        // Results #3 and #4 should be skeletonized (body lines stripped)
        assert!(!output.contains("let body_line = 42"));
        assert!(!output.contains("let z = 99"));

        // But their signatures and docs should be preserved
        assert!(output.contains("/// Third result doc"));
        assert!(output.contains("pub fn third()"));
        assert!(output.contains("/// Fourth result doc"));
        assert!(output.contains("pub fn fourth()"));

        // Body omitted placeholder should appear for skeletonized results
        assert!(output.contains("// ... body omitted"));
    }
}
