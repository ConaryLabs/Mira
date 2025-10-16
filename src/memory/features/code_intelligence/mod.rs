// src/memory/features/code_intelligence/mod.rs

pub mod types;
pub mod parser;
pub mod storage;
pub mod typescript_parser;
pub mod javascript_parser;
// REMOVED: websocket_storage and websocket_analyzer (Phase 1 - tables deleted)

pub use types::*;
pub use parser::RustParser;
pub use typescript_parser::TypeScriptParser;
pub use javascript_parser::JavaScriptParser;
pub use storage::{CodeIntelligenceStorage, RepoStats};

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct CodeIntelligenceService {
    storage: Arc<CodeIntelligenceStorage>,
    rust_parser: RustParser,
    typescript_parser: TypeScriptParser,
    javascript_parser: JavaScriptParser,
    // REMOVED: websocket_storage (Phase 1 - tables deleted)
}

impl CodeIntelligenceService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            storage: Arc::new(CodeIntelligenceStorage::new(pool.clone())),
            rust_parser: RustParser::new(),
            typescript_parser: TypeScriptParser::new(),
            javascript_parser: JavaScriptParser::new(),
            // REMOVED: websocket_storage (Phase 1 - tables deleted)
        }
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
                    self.typescript_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse TypeScript file: {}", file_path));
                }
            }
            "javascript" => {
                if self.javascript_parser.can_parse(content, Some(file_path)) {
                    self.javascript_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse JavaScript file: {}", file_path));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported language: {}", language));
            }
        };
        
        self.storage.store_file_analysis(file_id, language, &analysis).await?;
        
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
        _project_id: &str, // Keep parameter for compatibility but don't use
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
                    self.typescript_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse TypeScript file: {}", file_path));
                }
            }
            "javascript" => {
                if self.javascript_parser.can_parse(content, Some(file_path)) {
                    self.javascript_parser.parse_file(content, file_path).await?
                } else {
                    return Err(anyhow::anyhow!("Cannot parse JavaScript file: {}", file_path));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported language: {}", language));
            }
        };
        
        self.storage.store_file_analysis(file_id, language, &analysis).await?;
        
        // REMOVED: WebSocket storage (Phase 1 - tables deleted)
        
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
    
    pub async fn search_elements_for_project(&self, pattern: &str, project_id: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.search_elements_for_project(pattern, project_id, limit.unwrap_or(20)).await
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

    pub async fn get_complexity_hotspots_for_project(&self, project_id: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_complexity_hotspots_for_project(project_id, limit.unwrap_or(10)).await
    }

    pub async fn get_elements_by_type(&self, element_type: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_elements_by_type(element_type, limit).await
    }

    pub async fn delete_repository_data(&self, attachment_id: &str) -> Result<i64> {
        self.storage.delete_repository_data(attachment_id).await
    }
}
