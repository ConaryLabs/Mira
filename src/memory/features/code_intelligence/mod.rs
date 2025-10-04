// src/memory/features/code_intelligence/mod.rs

pub mod types;
pub mod parser;
pub mod storage;
pub mod typescript_parser;
pub mod javascript_parser;
pub mod websocket_storage;
pub mod websocket_analyzer;  // NEW: Separate WebSocket analysis

// Re-export key types and implementations for easy use
pub use types::*;
pub use parser::RustParser;
pub use typescript_parser::TypeScriptParser;
pub use javascript_parser::JavaScriptParser;
pub use storage::{CodeIntelligenceStorage, RepoStats};
pub use websocket_storage::{WebSocketStorage, OrphanedCall, UnusedHandler, DependencyReport};
pub use websocket_analyzer::{WebSocketAnalyzer, TypeScriptWebSocketAnalyzer, WebSocketAnalysis};  // NEW

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Main code intelligence service that coordinates parsing and storage
#[derive(Clone)]
pub struct CodeIntelligenceService {
    storage: Arc<CodeIntelligenceStorage>,
    rust_parser: RustParser,
    typescript_parser: TypeScriptParser,
    javascript_parser: JavaScriptParser,
    websocket_storage: Arc<WebSocketStorage>,  // NEW
}

impl CodeIntelligenceService {
    /// Create new code intelligence service
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            storage: Arc::new(CodeIntelligenceStorage::new(pool.clone())),
            rust_parser: RustParser::new(),
            typescript_parser: TypeScriptParser::new(),
            javascript_parser: JavaScriptParser::new(),
            websocket_storage: Arc::new(WebSocketStorage::new(pool)),  // NEW
        }
    }

    /// Analyze a file and store results
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
                    let result = self.rust_parser.parse_file(content, file_path).await?;
                    
                    // NEW: Store WebSocket handlers/responses for Rust files
                    // Note: We need project_id here but it's not passed in. 
                    // For now, we'll store them during git import when we have project_id.
                    // This is a limitation we'll address in the full integration.
                    
                    result
                } else {
                    return Err(anyhow::anyhow!("Cannot parse Rust file: {}", file_path));
                }
            }
            "typescript" => {
                if self.typescript_parser.can_parse(content, Some(file_path)) {
                    let result = self.typescript_parser.parse_file(content, file_path).await?;
                    
                    // NEW: Store WebSocket calls for TypeScript files
                    // Same project_id limitation as above
                    
                    result
                } else {
                    return Err(anyhow::anyhow!("Cannot parse TypeScript file: {}", file_path));
                }
            }
            "javascript" => {
                if self.javascript_parser.can_parse(content, Some(file_path)) {
                    let result = self.javascript_parser.parse_file(content, file_path).await?;
                    
                    // NEW: Store WebSocket calls for JavaScript files
                    // Same project_id limitation as above
                    
                    result
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
    
    /// NEW: Analyze and store with project context (for WebSocket tracking)
    pub async fn analyze_and_store_with_project(
        &self,
        file_id: i64,
        content: &str,
        file_path: &str,
        language: &str,
        project_id: &str,
    ) -> Result<FileAnalysisResult> {
        // Regular file analysis (pure parsing)
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
        
        // Store basic analysis
        self.storage.store_file_analysis(file_id, language, &analysis).await?;
        
        // NEW: Separate WebSocket analysis (optional, project-specific)
        match language {
            "rust" => {
                if let Ok(ws_analysis) = WebSocketAnalyzer::analyze_rust_file(content) {
                    if !ws_analysis.handlers.is_empty() {
                        self.websocket_storage.store_websocket_handlers(
                            project_id,
                            file_id,
                            &ws_analysis.handlers,
                        ).await?;
                    }
                    
                    if !ws_analysis.responses.is_empty() {
                        self.websocket_storage.store_websocket_responses(
                            project_id,
                            file_id,
                            &ws_analysis.responses,
                        ).await?;
                    }
                }
            }
            "typescript" | "javascript" => {
                if let Ok(ws_calls) = TypeScriptWebSocketAnalyzer::analyze(content, file_path) {
                    if !ws_calls.is_empty() {
                        self.websocket_storage.store_websocket_calls(
                            project_id,
                            file_id,
                            file_path,
                            &ws_calls,
                        ).await?;
                    }
                }
            }
            _ => {}
        }
        
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
    
    /// NEW: Link WebSocket dependencies
    pub async fn link_websocket_dependencies(&self, project_id: &str) -> Result<()> {
        self.websocket_storage.link_calls_to_handlers(project_id).await?;
        Ok(())
    }
    
    /// NEW: Get dependency report
    pub async fn get_dependency_report(&self, project_id: &str) -> Result<DependencyReport> {
        let orphaned_calls = self.websocket_storage.find_orphaned_calls(project_id).await?;
        let unused_handlers = self.websocket_storage.find_unused_handlers(project_id).await?;
        
        Ok(DependencyReport {
            orphaned_calls,
            unused_handlers,
        })
    }

    /// Search for code elements in a specific project
    pub async fn search_elements_for_project(&self, pattern: &str, project_id: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.search_elements_for_project(pattern, project_id, limit.unwrap_or(20)).await
    }

    /// Get analysis for a specific file
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

    /// Get repository statistics
    pub async fn get_repo_stats(&self, attachment_id: &str) -> Result<RepoStats> {
        self.storage.get_repo_stats(attachment_id).await
    }

    /// Get complexity hotspots for a specific project
    pub async fn get_complexity_hotspots_for_project(&self, project_id: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_complexity_hotspots_for_project(project_id, limit.unwrap_or(10)).await
    }

    /// Get all functions, structs, enums, etc.
    pub async fn get_elements_by_type(&self, element_type: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_elements_by_type(element_type, limit).await
    }

    /// Delete all code intelligence data for a repository
    pub async fn delete_repository_data(&self, attachment_id: &str) -> Result<i64> {
        self.storage.delete_repository_data(attachment_id).await
    }
}
