// src/services/file_context.rs

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::{
    api::ws::message::MessageMetadata,
    git::GitClient,
    llm::OpenAIClient,
    config::CONFIG,
};

#[derive(Clone)]
pub struct FileContextService {
    llm_client: Arc<OpenAIClient>,
    git_client: Arc<GitClient>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileIntent {
    needs_file_content: bool,
    confidence: f32,
    reasoning: String,
}

impl FileContextService {
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
    
    pub async fn check_intent(&self, message: &str, metadata: &MessageMetadata) -> Result<FileIntent> {
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

        let response = self.llm_client
            .generate_response(&prompt, None, true)
            .await
            .context("Failed to get intent analysis from LLM")?;

        let intent: FileIntent = serde_json::from_str(&response.content)
            .context("Failed to parse intent detection response")?;

        debug!(
            "Intent analysis result: needs_content={}, confidence={:.2}, reasoning={}",
            intent.needs_file_content, 
            intent.confidence,
            intent.reasoning.chars().take(100).collect::<String>()
        );

        Ok(intent)
    }

    pub async fn get_context_if_needed(
        &self,
        message: &str,
        metadata: &MessageMetadata,
        confidence_threshold: f32,
    ) -> Result<Option<String>> {
        let intent = self.check_intent(message, metadata).await?;
        
        if !intent.needs_file_content || intent.confidence < confidence_threshold {
            debug!(
                "Skipping file content: needs_content={}, confidence={:.2} < threshold={:.2}",
                intent.needs_file_content, intent.confidence, confidence_threshold
            );
            return Ok(None);
        }

        if let (Some(attachment_id), Some(file_path)) = (&metadata.attachment_id, &metadata.file_path) {
            info!("Retrieving file content for: {}", file_path);
            
            match self.git_client.store.get_attachment_by_id(attachment_id).await {
                Ok(Some(attachment)) => {
                    match self.git_client.get_file_content(&attachment, file_path) {
                        Ok(content) => {
                            debug!("Retrieved file content: {} bytes", content.len());
                            
                            const MAX_FILE_SIZE: usize = 50_000;
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

    pub fn format_file_context(&self, file_path: &str, content: &str, language: Option<&str>) -> String {
        let lang = language.unwrap_or("text");
        
        format!(
            "File: {}\nLanguage: {}\nContent:\n```{}\n{}\n```",
            file_path, lang, lang, content
        )
    }

    pub fn get_stats(&self) -> FileContextStats {
        FileContextStats {
            total_checks: 0,
            files_loaded: 0,
            average_confidence: 0.0,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FileContextStats {
    pub total_checks: u64,
    pub files_loaded: u64,
    pub average_confidence: f32,
}
