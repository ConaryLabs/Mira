//! Error detection and similar fix loading

use regex::Regex;
use sqlx::sqlite::SqlitePool;
use std::collections::HashSet;
use std::sync::Arc;

use crate::core::SemanticSearch;
use super::types::{ChatMessage, SimilarFix};

/// Detect error patterns in recent messages
/// Returns a list of error strings that can be used to find similar past fixes
pub fn detect_error_patterns(messages: &[ChatMessage]) -> Vec<String> {
    let mut patterns: HashSet<String> = HashSet::new();

    // Common error pattern regexes
    let error_patterns = [
        // Rust errors
        r"error\[E\d+\]:\s*(.+)",
        r"cannot\s+(find|move|borrow|infer)\s+(.+)",
        r"the trait .+ is not implemented",
        r"no method named .+ found",
        r"mismatched types",
        // General errors
        r"(?i)error:\s*(.+)",
        r"(?i)failed:\s*(.+)",
        r"(?i)panic(ked)?:\s*(.+)",
        r"(?i)exception:\s*(.+)",
        // Stack traces
        r"at .+:\d+:\d+",
        // Build/test failures
        r"(?i)build failed",
        r"(?i)test failed",
        r"(?i)compilation error",
    ];

    // Compile regexes
    let compiled: Vec<Regex> = error_patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    // Check each message for error patterns
    for msg in messages {
        // Skip assistant messages for error detection (focus on user-reported errors)
        // But check both for debugging context
        for line in msg.content.lines().take(50) {
            for re in &compiled {
                if re.is_match(line) {
                    // Truncate long error lines
                    let error = if line.len() > 150 {
                        format!("{}...", &line[..150])
                    } else {
                        line.to_string()
                    };
                    patterns.insert(error);

                    // Limit patterns to avoid query bloat
                    if patterns.len() >= 10 {
                        return patterns.into_iter().collect();
                    }
                }
            }
        }

        // Also check for explicit error indicators in message content
        let lower = msg.content.to_lowercase();
        if lower.contains("not working")
            || lower.contains("doesn't work")
            || lower.contains("broken")
            || lower.contains("fix this")
            || lower.contains("getting an error")
        {
            // Mark as "implicit error" for context
            patterns.insert("__implicit_error__".to_string());
        }
    }

    patterns.into_iter().collect()
}

/// Load similar fixes from error_fixes table
/// Uses semantic search when available, falls back to text matching
pub async fn load_similar_fixes(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    error_patterns: &[String],
    limit: usize,
) -> Vec<SimilarFix> {
    if error_patterns.is_empty() {
        return Vec::new();
    }

    // Skip implicit error marker for search
    let searchable: Vec<&String> = error_patterns
        .iter()
        .filter(|p| *p != "__implicit_error__")
        .collect();

    if searchable.is_empty() {
        return Vec::new();
    }

    // Build a combined search query
    let combined_query = searchable.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");

    // Try semantic search first if available
    if semantic.is_available() {
        if let Ok(results) = semantic
            .search("mira_error_fixes", &combined_query, limit, None)
            .await
        {
            if !results.is_empty() {
                return results
                    .into_iter()
                    .map(|r| SimilarFix {
                        error_pattern: r.metadata
                            .get("error_pattern")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        fix_description: r.content,
                        score: r.score,
                    })
                    .collect();
            }
        }
    }

    // Fallback to SQL text matching
    let mut fixes: Vec<SimilarFix> = Vec::new();

    for pattern in searchable.iter().take(3) {
        let like_pattern = format!("%{}%", pattern);

        let rows = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT error_pattern, fix_description
            FROM error_fixes
            WHERE error_pattern LIKE $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(&like_pattern)
        .bind(limit as i64)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (error_pattern, fix_description) in rows {
            fixes.push(SimilarFix {
                error_pattern,
                fix_description,
                score: 0.5, // Default score for text match
            });
        }
    }

    fixes.truncate(limit);
    fixes
}
