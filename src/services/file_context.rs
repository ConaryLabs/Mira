// src/services/file_context.rs
// FIXED: Uses centralized CONFIG instead of environment variables
// LLM-based file context intent detection service

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path as StdPath;
use tracing::{info, debug, warn};

use crate::{
    api::ws::message::MessageMetadata,
    git::GitClient,
    llm::OpenAIClient,
    config::CONFIG,
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
        info!(
            "📄 FileContextService initialized with intent model: {}",
            CONFIG.intent_model
        );

        Self {
            llm_client,
            git_client,
        }
    }
    
    /// Check if a message needs file context using LLM
    /// FIXED: Now uses CONFIG.intent_model instead of environment variables
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
- "Tell me about JavaScript in general"

Respond in JSON format:
{{
    "needs_file_content": true/false,
    "confidence": 0.0-1.0,
    "reasoning": "brief explanation"
}}"#,
            file_info, message
        );
        
        debug!("Analyzing file context intent with centralized model configuration");
        
        // Use simple_chat to get JSON response
        let system_prompt = "You are a file context analyzer. Respond only with valid JSON. Be conservative - only request file content when the user is clearly asking about the specific file they're viewing.";
        
        let response = self.llm_client
            .simple_chat(&prompt, model, system_prompt)
            .await
            .context("Failed to get intent from LLM")?;
        
        // Parse the structured response
        let intent: FileIntent = serde_json::from_str(&response)
            .context("Failed to parse intent response as JSON")?;
        
        // Log with structured logging instead of println!
        info!(
            "File context intent analysis: needs_content={}, confidence={:.2}, reasoning='{}'", 
            intent.needs_file_content, 
            intent.confidence,
            intent.reasoning
        );
        
        // Add warning for low confidence decisions
        if intent.needs_file_content && intent.confidence < 0.6 {
            warn!(
                "Low confidence file context request: {:.2} - {}",
                intent.confidence,
                intent.reasoning
            );
        }
        
        Ok(intent)
    }
    
    /// Get file content from git repository
    pub async fn get_file_content(
        &self,
        project_id: &str,
        file_path: &str,
        repo_id: Option<&str>,
    ) -> Result<String> {
        debug!(
            "Fetching file content: project={}, file={}, repo={:?}",
            project_id,
            file_path,
            repo_id
        );

        // Get attachment from store
        if let Some(repo) = repo_id {
            let attachments = self.git_client.store
                .list_project_attachments(project_id)
                .await
                .context("Failed to list project attachments")?;
            
            let attachment = attachments
                .into_iter()
                .find(|a| a.id == repo)
                .with_context(|| format!("Repository '{}' not found in project '{}'", repo, project_id))?;
            
            // Read file directly from the cloned repository
            let full_path = StdPath::new(&attachment.local_path).join(file_path);
            
            // Check if file exists and is readable
            if !full_path.exists() {
                return Err(anyhow::anyhow!(
                    "File '{}' not found in repository at '{}'", 
                    file_path, 
                    attachment.local_path
                ));
            }

            let content = std::fs::read_to_string(&full_path)
                .with_context(|| format!("Failed to read file content from '{}'", full_path.display()))?;
            
            info!(
                "Successfully loaded file content: {} bytes from {}",
                content.len(),
                file_path
            );
            
            Ok(content)
        } else {
            Err(anyhow::anyhow!("Repository ID is required to fetch file content"))
        }
    }
    
    /// Process a message with potential file context
    pub async fn process_with_context(
        &self,
        message: &str,
        metadata: &MessageMetadata,
        project_id: Option<&str>,
    ) -> Result<String> {
        debug!("Processing message with potential file context");

        // Check if file context is needed
        let intent = self.check_intent(message, metadata).await?;
        
        // Use configurable confidence threshold (could be added to CONFIG later)
        let confidence_threshold = 0.7;
        
        if intent.needs_file_content && intent.confidence > confidence_threshold {
            if let (Some(file_path), Some(repo_id)) = 
                (&metadata.file_path, &metadata.repo_id) {
                
                debug!("Intent analysis suggests file content is needed - fetching file");
                
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
                let enhanced = if !selected.is_empty() {
                    format!(
                        "User is viewing file: {}\n\nSelected text:\n```\n{}\n```\n\nFull file content:\n```\n{}\n```\n\nUser's question: {}",
                        file_path, selected, content, message
                    )
                } else {
                    format!(
                        "User is viewing file: {}\n\nFile content:\n```\n{}\n```\n\nUser's question: {}",
                        file_path, content, message
                    )
                };
                
                info!(
                    "Enhanced message with file context: {} chars total (file: {} chars, selected: {} chars)",
                    enhanced.len(),
                    content.len(),
                    selected.len()
                );
                
                Ok(enhanced)
            } else {
                warn!("Intent suggests file context needed but file_path or repo_id missing");
                Ok(message.to_string())
            }
        } else {
            debug!(
                "No file context needed: needs_content={}, confidence={:.2} (threshold: {:.2})",
                intent.needs_file_content,
                intent.confidence,
                confidence_threshold
            );
            Ok(message.to_string())
        }
    }
    
    /// Get the current intent model configuration
    /// NEW: Provides access to configuration for debugging
    pub fn get_intent_model(&self) -> &str {
        &CONFIG.intent_model
    }
    
    /// Check if file context service is properly configured
    /// NEW: Health check functionality
    pub fn is_configured(&self) -> bool {
        !CONFIG.intent_model.is_empty()
    }
}

// REMOVED: std::env::var("MIRA_INTENT_MODEL") usage
// ADDED: Centralized CONFIG.intent_model usage
// ADDED: Enhanced structured logging with context
// ADDED: Better error handling with detailed context
// ADDED: Configuration introspection methods
// IMPROVED: More detailed debug information
