// src/memory/features/code_intelligence/mod.rs

pub mod invalidation;
pub mod javascript_parser;
pub mod parser;
pub mod storage;
pub mod types;
pub mod typescript_parser;

pub use javascript_parser::JavaScriptParser;
pub use parser::RustParser;
pub use storage::{CodeIntelligenceStorage, RepoStats};
pub use types::*;
pub use typescript_parser::TypeScriptParser;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::embeddings::EmbeddingHead;
use crate::llm::provider::OpenAiEmbeddings;
use crate::memory::core::types::MemoryEntry;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;

#[derive(Clone)]
pub struct CodeIntelligenceService {
    storage: Arc<CodeIntelligenceStorage>,
    multi_store: Arc<QdrantMultiStore>,
    embedding_client: Arc<OpenAiEmbeddings>,
    pool: SqlitePool,
    rust_parser: RustParser,
    typescript_parser: TypeScriptParser,
    javascript_parser: JavaScriptParser,
}

impl CodeIntelligenceService {
    pub fn new(
        pool: SqlitePool,
        multi_store: Arc<QdrantMultiStore>,
        embedding_client: Arc<OpenAiEmbeddings>,
    ) -> Self {
        Self {
            storage: Arc::new(CodeIntelligenceStorage::new(pool.clone())),
            multi_store,
            embedding_client,
            pool,
            rust_parser: RustParser::new(),
            typescript_parser: TypeScriptParser::new(),
            javascript_parser: JavaScriptParser::new(),
        }
    }

    /// Invalidate all embeddings for a file before re-analyzing
    pub async fn invalidate_file(&self, file_id: i64) -> Result<u64> {
        invalidation::invalidate_file_embeddings(&self.pool, &self.multi_store, file_id).await
    }

    /// Invalidate all embeddings for an entire project
    pub async fn invalidate_project(&self, project_id: &str) -> Result<u64> {
        invalidation::invalidate_project_embeddings(&self.pool, &self.multi_store, project_id).await
    }

    /// Embed all code elements for a file into Qdrant
    pub async fn embed_code_elements(&self, file_id: i64, project_id: &str) -> Result<usize> {
        info!("Embedding code elements for file_id: {}", file_id);

        // Query database for elements with their IDs
        let rows = sqlx::query!(
            r#"
            SELECT id, language, element_type, name, full_path, content, visibility
            FROM code_elements
            WHERE file_id = ?
            ORDER BY start_line
            "#,
            file_id
        )
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            debug!("No code elements to embed for file_id {}", file_id);
            return Ok(0);
        }

        let mut embedded_count = 0;

        for row in rows {
            // Unwrap Option<i64> from query result
            let element_id = match row.id {
                Some(id) => id,
                None => continue,
            };

            let language = row.language;
            let element_type = row.element_type;
            let name = row.name;
            let full_path = row.full_path;
            let content = row.content;

            // Create embedding content: "type name: content"
            let embed_text = format!("{} {}: {}", element_type, name, content);

            // Generate embedding
            let embedding = match self.embedding_client.embed(&embed_text).await {
                Ok(emb) => emb,
                Err(e) => {
                    warn!("Failed to embed code element {}: {}", element_id, e);
                    continue;
                }
            };

            // Create MemoryEntry for Qdrant storage
            let entry = MemoryEntry {
                id: Some(element_id),
                session_id: format!("code:{}", project_id),
                role: "code".to_string(),
                content: content.clone(),
                timestamp: chrono::Utc::now(),
                embedding: Some(embedding),

                // Code-specific metadata
                contains_code: Some(true),
                programming_lang: Some(language.clone()),

                // Store file_id for invalidation
                tags: Some(vec![
                    format!("file_id:{}", file_id),
                    format!("element_type:{}", element_type),
                    format!("language:{}", language),
                    format!("name:{}", name),
                    format!("path:{}", full_path),
                ]),

                // All other fields set to None
                response_id: None,
                parent_id: None,
                mood: None,
                intensity: None,
                salience: None,
                original_salience: None,
                intent: None,
                topics: None,
                summary: None,
                relationship_impact: None,
                language: None,
                analyzed_at: None,
                analysis_version: None,
                routed_to_heads: None,
                last_recalled: None,
                recall_count: None,
                contains_error: None,
                error_type: None,
                error_severity: None,
                error_file: None,
                model_version: None,
                prompt_tokens: None,
                completion_tokens: None,
                reasoning_tokens: None,
                total_tokens: None,
                latency_ms: None,
                generation_time_ms: None,
                finish_reason: None,
                tool_calls: None,
                temperature: None,
                max_tokens: None,
                embedding_heads: None,
                qdrant_point_ids: None,
            };

            // Store in code collection (element_id is used as point_id)
            match self.multi_store.save(EmbeddingHead::Code, &entry).await {
                Ok(_) => {
                    embedded_count += 1;
                    debug!(
                        "Embedded code element {} ({}::{})",
                        element_id, element_type, name
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to store embedding for element {}: {}",
                        element_id, e
                    );
                }
            }
        }

        if embedded_count > 0 {
            info!(
                "Embedded {} code elements for file_id {}",
                embedded_count, file_id
            );
        }

        Ok(embedded_count)
    }

    /// Semantic search for code elements
    /// Returns MemoryEntry objects formatted for context inclusion
    pub async fn search_code(
        &self,
        query: &str,
        project_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        debug!(
            "Searching code for project {} with query: {}",
            project_id, query
        );

        // Embed the query
        let query_embedding = self.embedding_client.embed(query).await?;

        // Search code collection - returns Vec<MemoryEntry> directly
        let session_id = format!("code:{}", project_id);
        let results = self
            .multi_store
            .search(EmbeddingHead::Code, &session_id, &query_embedding, limit)
            .await?;

        debug!("Found {} code search results", results.len());

        // Format entries for context display
        let entries: Vec<MemoryEntry> = results
            .into_iter()
            .filter_map(|mut entry| {
                // Extract metadata from tags
                let tags = entry.tags.as_ref()?;

                let element_type = tags
                    .iter()
                    .find(|t| t.starts_with("element_type:"))
                    .and_then(|t| t.strip_prefix("element_type:"))
                    .unwrap_or("");

                let name = tags
                    .iter()
                    .find(|t| t.starts_with("name:"))
                    .and_then(|t| t.strip_prefix("name:"))
                    .unwrap_or("");

                let path = tags
                    .iter()
                    .find(|t| t.starts_with("path:"))
                    .and_then(|t| t.strip_prefix("path:"))
                    .unwrap_or("");

                // Format content as "file_path: element_name (type)"
                // This is what the prompt builder expects
                let formatted_content = if !path.is_empty() && !name.is_empty() {
                    format!("{}: {} ({})", path, name, element_type)
                } else {
                    // Fallback to original content
                    entry.content.clone()
                };

                // Update entry with formatted content
                entry.content = formatted_content;
                entry.session_id = project_id.to_string();

                Some(entry)
            })
            .collect();

        debug!("Found {} relevant code elements", entries.len());
        Ok(entries)
    }

    pub async fn analyze_and_store_file(
        &self,
        file_id: i64,
        content: &str,
        file_path: &str,
        language: &str,
    ) -> Result<FileAnalysisResult> {
        let analysis = match language {
            "rust" => {
                if self.rust_parser.can_parse(content, Some(file_path)) {
                    self.rust_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse Rust file: {}", file_path));
                }
            }
            "typescript" => {
                if self.typescript_parser.can_parse(content, Some(file_path)) {
                    self.typescript_parser
                        .parse_file(content, file_path)
                        .await?
                } else {
                    return Err(anyhow::anyhow!(
                        "Cannot parse TypeScript file: {}",
                        file_path
                    ));
                }
            }
            "javascript" => {
                if self.javascript_parser.can_parse(content, Some(file_path)) {
                    self.javascript_parser
                        .parse_file(content, file_path)
                        .await?
                } else {
                    return Err(anyhow::anyhow!(
                        "Cannot parse JavaScript file: {}",
                        file_path
                    ));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported language: {}", language));
            }
        };

        self.storage
            .store_file_analysis(file_id, language, &analysis)
            .await?;

        Ok(FileAnalysisResult {
            file_id,
            language: language.to_string(),
            elements_count: analysis.elements.len(),
            complexity_score: analysis.complexity_score,
            quality_issues_count: analysis.quality_issues.len(),
            test_coverage: if analysis.test_count > 0 {
                analysis.test_count as f64 / analysis.elements.len() as f64
            } else {
                0.0
            },
            doc_coverage: analysis.doc_coverage,
        })
    }

    pub async fn analyze_and_store_with_project(
        &self,
        file_id: i64,
        content: &str,
        file_path: &str,
        language: &str,
        _project_id: &str,
    ) -> Result<FileAnalysisResult> {
        let analysis = match language {
            "rust" => {
                if self.rust_parser.can_parse(content, Some(file_path)) {
                    self.rust_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse Rust file: {}", file_path));
                }
            }
            "typescript" => {
                if self.typescript_parser.can_parse(content, Some(file_path)) {
                    self.typescript_parser
                        .parse_file(content, file_path)
                        .await?
                } else {
                    return Err(anyhow::anyhow!(
                        "Cannot parse TypeScript file: {}",
                        file_path
                    ));
                }
            }
            "javascript" => {
                if self.javascript_parser.can_parse(content, Some(file_path)) {
                    self.javascript_parser
                        .parse_file(content, file_path)
                        .await?
                } else {
                    return Err(anyhow::anyhow!(
                        "Cannot parse JavaScript file: {}",
                        file_path
                    ));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported language: {}", language));
            }
        };

        self.storage
            .store_file_analysis(file_id, language, &analysis)
            .await?;

        Ok(FileAnalysisResult {
            file_id,
            language: language.to_string(),
            elements_count: analysis.elements.len(),
            complexity_score: analysis.complexity_score,
            quality_issues_count: analysis.quality_issues.len(),
            test_coverage: if analysis.test_count > 0 {
                analysis.test_count as f64 / analysis.elements.len() as f64
            } else {
                0.0
            },
            doc_coverage: analysis.doc_coverage,
        })
    }

    pub async fn search_elements_for_project(
        &self,
        pattern: &str,
        project_id: &str,
        limit: Option<i32>,
    ) -> Result<Vec<CodeElement>> {
        self.storage
            .search_elements_for_project(pattern, project_id, limit.unwrap_or(20))
            .await
    }

    pub async fn get_file_analysis(&self, file_id: i64) -> Result<Option<FileContext>> {
        let elements = self.storage.get_file_elements(file_id).await?;
        if elements.is_empty() {
            return Ok(None);
        }

        let quality_issues = self.storage.get_file_quality_issues(file_id).await?;

        Ok(Some(FileContext {
            elements,
            quality_issues,
        }))
    }

    pub async fn get_repo_stats(&self, attachment_id: &str) -> Result<RepoStats> {
        self.storage.get_repo_stats(attachment_id).await
    }

    pub async fn get_complexity_hotspots_for_project(
        &self,
        project_id: &str,
        limit: Option<i32>,
    ) -> Result<Vec<CodeElement>> {
        self.storage
            .get_complexity_hotspots_for_project(project_id, limit.unwrap_or(10))
            .await
    }

    pub async fn get_elements_by_type(
        &self,
        element_type: &str,
        limit: Option<i32>,
    ) -> Result<Vec<CodeElement>> {
        self.storage.get_elements_by_type(element_type, limit).await
    }

    pub async fn delete_repository_data(&self, attachment_id: &str) -> Result<i64> {
        self.storage.delete_repository_data(attachment_id).await
    }
}
