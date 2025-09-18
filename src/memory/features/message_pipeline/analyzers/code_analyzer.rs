// src/memory/features/message_pipeline/analyzers/code_analyzer.rs

//! Code message analyzer - handles programming content analysis
//! 
//! This is a stub implementation that will be enhanced with proper AST parsing
//! when code intelligence is implemented. For now, provides basic code detection.

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::llm::client::OpenAIClient;

// ===== CODE ANALYSIS RESULT =====

/// Result from code-specific analysis 
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisResult {
    // Language detection
    pub programming_lang: Option<String>,
    
    // Code quality metrics (future: from AST analysis)
    pub quality: Option<String>,
    pub complexity: Option<f32>,
    pub purpose: Option<String>,
    
    // Code elements (future: from proper parsing)  
    pub functions: Vec<String>,
    pub types: Vec<String>,
    pub imports: Vec<String>,
    
    // Processing metadata
    pub processed_at: chrono::DateTime<chrono::Utc>,
}

// ===== CODE ANALYZER =====

pub struct CodeAnalyzer {
    llm_client: Arc<OpenAIClient>,
}

impl CodeAnalyzer {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }
    
    /// Analyze code content
    /// TODO: Replace with proper AST parsing when code intelligence is implemented
    pub async fn analyze(
        &self,
        content: &str,
        _role: &str,
        _context: Option<&str>,
    ) -> Result<CodeAnalysisResult> {
        debug!("Analyzing code content: {} chars", content.len());
        
        // TEMPORARY: Basic analysis until AST parsing is implemented
        let programming_lang = self.detect_programming_language(content);
        
        if programming_lang.is_none() {
            warn!("Could not detect programming language for content");
        }
        
        // TODO: Replace with proper AST analysis
        let (functions, types, imports) = self.extract_basic_elements(content);
        
        // TODO: Replace with proper LLM-based semantic analysis when needed
        let (quality, complexity, purpose) = if content.len() > 100 {
            self.basic_semantic_analysis(content, &programming_lang).await?
        } else {
            (None, None, None)
        };
        
        Ok(CodeAnalysisResult {
            programming_lang,
            quality,
            complexity,
            purpose,
            functions,
            types,
            imports,
            processed_at: chrono::Utc::now(),
        })
    }
    
    /// Quick language detection based on syntax patterns
    /// TODO: Replace with proper file extension + AST-based detection
    fn detect_programming_language(&self, content: &str) -> Option<String> {
        // TEMPORARY: Pattern-based detection
        
        // Rust indicators
        if content.contains("fn ") || content.contains("impl ") || content.contains("struct ") 
            || content.contains("enum ") || content.contains("pub fn") || content.contains("use crate::") {
            return Some("rust".to_string());
        }
        
        // TypeScript indicators  
        if content.contains("interface ") || content.contains("type ") || content.contains(": string")
            || content.contains(": number") || content.contains("export interface") {
            return Some("typescript".to_string());
        }
        
        // JavaScript indicators
        if content.contains("function ") || content.contains("const ") || content.contains("=>")
            || content.contains("export default") || content.contains("import ") {
            return Some("javascript".to_string());
        }
        
        // Python indicators
        if content.contains("def ") || content.contains("class ") || content.contains("import ")
            || content.contains("from ") || content.starts_with("#!") {
            return Some("python".to_string());
        }
        
        None
    }
    
    /// Extract basic code elements using regex patterns
    /// TODO: Replace with proper AST parsing for accurate extraction
    fn extract_basic_elements(&self, content: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut functions = Vec::new();
        let mut types = Vec::new();
        let mut imports = Vec::new();
        
        // TEMPORARY: Basic regex-based extraction
        
        // Extract function names (Rust style)
        for line in content.lines() {
            let line = line.trim();
            
            // Rust functions
            if line.starts_with("pub fn ") || line.starts_with("fn ") {
                if let Some(name_start) = line.find("fn ") {
                    let name_part = &line[name_start + 3..];
                    if let Some(paren_pos) = name_part.find('(') {
                        let func_name = name_part[..paren_pos].trim().to_string();
                        if !func_name.is_empty() {
                            functions.push(func_name);
                        }
                    }
                }
            }
            
            // Rust types
            if line.starts_with("struct ") || line.starts_with("pub struct ") {
                if let Some(name) = self.extract_type_name(line, "struct") {
                    types.push(name);
                }
            }
            if line.starts_with("enum ") || line.starts_with("pub enum ") {
                if let Some(name) = self.extract_type_name(line, "enum") {
                    types.push(name);
                }
            }
            
            // Rust imports
            if line.starts_with("use ") {
                let import_part = line[4..].trim();
                if let Some(semicolon) = import_part.find(';') {
                    imports.push(import_part[..semicolon].trim().to_string());
                }
            }
        }
        
        (functions, types, imports)
    }
    
    /// Extract type name from struct/enum declaration
    fn extract_type_name(&self, line: &str, keyword: &str) -> Option<String> {
        if let Some(keyword_pos) = line.find(keyword) {
            let after_keyword = &line[keyword_pos + keyword.len()..].trim();
            let name_part = after_keyword.split_whitespace().next()?;
            // Remove generic parameters if present
            let name = if let Some(generic_pos) = name_part.find('<') {
                &name_part[..generic_pos]
            } else if let Some(brace_pos) = name_part.find('{') {
                &name_part[..brace_pos]
            } else {
                name_part
            };
            Some(name.trim().to_string())
        } else {
            None
        }
    }
    
    /// Basic semantic analysis using LLM
    /// TODO: This will be replaced/enhanced with AST-based analysis
    async fn basic_semantic_analysis(
        &self,
        content: &str,
        language: &Option<String>,
    ) -> Result<(Option<String>, Option<f32>, Option<String>)> {
        
        let lang_context = language
            .as_ref()
            .map(|l| format!("This is {} code. ", l))
            .unwrap_or_default();
        
        let prompt = format!(
            r#"{}Analyze this code snippet and provide:

```
{}
```

**Provide brief analysis:**
1. **Quality** (good/fair/poor): Overall code quality
2. **Complexity** (1.0-10.0): Algorithmic complexity
3. **Purpose** (1-2 words): What this code does

**Response format:**
```json
{{
  "quality": "<quality or null>",
  "complexity": <number or null>,
  "purpose": "<purpose or null>"
}}
```"#,
            lang_context, content
        );
        
        let response = self.llm_client
            .summarize_conversation(&prompt, 200)
            .await?;
        
        // Parse response
        if let Ok(json_str) = self.extract_json(&response) {
            #[derive(Deserialize)]
            struct CodeAnalysis {
                quality: Option<String>,
                complexity: Option<f32>,
                purpose: Option<String>,
            }
            
            if let Ok(parsed) = serde_json::from_str::<CodeAnalysis>(&json_str) {
                return Ok((
                    parsed.quality.filter(|s| !s.trim().is_empty()),
                    parsed.complexity.map(|c| c.max(1.0).min(10.0)),
                    parsed.purpose.filter(|s| !s.trim().is_empty()),
                ));
            }
        }
        
        // Fallback to no semantic analysis
        Ok((None, None, None))
    }
    
    /// Extract JSON from response (similar to chat_analyzer)
    fn extract_json(&self, response: &str) -> Result<String> {
        if let Some(start) = response.find("```json") {
            let json_start = start + 7;
            if let Some(end) = response[json_start..].find("```") {
                return Ok(response[json_start..json_start + end].trim().to_string());
            }
        }
        
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                if end > start {
                    return Ok(response[start..=end].to_string());
                }
            }
        }
        
        Err(anyhow::anyhow!("Could not extract JSON from response"))
    }
}
