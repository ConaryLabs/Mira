// src/services/file_context.rs
// LLM-based file context intent detection service

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path as StdPath;
use tracing::{info, debug};

use crate::{
    api::ws::message::MessageMetadata,
    git::GitClient,
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
        // Use configurable model from environment
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
        
        info!("File context intent: needs_content={}, confidence={:.2}", 
              intent.needs_file_content, intent.confidence);
        
        Ok(intent)
    }
    
    /// Get file content from git repository
    pub async fn get_file_content(
        &self,
        project_id: &str,
        file_path: &str,
        repo_id: Option<&str>,
    ) -> Result<String> {
        // Get attachment from store
        if let Some(repo) = repo_id {
            let attachments = self.git_client.store
                .list_project_attachments(project_id)
                .await
                .context("Failed to list attachments")?;
            
            let attachment = attachments
                .into_iter()
                .find(|a| a.id == repo)
                .ok_or_else(|| anyhow::anyhow!("Repository not found"))?;
            
            // Read file directly from the cloned repository
            let full_path = StdPath::new(&attachment.local_path).join(file_path);
            let content = std::fs::read_to_string(&full_path)
                .context("Failed to read file content")?;
            
            Ok(content)
        } else {
            Err(anyhow::anyhow!("Repository ID required to fetch file content"))
        }
    }
    
    /// Process a message with potential file context
    pub async fn process_with_context(
        &self,
        message: &str,
        metadata: &MessageMetadata,
        project_id: Option<&str>,
    ) -> Result<String> {
        // Check if file context is needed
        let intent = self.check_intent(message, metadata).await?;
        
        if intent.needs_file_content && intent.confidence > 0.7 {
            if let (Some(file_path), Some(repo_id)) = 
                (&metadata.file_path, &metadata.repo_id) {
                
                // Get the file content
                let content = self.get_file_content(
                    project_id.unwrap_or("default"),
                    file_path,
                    Some(repo_id),
                ).await?;
                
                // Get selected text if available
                let selected = metadata.selection.as_ref()
                    .and_then(|s| s.text.clone())
                    .unwrap_or_default();
                
                // Build enhanced message with context
                let enhanced = format!(
                    "User message: {}\n\nFile context ({}):\n{}\n{}",
                    message,
                    file_path,
                    if !selected.is_empty() {
                        format!("Selected text:\n{}", selected)
                    } else {
                        String::new()
                    },
                    if content.len() > 5000 {
                        format!("File content (truncated):\n{}", &content[..5000])
                    } else {
                        format!("File content:\n{}", content)
                    }
                );
                
                info!("Enhanced message with file context");
                return Ok(enhanced);
            }
        }
        
        // Return original message if no context needed
        Ok(message.to_string())
    }
}
