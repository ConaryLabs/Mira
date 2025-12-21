//! Code index hints
//!
//! Provides relevant code symbols from the codebase based on query analysis.
//! Uses semantic search when available (Qdrant + Gemini), with SQL fallback.

use anyhow::Result;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use tracing::debug;

use super::{CodeIndexFileHint, CodeIndexSymbolHint, SessionManager};
use crate::core::COLLECTION_CODE;

impl SessionManager {
    /// Load code index hints for a query
    /// Uses semantic search when available, falls back to SQL term matching
    pub(super) async fn load_code_index_hints(&self, query: &str) -> Vec<CodeIndexFileHint> {
        // Try semantic search first
        if self.semantic.is_available() {
            match self.load_code_hints_semantic(query).await {
                Ok(hints) if !hints.is_empty() => {
                    debug!("Loaded {} code hints via semantic search", hints.len());
                    return hints;
                }
                Ok(_) => {
                    debug!("Semantic search returned no results, falling back to SQL");
                }
                Err(e) => {
                    debug!("Semantic code search failed: {}, falling back to SQL", e);
                }
            }
        }

        // Fallback to SQL term matching
        match self.load_code_hints_sql(query).await {
            Ok(hints) => {
                debug!("Loaded {} code hints via SQL", hints.len());
                hints
            }
            Err(e) => {
                debug!("Code index hints unavailable: {}", e);
                Vec::new()
            }
        }
    }

    /// Semantic code search using COLLECTION_CODE
    /// Returns conceptually related code based on natural language query
    async fn load_code_hints_semantic(&self, query: &str) -> Result<Vec<CodeIndexFileHint>> {
        use qdrant_client::qdrant::{Condition, Filter};

        // Filter to project path
        let filter = Filter::must([
            Condition::matches("file_path", format!("{}%", self.project_path))
        ]);

        let results = self
            .semantic
            .search(COLLECTION_CODE, query, 15, Some(filter))
            .await?;

        if results.is_empty() {
            return Ok(Vec::new());
        }

        // Group by file_path
        let mut files: HashMap<String, Vec<CodeIndexSymbolHint>> = HashMap::new();
        let mut seen: HashSet<(String, i64)> = HashSet::new();

        for result in results {
            let file_path = result
                .metadata
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if file_path.is_empty() {
                continue;
            }

            let name = result
                .metadata
                .get("symbol_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let start_line = result
                .metadata
                .get("start_line")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            // Dedup
            if seen.contains(&(name.clone(), start_line)) {
                continue;
            }
            seen.insert((name.clone(), start_line));

            let hint = CodeIndexSymbolHint {
                name,
                qualified_name: result
                    .metadata
                    .get("qualified_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                symbol_type: result
                    .metadata
                    .get("symbol_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                signature: result
                    .metadata
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                start_line,
                end_line: result
                    .metadata
                    .get("end_line")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(start_line),
            };

            files.entry(file_path).or_default().push(hint);
        }

        // Convert to ranked list
        let mut file_list: Vec<CodeIndexFileHint> = files
            .into_iter()
            .map(|(file_path, mut symbols)| {
                symbols.truncate(8);
                CodeIndexFileHint { file_path, symbols }
            })
            .collect();

        file_list.sort_by_key(|f| std::cmp::Reverse(f.symbols.len()));
        file_list.truncate(8);

        Ok(file_list)
    }

    /// SQL-based term matching fallback
    async fn load_code_hints_sql(&self, query: &str) -> Result<Vec<CodeIndexFileHint>> {
        let terms = extract_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        // Indexer stores absolute file paths; keep results scoped to the current project.
        let project_prefix = format!("{}%", self.project_path);

        let mut files: HashMap<String, Vec<CodeIndexSymbolHint>> = HashMap::new();
        let mut seen: HashMap<String, HashSet<(String, i64)>> = HashMap::new();

        // Pull a small number of hits per term; merge/dedup across terms.
        for term in terms.iter().take(6) {
            let like = format!("%{}%", term);

            let rows = sqlx::query(
                r#"
                SELECT file_path, name, qualified_name, symbol_type, signature, start_line, end_line
                FROM code_symbols
                WHERE file_path LIKE $1
                  AND (name LIKE $2 OR qualified_name LIKE $2)
                ORDER BY analyzed_at DESC
                LIMIT 50
                "#,
            )
            .bind(&project_prefix)
            .bind(&like)
            .fetch_all(&self.db)
            .await?;

            for row in rows {
                let file_path: String = row.get("file_path");
                let name: String = row.get("name");
                let start_line: i64 = row.get("start_line");

                let entry_seen = seen.entry(file_path.clone()).or_default();
                if entry_seen.contains(&(name.clone(), start_line)) {
                    continue;
                }
                entry_seen.insert((name.clone(), start_line));

                let hint = CodeIndexSymbolHint {
                    name,
                    qualified_name: row.get("qualified_name"),
                    symbol_type: row.get("symbol_type"),
                    signature: row.get("signature"),
                    start_line,
                    end_line: row.get("end_line"),
                };

                files.entry(file_path).or_default().push(hint);
            }

            // Stop early if we already have enough breadth.
            if files.len() >= 8 {
                break;
            }
        }

        if files.is_empty() {
            return Ok(Vec::new());
        }

        // Convert to a ranked list: most hits per file first.
        let mut file_list: Vec<CodeIndexFileHint> = files
            .into_iter()
            .map(|(file_path, mut symbols)| {
                symbols.truncate(8);
                CodeIndexFileHint { file_path, symbols }
            })
            .collect();

        file_list.sort_by_key(|f| std::cmp::Reverse(f.symbols.len()));
        file_list.truncate(6);

        Ok(file_list)
    }
}

/// Extract search terms from a query string
pub(super) fn extract_terms(query: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(query.len());
    for c in query.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cleaned.push(c.to_ascii_lowercase());
        } else {
            cleaned.push(' ');
        }
    }

    // Super light filtering. We just want a handful of useful tokens.
    let noise: HashSet<&'static str> = [
        "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "with", "without",
        "this", "that", "these", "those", "it", "is", "are", "be", "was", "were",
        "use", "using", "used", "make", "makes", "making", "do", "does", "did",
        "how", "what", "where", "why", "when",
        "file", "files", "function", "functions", "struct", "structs", "class", "classes",
        "module", "crate", "rust", "code",
    ]
    .into_iter()
    .collect();

    let mut uniq: HashSet<String> = HashSet::new();
    for raw in cleaned.split_whitespace() {
        if raw.len() < 3 {
            continue;
        }
        if noise.contains(raw) {
            continue;
        }
        if raw.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        uniq.insert(raw.to_string());
    }

    let mut terms: Vec<String> = uniq.into_iter().collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    terms.truncate(8);
    terms
}
