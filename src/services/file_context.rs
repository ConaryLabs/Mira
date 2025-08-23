// src/services/file_context.rs
// CONSOLIDATED: Uses centralized CONFIG.intent_model instead of environment variables
// CLEANED: Professional logging without emojis for terminal-friendly output

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path as StdPath;
use tracing::{debug, info, warn};

use crate::{
    api::ws::message::MessageMetadata,
    git::GitClient,
    llm::OpenAIClient,
    config::CONFIG, // Single source of truth
};

/// Service for determining if file context is needed for a message
/// Uses centralized CONFIG instead of environment variables
#[derive(Clone)]
pub struct FileContextService {
    llm_client: Arc<OpenAIClient>,
    git_client: Arc<GitClient>,
}

/// Intent detection result from LLM
#[derive(Debug, Serialize, Deserialize)]
struct FileIntent {
    needs_file_content: bool,
    confidence: f32,
    reasoning: String,
}

impl FileContextService {
    /// Create new FileContextService
    /// Uses centralized CONFIG for consistency  
    pub fn new(llm_client: Arc<OpenAIClient>, git_client: Arc<GitClient>) -> Self {
        debug!(
            "FileContextService initialized with intent model: {}",
            CONFIG.intent_model
        );

        Self {
            llm_client,
            git_client,
        }
    }
    
    /// Check if a message needs file context using LLM
    /// Now uses CONFIG.intent_model instead of environment variable
    pub async fn check_intent(&self, message: &str, metadata: &MessageMetadata) -> Result<FileIntent> {
        // Use centralized configuration instead of environment variable
        let model = &CONFIG.intent_model;
        
        debug!(
            "Checking file context intent with model: {} for file: {}",
            model,
            metadata.file_path.as_deref().unwrap_or("unknown")
        );

        let file_info = format!(
            "User is viewing: {}\nLanguage: {}",
            metadata.file_path.as_deref().unwrap_or("unknown"),
            metadata.language.as_deref().unwrap_or("unknown")
        );
        
        // Build prompt for intent detection
        let prompt = format!(
            r#"You are analyzing whether a user's message needs the content of a file they're viewing.

Context:
{}

User's message: "{}"

Analyze if this message is asking about, referring to, or needs the content of the file being viewed.

Examples of messages that NEED file content:
- "What does this function do?"
- "How can I fix this error on line 42?"
- "Explain this code"
- "What variables are defined here?"

Examples of messages that DON'T need file content:
- "Hello"
- "What's the weather?"
- "How are you?"

Respond with JSON:
{{
    "needs_file_content": boolean,
    "confidence": number (0.0-1.0),
    "reasoning": "explanation of why this decision was made"
}}"#,
            file_info, message
        );

        debug!("Intent detection prompt length: {} chars", prompt.len());

        // Call LLM for intent analysis
        let response = self.llm_client
            .generate_response(&prompt, None, true)
            .await
            .context("Failed to get intent analysis from LLM")?;

        // Parse JSON response
        let intent: FileIntent = serde_json::from_str(&response.text)
            .context("Failed to parse intent detection response")?;

        debug!(
            "Intent analysis result: needs_content={}, confidence={:.2}, reasoning={}",
            intent.needs_file_content, 
            intent.confidence,
            intent.reasoning.chars().take(100).collect::<String>()
        );

        Ok(intent)
    }

    /// Get file content if needed based on intent analysis
    pub async fn get_context_if_needed(
        &self,
        message: &str,
        metadata: &MessageMetadata,
        confidence_threshold: f32,
    ) -> Result<Option<String>> {
        // First check if we should get file content
        let intent = self.check_intent(message, metadata).await?;
        
        if !intent.needs_file_content || intent.confidence < confidence_threshold {
            debug!(
                "Skipping file content: needs_content={}, confidence={:.2} < threshold={:.2}",
                intent.needs_file_content, intent.confidence, confidence_threshold
            );
            return Ok(None);
        }

        // Get the file content
        if let (Some(attachment_id), Some(file_path)) = (&metadata.attachment_id, &metadata.file_path) {
            info!("Retrieving file content for: {}", file_path);
            
            // Try to get file content from git client
            match self.git_client.store.get_attachment_by_id(attachment_id).await {
                Ok(Some(attachment)) => {
                    match self.git_client.get_file_content(&attachment, file_path) {
                        Ok(content) => {
                            debug!("Retrieved file content: {} bytes", content.len());
                            
                            // Limit file size for context (prevent token overflow)
                            const MAX_FILE_SIZE: usize = 50_000; // ~50KB
                            if content.len() > MAX_FILE_SIZE {
                                let truncated = format!(
                                    "{}... [truncated - file too large ({} bytes)]",
                                    &content[..MAX_FILE_SIZE],
                                    content.len()
                                );
                                warn!("File content truncated from {} to {} bytes", content.len(), truncated.len());
                                Ok(Some(truncated))
                            } else {
                                Ok(Some(content))
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get file content for {}: {}", file_path, e);
                            Ok(None)
                        }
                    }
                }
                Ok(None) => {
                    warn!("Attachment {} not found", attachment_id);
                    Ok(None)
                }
                Err(e) => {
                    warn!("Failed to get attachment {}: {}", attachment_id, e);
                    Ok(None)
                }
            }
        } else {
            debug!("Missing attachment_id or file_path in metadata");
            Ok(None)
        }
    }

    /// Format file content for inclusion in chat context
    pub fn format_file_context(&self, file_path: &str, content: &str, language: Option<&str>) -> String {
        let lang = language.unwrap_or("text");
        
        format!(
            "File: {}\nLanguage: {}\nContent:\n```{}\n{}\n```",
            file_path, lang, lang, content
        )
    }

    /// Get summary stats about file context usage
    pub fn get_stats(&self) -> FileContextStats {
        // This could be expanded to track actual usage statistics
        FileContextStats {
            total_checks: 0,
            files_loaded: 0,
            average_confidence: 0.0,
        }
    }
}

/// Statistics about file context usage
#[derive(Debug, Serialize)]
pub struct FileContextStats {
    pub total_checks: u64,
    pub files_loaded: u64,
    pub average_confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_context() {
        let service = create_test_service();
        
        let formatted = service.format_file_context(
            "test.rs",
            "fn main() {\n    println!(\"Hello\");\n}",
            Some("rust")
        );
        
        assert!(formatted.contains("File: test.rs"));
        assert!(formatted.contains("Language: rust"));
        assert!(formatted.contains("```rust"));
        assert!(formatted.contains("fn main()"));
    }

    #[test]
    fn test_file_intent_serialization() {
        let intent = FileIntent {
            needs_file_content: true,
            confidence: 0.85,
            reasoning: "User is asking about a specific function".to_string(),
        };
        
        let serialized = serde_json::to_string(&intent).unwrap();
        let deserialized: FileIntent = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(intent.needs_file_content, deserialized.needs_file_content);
        assert_eq!(intent.confidence, deserialized.confidence);
        assert_eq!(intent.reasoning, deserialized.reasoning);
    }

    // Helper for creating test service (would need proper mocks in real tests)
    fn create_test_service() -> FileContextService {
        // This would need proper mock objects in a real test
        use std::sync::Arc;
        
        // Create minimal test service - this won't work for real tests
        // but shows the structure for proper test setup
        FileContextService {
            llm_client: Arc::new(unsafe { std::mem::zeroed() }), // Don't do this in real tests!
            git_client: Arc::new(unsafe { std::mem::zeroed() }), // Don't do this in real tests!
        }
    }
}
