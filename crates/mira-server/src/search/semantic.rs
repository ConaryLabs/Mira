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

/// Rerank with intent-specific boosts
fn rerank_results_with_intent(
    results: &mut [SearchResult],
    project_path: Option<&str>,
    intent: QueryIntent,
) {
    use std::time::{SystemTime, Duration};

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

/// Parse symbol name and kind from chunk header
/// Headers look like: "// function foo", "// function foo: sig", "// function foo (continued)"
fn parse_symbol_header(chunk_content: &str) -> Option<(String, String)> {
    let first_line = chunk_content.lines().next()?;
    if !first_line.starts_with("// ") {
        return None;
    }

    let rest = first_line.strip_prefix("// ")?;

    // Skip "module-level code" - no symbol to look up
    if rest.starts_with("module") {
        return None;
    }

    // Split on first space to get kind
    let (kind, remainder) = rest.split_once(' ')?;

    // Get name: everything before ":" or " (continued)" or end of string
    let name = if let Some(idx) = remainder.find(':') {
        &remainder[..idx]
    } else if let Some(idx) = remainder.find(" (continued)") {
        &remainder[..idx]
    } else {
        remainder
    };

    Some((kind.to_string(), name.trim().to_string()))
}

/// Look up symbol bounds from code_symbols table
fn lookup_symbol_bounds(
    db: &crate::db::Database,
    project_id: Option<i64>,
    file_path: &str,
    symbol_name: &str,
) -> Option<(u32, u32)> {
    let conn = db.conn();
    let query = if project_id.is_some() {
        "SELECT start_line, end_line FROM code_symbols
         WHERE project_id = ?1 AND file_path = ?2 AND name = ?3
         LIMIT 1"
    } else {
        "SELECT start_line, end_line FROM code_symbols
         WHERE file_path = ?1 AND name = ?2
         LIMIT 1"
    };

    let result: Option<(u32, u32)> = if let Some(pid) = project_id {
        conn.query_row(query, rusqlite::params![pid, file_path, symbol_name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .ok()
    } else {
        conn.query_row(query, rusqlite::params![file_path, symbol_name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .ok()
    };

    result
}

/// Expand search result to full symbol using code_symbols table
pub fn expand_context(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
) -> Option<(Option<String>, String)> {
    expand_context_with_db(file_path, chunk_content, project_path, None, None)
}

/// Expand search result to full symbol using code_symbols table (with DB access)
pub fn expand_context_with_db(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
    db: Option<&crate::db::Database>,
    project_id: Option<i64>,
) -> Option<(Option<String>, String)> {
    // Extract symbol info from header
    let symbol_info = if chunk_content.starts_with("// ") {
        chunk_content.lines().next().map(|s| s.to_string())
    } else {
        None
    };

    // Try to expand using symbol bounds from DB
    if let (Some(db), Some(proj_path)) = (db, project_path) {
        if let Some((kind, name)) = parse_symbol_header(chunk_content) {
            if let Some((start_line, end_line)) = lookup_symbol_bounds(db, project_id, file_path, &name) {
                let full_path = Path::new(proj_path).join(file_path);
                if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                    let all_lines: Vec<&str> = file_content.lines().collect();

                    // Convert 1-indexed lines to 0-indexed
                    let start = (start_line.saturating_sub(1)) as usize;
                    let end = std::cmp::min(end_line as usize, all_lines.len());

                    if start < all_lines.len() {
                        let full_symbol = all_lines[start..end].join("\n");
                        let header = format!("// {} {} (lines {}-{})", kind, name, start_line, end_line);
                        return Some((Some(header), full_symbol));
                    }
                }
            }
        }
    }

    // Fallback: use original +-5 line approach
    if let Some(proj_path) = project_path {
        let full_path = Path::new(proj_path).join(file_path);
        if let Ok(file_content) = std::fs::read_to_string(&full_path) {
            let search_content = if chunk_content.starts_with("// ") {
                chunk_content.lines().skip(1).collect::<Vec<_>>().join("\n")
            } else {
                chunk_content.to_string()
            };

            if let Some(pos) = file_content.find(&search_content) {
                let lines_before = file_content[..pos].matches('\n').count();
                let all_lines: Vec<&str> = file_content.lines().collect();
                let match_lines = search_content.matches('\n').count() + 1;

                let start_line = lines_before.saturating_sub(5);
                let end_line = std::cmp::min(lines_before + match_lines + 5, all_lines.len());

                let context_code = all_lines[start_line..end_line].join("\n");
                return Some((symbol_info, context_code));
            }
        }
    }

    // Final fallback: return chunk as-is
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

// ============================================================================
// Cross-Reference Search (Call Graph)
// ============================================================================

/// Result from cross-reference search
#[derive(Debug, Clone)]
pub struct CrossRefResult {
    /// The symbol being referenced
    pub symbol_name: String,
    /// File containing the symbol
    pub file_path: String,
    /// The relationship type
    pub ref_type: CrossRefType,
    /// Number of calls (for callers)
    pub call_count: i32,
}

/// Type of cross-reference relationship
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrossRefType {
    /// Functions that call the target
    Caller,
    /// Functions called by the target
    Callee,
}

impl std::fmt::Display for CrossRefType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrossRefType::Caller => write!(f, "caller"),
            CrossRefType::Callee => write!(f, "callee"),
        }
    }
}

/// Extract target symbol name from caller/callee queries
/// Patterns: "who calls X", "callers of X", "what calls X"
///           "what does X call", "callees of X", "functions called by X"
fn extract_crossref_target(query: &str) -> Option<(String, CrossRefType)> {
    let q = query.to_lowercase();

    // Caller patterns
    for pattern in ["who calls ", "callers of ", "what calls ", "references to "] {
        if let Some(rest) = q.strip_prefix(pattern) {
            let name = rest.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Caller));
            }
        }
    }

    // Also check for pattern in the middle: "find callers of X"
    for pattern in [" callers of ", " who calls ", " what calls "] {
        if let Some(idx) = q.find(pattern) {
            let rest = &q[idx + pattern.len()..];
            let name = rest.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                return Some((name.to_string(), CrossRefType::Caller));
            }
        }
    }

    // Callee patterns
    for pattern in ["what does ", "functions called by ", "callees of "] {
        if let Some(rest) = q.strip_prefix(pattern) {
            // For "what does X call" - extract X
            let name = rest.split_whitespace().next()?;
            let name = name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() && q.contains(" call") {
                return Some((name.to_string(), CrossRefType::Callee));
            }
        }
    }

    None
}

/// Find functions that call a given symbol
pub fn find_callers(
    db: &crate::db::Database,
    project_id: Option<i64>,
    target_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    let conn = db.conn();

    // Find all call_graph entries where callee_name matches target
    // Join with code_symbols to get caller details
    let query = if project_id.is_some() {
        "SELECT cs.name, cs.file_path, cg.call_count
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1 AND cs.project_id = ?2
         ORDER BY cg.call_count DESC
         LIMIT ?3"
    } else {
        "SELECT cs.name, cs.file_path, cg.call_count
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cg.callee_name = ?1
         ORDER BY cg.call_count DESC
         LIMIT ?2"
    };

    let results: Vec<CrossRefResult> = if let Some(pid) = project_id {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![target_name, pid, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Caller,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    } else {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![target_name, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Caller,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };

    results
}

/// Check if a function name is a stdlib/utility call that should be filtered
fn is_stdlib_call(name: &str) -> bool {
    // Common Rust std methods and traits
    const STDLIB_NAMES: &[&str] = &[
        // Iterator/collection methods
        "map", "filter", "collect", "iter", "into_iter", "for_each", "fold", "reduce",
        "find", "any", "all", "count", "take", "skip", "chain", "zip", "enumerate",
        "filter_map", "flat_map", "flatten", "peekable", "rev", "cycle",
        // Option/Result methods
        "unwrap", "unwrap_or", "unwrap_or_else", "unwrap_or_default", "expect",
        "ok", "err", "is_some", "is_none", "is_ok", "is_err", "ok_or", "ok_or_else",
        "map_err", "and_then", "or_else", "transpose", "as_ref", "as_mut",
        // Common traits/constructors
        "new", "default", "clone", "to_string", "to_owned", "into", "from",
        "as_str", "as_bytes", "as_slice", "to_vec", "push", "pop", "insert", "remove",
        "get", "get_mut", "contains", "len", "is_empty", "clear", "extend",
        // Result/Option constructors
        "Ok", "Err", "Some", "None",
        // Formatting
        "format", "write", "writeln", "print", "println", "eprint", "eprintln",
        // Logging (without prefix)
        "debug", "info", "warn", "error", "trace",
        // Common string methods
        "split", "join", "trim", "replace", "starts_with", "ends_with", "contains",
        "to_lowercase", "to_uppercase", "parse", "chars", "bytes", "lines",
        // Sync primitives
        "lock", "read", "write", "try_lock", "try_read", "try_write",
        // Async
        "await", "poll", "spawn", "block_on",
        // Math/comparison
        "max", "min", "abs", "cmp", "partial_cmp", "eq", "ne", "lt", "le", "gt", "ge",
        // Database/connection
        "conn", "connection", "execute", "query", "prepare", "query_row", "query_map",
        // Misc
        "drop", "take", "swap", "mem", "ptr", "Box", "Rc", "Arc", "Vec", "String",
        "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque",
    ];

    // Check exact match
    if STDLIB_NAMES.contains(&name) {
        return true;
    }

    // Check prefixes (logging crates, std types, etc.)
    let prefixes = [
        "tracing::", "log::", "std::", "core::",
        "Vec::", "String::", "HashMap::", "HashSet::", "BTreeMap::", "BTreeSet::",
        "Option::", "Result::", "Box::", "Rc::", "Arc::", "Cell::", "RefCell::",
        "Mutex::", "RwLock::", "Path::", "PathBuf::", "OsStr::", "OsString::",
    ];
    for prefix in prefixes {
        if name.starts_with(prefix) {
            return true;
        }
    }

    false
}

/// Find functions called by a given symbol
pub fn find_callees(
    db: &crate::db::Database,
    project_id: Option<i64>,
    caller_name: &str,
    limit: usize,
) -> Vec<CrossRefResult> {
    let conn = db.conn();

    // Find the caller symbol(s), then get all their callees
    let query = if project_id.is_some() {
        "SELECT cg.callee_name, cs.file_path, COUNT(*) as cnt
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cs.name = ?1 AND cs.project_id = ?2
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?3"
    } else {
        "SELECT cg.callee_name, cs.file_path, COUNT(*) as cnt
         FROM call_graph cg
         JOIN code_symbols cs ON cg.caller_id = cs.id
         WHERE cs.name = ?1
         GROUP BY cg.callee_name
         ORDER BY cnt DESC
         LIMIT ?2"
    };

    let results: Vec<CrossRefResult> = if let Some(pid) = project_id {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![caller_name, pid, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Callee,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    } else {
        conn.prepare(query)
            .and_then(|mut stmt| {
                stmt.query_map(params![caller_name, limit as i64], |row| {
                    Ok(CrossRefResult {
                        symbol_name: row.get(0)?,
                        file_path: row.get(1)?,
                        ref_type: CrossRefType::Callee,
                        call_count: row.get::<_, i32>(2).unwrap_or(1),
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    };

    // Filter out stdlib/utility calls
    results.into_iter()
        .filter(|r| !is_stdlib_call(&r.symbol_name))
        .collect()
}

/// Cross-reference search: find callers or callees based on query
/// Returns None if query doesn't match cross-reference patterns
pub fn crossref_search(
    db: &crate::db::Database,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
) -> Option<(String, CrossRefType, Vec<CrossRefResult>)> {
    tracing::debug!(query = %query, "crossref_search: checking query");
    let (target, ref_type) = extract_crossref_target(query)?;
    tracing::info!(target = %target, ref_type = ?ref_type, "crossref_search: pattern matched");

    let results = match ref_type {
        CrossRefType::Caller => find_callers(db, project_id, &target, limit),
        CrossRefType::Callee => find_callees(db, project_id, &target, limit),
    };

    Some((target, ref_type, results))
}

/// Format cross-reference results for display
pub fn format_crossref_results(
    target: &str,
    ref_type: CrossRefType,
    results: &[CrossRefResult],
) -> String {
    if results.is_empty() {
        return match ref_type {
            CrossRefType::Caller => format!("No callers found for `{}`.", target),
            CrossRefType::Callee => format!("No callees found for `{}`.", target),
        };
    }

    let header = match ref_type {
        CrossRefType::Caller => format!("Functions that call `{}`:\n\n", target),
        CrossRefType::Callee => format!("Functions called by `{}`:\n\n", target),
    };

    let mut response = header;

    for (i, result) in results.iter().enumerate() {
        response.push_str(&format!(
            "{}. `{}` in {} ({}x)\n",
            i + 1,
            result.symbol_name,
            result.file_path,
            result.call_count
        ));
    }

    response
}
