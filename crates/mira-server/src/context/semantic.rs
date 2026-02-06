// crates/mira-server/src/context/semantic.rs
// Semantic context injection using embeddings search

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::search::hybrid_search;
use crate::utils::truncate_at_boundary;
use std::path::Path;
use std::sync::Arc;

pub struct SemanticInjector {
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    fuzzy: Option<Arc<FuzzyCache>>,
}

impl SemanticInjector {
    pub fn new(
        pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
        fuzzy: Option<Arc<FuzzyCache>>,
    ) -> Self {
        Self {
            pool,
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

        // Perform hybrid search (falls back to keyword search if embeddings is None)
        let result = hybrid_search(
            &self.pool,
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

                // Format results as context (useful but concise)
                let mut context = String::new();
                context.push_str("Relevant code:\n");

                for (i, search_result) in hybrid_result.results.iter().enumerate() {
                    if i > 0 {
                        context.push('\n');
                    }
                    // Extract filename from path
                    let filename = Path::new(&search_result.file_path)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&search_result.file_path);
                    // Truncate content appropriately
                    let content = truncate_at_boundary(&search_result.content, 200);
                    context.push_str(&format!("{}. {}:\n```\n{}\n```", i + 1, filename, content));
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
