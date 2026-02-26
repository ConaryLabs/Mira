// crates/mira-server/src/context/semantic.rs
// Semantic context injection using embeddings search

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::search::hybrid_search;
use std::path::Path;
use std::sync::Arc;

pub struct SemanticInjector {
    pool: Arc<DatabasePool>,
    code_pool: Option<Arc<DatabasePool>>,
    embeddings: Option<Arc<EmbeddingClient>>,
    fuzzy: Option<Arc<FuzzyCache>>,
}

impl SemanticInjector {
    pub fn new(
        pool: Arc<DatabasePool>,
        code_pool: Option<Arc<DatabasePool>>,
        embeddings: Option<Arc<EmbeddingClient>>,
        fuzzy: Option<Arc<FuzzyCache>>,
    ) -> Self {
        Self {
            pool,
            code_pool,
            embeddings,
            fuzzy,
        }
    }

    /// Inject relevant context based on semantic similarity to user message
    pub async fn inject_context(
        &self,
        user_message: &str,
        session_id: &str,
        project_id: Option<i64>,
        project_path: Option<&str>,
    ) -> String {
        // For now, ignore session_id (could be used for session-specific memories later)
        let _ = session_id;

        // Use code_pool for hybrid search (vec_code/code_fts live in mira-code.db),
        // falling back to main pool for backward compatibility
        let search_pool = self.code_pool.as_ref().unwrap_or(&self.pool);
        let result = hybrid_search(
            search_pool,
            self.embeddings.as_ref(),
            self.fuzzy.as_ref(),
            user_message,
            project_id,
            project_path,
            3, // limit to 3 results for context injection - useful but not excessive
        )
        .await;

        match result {
            Ok(hybrid_result) => {
                if hybrid_result.results.is_empty() {
                    return String::new();
                }

                // Format results as relationship summaries (symbols in the relevant file range)
                let mut context = String::new();
                context.push_str("Relevant code:\n");

                // Collect file paths and start lines for symbol lookup
                let chunk_info: Vec<(String, i64)> = hybrid_result
                    .results
                    .iter()
                    .map(|r| (r.file_path.clone(), r.start_line as i64))
                    .collect();

                // Look up symbols near each chunk's location
                let symbols_by_file: Vec<Vec<(String, String, i64)>> = search_pool
                    .run(move |conn| {
                        let mut result = Vec::new();
                        for (file_path, start_line) in &chunk_info {
                            let syms = conn
                                .prepare(
                                    "SELECT name, symbol_type, start_line FROM code_symbols
                                     WHERE file_path = ?1
                                     AND start_line >= ?2 - 5 AND start_line <= ?2 + 200
                                     ORDER BY start_line
                                     LIMIT 8",
                                )
                                .and_then(|mut stmt| {
                                    stmt.query_map(
                                        rusqlite::params![file_path, start_line],
                                        |row| {
                                            Ok((
                                                row.get::<_, String>(0)?,
                                                row.get::<_, String>(1)?,
                                                row.get::<_, i64>(2)?,
                                            ))
                                        },
                                    )
                                    .map(|rows| {
                                        rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
                                    })
                                })
                                .unwrap_or_default();
                            result.push(syms);
                        }
                        Ok::<_, crate::error::MiraError>(result)
                    })
                    .await
                    .unwrap_or_default();

                for (i, search_result) in hybrid_result.results.iter().enumerate() {
                    if i > 0 {
                        context.push('\n');
                    }
                    let filename = Path::new(&search_result.file_path)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&search_result.file_path);

                    let syms = symbols_by_file.get(i).cloned().unwrap_or_default();
                    if syms.is_empty() {
                        // Fallback: show file:line with no symbol info
                        context.push_str(&format!(
                            "{}. {}:{}:\n```\n(no symbols indexed)\n```",
                            i + 1,
                            filename,
                            search_result.start_line
                        ));
                    } else {
                        let summary: Vec<String> = syms
                            .iter()
                            .map(|(name, kind, line)| format!("{}:{}({})", name, kind, line))
                            .collect();
                        context.push_str(&format!(
                            "{}. {}:\n```\n{}\n```",
                            i + 1,
                            filename,
                            summary.join(", ")
                        ));
                    }
                }

                context
            }
            Err(e) => {
                tracing::warn!("SemanticInjector error: {}", e);
                String::new()
            }
        }
    }
}
