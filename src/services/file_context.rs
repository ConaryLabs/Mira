// src/services/file_context.rs
// LLM-based file context intent detection service

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path as StdPath;
use tracing::{info, warn, debug};

use crate::{
    api::ws::message::MessageMetadata,
    git::{GitClient, GitStore},
    llm::OpenAIClient,
};

/// Service for determining if file context is needed for a message
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
    pub fn new(llm_client: Arc<OpenAIClient>, git_client: Arc<GitClient>) -> Self {
        Self {
            llm_client,
            git_client,
        }
    }
    
    /// Check if a message needs file context using LLM
    pub async fn check_intent(&self, message: &str, metadata: &MessageMetadata) -> Result<FileIntent> {
        // Use configurable model or default to gpt-5
        let model = std::env::var("MIRA_INTENT_MODEL")
            .unwrap_or_else(|_| "gpt-5".to_string());
        
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

Respond in JSON format:
{{
    "needs_file_content": true/false,
    "confidence": 0.0-1.0,
    "reasoning": "brief explanation"
}}"#,
            file_info, message
        );
        
        debug!("Checking file context intent with LLM");
        
        // Use simple_chat to get JSON response
        let system_prompt = "You are a file context analyzer. Respond only with valid JSON.";
        
        let response = self.llm_client
            .simple_chat(&prompt, &model, system_prompt)
            .await
            .context("Failed to get intent from LLM")?;
        
        // Parse the structured response
        let intent: FileIntent = serde_json::from_str(&response)
            .context("Failed to parse intent response")?;
        
        info!(
            "File context intent: needs={}, confidence={:.2}, reason={}",
            intent.needs_file_content, intent.confidence, intent.reasoning
        );
        
        Ok(intent)
    }
    
    /// Get file content if needed based on intent
    pub async fn get_file_content_if_needed(
        &self,
        message: &str,
        project_id: &str,
        metadata: &MessageMetadata,
    ) -> Result<Option<String>> {
        // Check intent with LLM
        let intent = self.check_intent(message, metadata).await?;
        
        // If LLM says we don't need the file, return None
        if !intent.needs_file_content || intent.confidence < 0.7 {
            debug!("LLM determined file content not needed (confidence: {:.2})", intent.confidence);
            return Ok(None);
        }
        
        // Get the file content directly from filesystem
        if let (Some(attachment_id), Some(file_path)) = 
            (&metadata.attachment_id, &metadata.file_path) {
            
            debug!("Fetching file content for {}", file_path);
            
            // Get attachment info to find local path
            match self.git_client.store.get_attachment_by_id(attachment_id).await {
                Ok(Some(attachment)) => {
                    // Build full file path
                    let full_path = StdPath::new(&attachment.local_path).join(file_path);
                    
                    // Read file content
                    match tokio::fs::read_to_string(&full_path).await {
                        Ok(content) => {
                            // Truncate if too large (>10KB)
                            let truncated = if content.len() > 10240 {
                                let truncated = format!(
                                    "{}\n\n[... file truncated, showing first 10KB ...]",
                                    &content[..10240]
                                );
                                warn!("File content truncated from {} to 10KB", content.len());
                                truncated
                            } else {
                                content
                            };
                            
                            info!("Including file content ({} bytes)", truncated.len());
                            Ok(Some(truncated))
                        }
                        Err(e) => {
                            warn!("Failed to read file content: {}", e);
                            Ok(None)
                        }
                    }
                }
                Ok(None) => {
                    warn!("Attachment {} not found", attachment_id);
                    Ok(None)
                }
                Err(e) => {
                    warn!("Failed to get attachment: {}", e);
                    Ok(None)
                }
            }
        } else {
            debug!("No attachment_id or file_path in metadata");
            Ok(None)
        }
    }
    
    /// Process a message with potential file context
    pub async fn enhance_message_with_context(
        &self,
        message: &str,
        project_id: &str,
        metadata: Option<&MessageMetadata>,
    ) -> Result<String> {
        // If no metadata, return original message
        let metadata = match metadata {
            Some(m) => m,
            None => return Ok(message.to_string()),
        };
        
        // Get file content if LLM determines it's needed
        if let Some(file_content) = self
            .get_file_content_if_needed(message, project_id, metadata)
            .await? {
            
            // Build enhanced message with file context
            let enhanced = format!(
                "I'm looking at `{}` which contains:\n\n```{}\n{}\n```\n\n{}",
                metadata.file_path.as_deref().unwrap_or("file"),
                metadata.language.as_deref().unwrap_or(""),
                file_content,
                message
            );
            
            debug!("Enhanced message with file context");
            Ok(enhanced)
        } else {
            Ok(message.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_intent_parsing() {
        let json = r#"{
            "needs_file_content": true,
            "confidence": 0.95,
            "reasoning": "User is asking about 'this function' which refers to code"
        }"#;
        
        let intent: FileIntent = serde_json::from_str(json).unwrap();
        assert!(intent.needs_file_content);
        assert!(intent.confidence > 0.9);
    }
}
