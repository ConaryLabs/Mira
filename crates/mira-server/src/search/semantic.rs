// crates/mira-server/src/search/semantic.rs
// Semantic code search with hybrid fallback

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::search::keyword::keyword_search;
use crate::search::utils::{distance_to_score, embedding_to_bytes};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;

/// Search result with file path, content, and similarity score
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_path: String,
    pub content: String,
    pub score: f32,
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
            "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?2) as distance
             FROM vec_code
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY distance
             LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;

    let results: Vec<SearchResult> = stmt
        .query_map(params![project_id, embedding_bytes, limit as i64], |row| {
            Ok(SearchResult {
                file_path: row.get(0)?,
                content: row.get(1)?,
                score: distance_to_score(row.get(2)?),
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

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
            return Ok(HybridSearchResult {
                results: keyword_results
                    .into_iter()
                    .map(|(file_path, content, score)| SearchResult {
                        file_path,
                        content,
                        score,
                    })
                    .collect(),
                search_type: SearchType::Keyword,
            });
        }
    }

    // Return semantic results (even if low quality, better than nothing)
    Ok(HybridSearchResult {
        results: semantic_results,
        search_type: SearchType::Semantic,
    })
}

/// Expand search result with surrounding context from the file
pub fn expand_context(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
) -> Option<(Option<String>, String)> {
    // Extract symbol info from header comment if present
    let symbol_info = if chunk_content.starts_with("// ") {
        chunk_content.lines().next().map(|s| s.to_string())
    } else {
        None
    };

    // Try to read full file and find the matching section
    if let Some(proj_path) = project_path {
        let full_path = Path::new(proj_path).join(file_path);
        if let Ok(file_content) = std::fs::read_to_string(&full_path) {
            // Strip the header comment if present for searching
            let search_content = if chunk_content.starts_with("// ") {
                chunk_content.lines().skip(1).collect::<Vec<_>>().join("\n")
            } else {
                chunk_content.to_string()
            };

            // Find the position in the file
            if let Some(pos) = file_content.find(&search_content) {
                let lines_before = file_content[..pos].matches('\n').count();
                let all_lines: Vec<&str> = file_content.lines().collect();
                let match_lines = search_content.matches('\n').count() + 1;

                // Get surrounding context (5 lines before and after)
                let start_line = lines_before.saturating_sub(5);
                let end_line = std::cmp::min(lines_before + match_lines + 5, all_lines.len());

                let context_code: String = all_lines[start_line..end_line].join("\n");
                return Some((symbol_info, context_code));
            }
        }
    }

    // Fallback: return chunk as-is
    Some((symbol_info, chunk_content.to_string()))
}

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
        response.push_str(&format!(
            "## {} (score: {:.2})\n",
            result.file_path, result.score
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
        } else {
            if result.content.len() > 500 {
                format!("{}...", &result.content[..500])
            } else {
                result.content.clone()
            }
        };

        response.push_str(&format!("```\n{}\n```\n\n", display_content));
    }

    response
}
