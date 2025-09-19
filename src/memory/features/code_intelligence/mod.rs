// src/memory/features/code_intelligence/mod.rs
// Main module for code intelligence - ties everything together

pub mod types;
pub mod parser;
pub mod storage;

// Re-export key types and implementations for easy use
pub use types::*;
pub use parser::RustParser;
pub use storage::{CodeIntelligenceStorage, RepoStats};

// Convenience imports for external users
use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Main code intelligence service that coordinates parsing and storage
pub struct CodeIntelligenceService {
    storage: Arc<CodeIntelligenceStorage>,
    rust_parser: RustParser,
}

impl CodeIntelligenceService {
    /// Create new code intelligence service
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            storage: Arc::new(CodeIntelligenceStorage::new(pool)),
            rust_parser: RustParser::new(),
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
        match language {
            "rust" => {
                if self.rust_parser.can_parse(content, Some(file_path)) {
                    let analysis = self.rust_parser.parse_file(content, file_path).await?;
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
                } else {
                    Err(anyhow::anyhow!("Cannot parse {} file: {}", language, file_path))
                }
            }
            _ => {
                Err(anyhow::anyhow!("Unsupported language: {}", language))
            }
        }
    }

    /// Search for code elements
    pub async fn search_elements(&self, pattern: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.search_elements(pattern, limit.unwrap_or(20)).await
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

    /// Get complexity hotspots
    pub async fn get_complexity_hotspots(&self, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_complexity_hotspots(limit.unwrap_or(10)).await
    }

    /// Get all functions, structs, enums, etc.
    pub async fn get_elements_by_type(&self, element_type: &str, limit: Option<i32>) -> Result<Vec<CodeElement>> {
        self.storage.get_elements_by_type(element_type, limit).await
    }

    /// Get the storage service directly (for advanced operations)
    pub fn storage(&self) -> Arc<CodeIntelligenceStorage> {
        self.storage.clone()
    }

    /// Check if a language is supported
    pub fn supports_language(&self, language: &str) -> bool {
        matches!(language, "rust")
    }

    /// Get list of supported languages
    pub fn supported_languages(&self) -> Vec<&'static str> {
        vec!["rust"]
    }
}

/// Result of analyzing a file
#[derive(Debug)]
pub struct FileAnalysisResult {
    pub file_id: i64,
    pub language: String,
    pub elements_count: usize,
    pub complexity_score: u32,
    pub quality_issues_count: usize,
    pub test_coverage: f64,
    pub doc_coverage: f64,
}

/// Complete context for a file
#[derive(Debug)]
pub struct FileContext {
    pub elements: Vec<CodeElement>,
    pub quality_issues: Vec<QualityIssue>,
}
