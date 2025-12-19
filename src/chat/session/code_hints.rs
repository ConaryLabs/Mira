//! Code index hints
//!
//! Provides relevant code symbols from the codebase based on query analysis.

use anyhow::Result;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use tracing::debug;

use super::{CodeIndexFileHint, CodeIndexSymbolHint, SessionManager};

impl SessionManager {
    /// Load code index hints for a query
    pub(super) async fn load_code_index_hints(&self, query: &str) -> Vec<CodeIndexFileHint> {
        match self.load_code_index_hints_inner(query).await {
            Ok(hints) => hints,
            Err(e) => {
                // This is an optional enhancement. If the DB doesn't have the code tables,
                // we just skip it.
                debug!("Code index hints unavailable: {}", e);
                Vec::new()
            }
        }
    }

    async fn load_code_index_hints_inner(&self, query: &str) -> Result<Vec<CodeIndexFileHint>> {
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
