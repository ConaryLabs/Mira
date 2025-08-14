// src/services/chat.rs
// Phase 7: Unified ChatService using GPT-5 Responses API

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, debug, instrument};

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::{ThreadManager, ResponseMessage};
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::services::memory::MemoryService;
use crate::persona::PersonaOverlay;

/// Output format for chat responses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

/// Configuration for ChatService
#[derive(Clone)]
struct ChatConfig {
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub enable_debug_logging: bool,
}

impl ChatConfig {
    fn from_env() -> Self {
        let enable_debug_logging = std::env::var("MIRA_DEBUG_LOGGING")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        Self {
            model: std::env::var("MIRA_MODEL").unwrap_or_else(|_| "gpt-5".to_string()),
            verbosity: std::env::var("MIRA_VERBOSITY").unwrap_or_else(|_| "medium".to_string()),
            reasoning_effort: std::env::var("MIRA_REASONING_EFFORT")
                .unwrap_or_else(|_| "medium".to_string()),
            max_output_tokens: std::env::var("MIRA_MAX_OUTPUT_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(128000),  // 128k tokens - maximum for GPT-5
            history_message_cap: std::env::var("MIRA_HISTORY_MESSAGE_CAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            history_token_limit: std::env::var("MIRA_HISTORY_TOKEN_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(32768),  // 32k tokens for history
            max_retrieval_tokens: std::env::var("MIRA_MAX_RETRIEVAL_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096),   // 4k tokens for context retrieval
            enable_debug_logging,
        }
    }
}

/// Unified ChatService for all chat interfaces
#[derive(Clone)]
pub struct ChatService {
    client: Arc<OpenAIClient>,
    threads: Arc<ThreadManager>,
    memory_service: Arc<MemoryService>,
    vector_store_manager: Arc<VectorStoreManager>,
    persona: PersonaOverlay,
    config: ChatConfig,
}

impl ChatService {
    /// Create a new ChatService with all dependencies
    pub fn new(
        client: Arc<OpenAIClient>,
        threads: Arc<ThreadManager>,
        memory_service: Arc<MemoryService>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
    ) -> Self {
        let config = ChatConfig::from_env();

        info!("🎛️ ChatService configuration:");
        info!("   Model: {}", config.model);
        info!("   Verbosity: {}", config.verbosity);
        info!("   Reasoning: {}", config.reasoning_effort);
        info!("   Max output tokens: {}", config.max_output_tokens);
        info!("   History message cap: {}", config.history_message_cap);
        info!("   History token limit: {}", config.history_token_limit);
        info!("   Max retrieval tokens: {}", config.max_retrieval_tokens);
        info!("   Debug logging: {}", config.enable_debug_logging);

        Self {
            client,
            threads,
            memory_service,
            vector_store_manager,
            persona,
            config,
        }
    }

    /// Process a message through the unified chat pipeline
    #[instrument(skip(self), fields(session_id = %session_id, project_id = ?project_id))]
    pub async fn process_message(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
        return_structured: bool,
    ) -> Result<ChatResponse> {
        let start_time = Instant::now();

        info!("🚀 Processing chat message");
        debug!("User input: {} chars", user_text.len());

        // 1) persist user msg
        self.memory_service.save_user_message(session_id, user_text).await?;

        // 2) add to thread
        self.threads.add_message(session_id, ResponseMessage {
            role: "user".to_string(),
            content: Some(user_text.to_string()),
        }).await?;

        // 3) history (kept for observability)
        let history = self.threads
            .get_conversation_capped(session_id, self.config.history_message_cap)
            .await;
        info!("📜 History: {} messages", history.len());

        // 4) context
        let context = self.build_context(session_id, user_text, project_id).await?;

        // 5) call GPT-5
        let gpt5_response = self.respond_gpt5(
            session_id,
            user_text,
            &context,
            return_structured,
        ).await?;

        // 6) persist assistant response
        self.threads.add_message(session_id, ResponseMessage {
            role: "assistant".to_string(),
            content: Some(gpt5_response.output.clone()),
        }).await?;

        // Use save_assistant_response instead of save_assistant_message
        self.memory_service.save_assistant_response(
            session_id,
            &gpt5_response,
        ).await?;

        let elapsed = start_time.elapsed();
        info!("✅ Chat response generated in {:?}", elapsed);

        Ok(gpt5_response)
    }

    /// Build context from memory and vector store
    async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<String> {
        let mut context_parts = vec![];

        // Get recent context
        let recent_context = self.memory_service
            .get_recent_context(session_id, 4)
            .await?;
        
        if !recent_context.is_empty() {
            // Convert MemoryEntry to strings
            let context_strings: Vec<String> = recent_context
                .iter()
                .map(|entry| entry.content.clone())
                .collect();
            context_parts.push(format!("Recent context:\n{}", context_strings.join("\n")));
        }

        // Get embedding for the user text first
        let embedding = self.client.get_embedding(user_text).await?;
        
        // Now search with the embedding
        let similar_memories = self.memory_service
            .search_similar(session_id, &embedding, 3)
            .await?;
        
        if !similar_memories.is_empty() {
            let memories_text = similar_memories
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect::<Vec<_>>()
                .join("\n");
            context_parts.push(format!("Related memories:\n{}", memories_text));
        }

        // Skip vector store context for now since the method doesn't exist
        // TODO: Implement vector store search when available

        Ok(context_parts.join("\n\n"))
    }

    /// Call GPT-5 Responses API with proper formatting
    async fn respond_gpt5(
        &self,
        session_id: &str,
        user_text: &str,
        context: &str,
        return_structured: bool,
    ) -> Result<ChatResponse> {
        // Build the input messages - use the JSON version when structured output is needed
        let history = self.get_trimmed_history(session_id).await?;
        let input = if return_structured {
            // Use the method that adds "JSON" to the system message
            self.build_gpt5_input_with_json_instruction(history, user_text, context)
        } else {
            self.build_gpt5_input(history, user_text, context)
        };

        // Build the request
        let response_manager = crate::llm::responses::manager::ResponsesManager::new(self.client.clone());
        
        // Prepare parameters
        let parameters = serde_json::json!({
            "verbosity": self.config.verbosity,
            "reasoning_effort": self.config.reasoning_effort,
            "max_output_tokens": self.config.max_output_tokens,
        });

        // Prepare response format if structured
        let response_format = if return_structured {
            Some(serde_json::json!({ "type": "json_object" }))
        } else {
            None
        };

        // Build instructions with JSON mention if needed
        let instructions = if return_structured {
            format!(
                "{}\n\nIMPORTANT: You must respond with a valid JSON object containing the following fields: reply (string), mood (string), salience (number 0-10), summary (string), memory_type (string), tags (array of strings), intent (string), and optionally monologue (string) and reasoning_summary (string).",
                self.persona.prompt()
            )
        } else {
            self.build_instructions()
        };

        // Call the API
        let response = response_manager.create_response(
            &self.config.model,
            input,
            Some(instructions),
            response_format,
            Some(parameters),
        ).await?;

        // Parse the response
        if return_structured {
            self.parse_structured_response(&response.text)
        } else {
            Ok(self.create_default_response(&response.text))
        }
    }

    /// Build GPT-5 input messages with proper content format
    fn build_gpt5_input(
        &self,
        history: Vec<ResponseMessage>,
        user_text: &str,
        context: &str,
    ) -> Vec<serde_json::Value> {
        let mut messages = vec![];

        // Add history (already in GPT-5 format)
        for msg in history {
            if let Some(content) = msg.content {
                messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": [{
                        "type": if msg.role == "user" { "input_text" } else { "output_text" },
                        "text": content
                    }]
                }));
            }
        }

        // Add context if present
        if !context.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": format!("Context:\n{}", context)
                }]
            }));
        }

        // Add current user message
        messages.push(serde_json::json!({
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": user_text
            }]
        }));

        messages
    }

    /// Build GPT-5 input messages with JSON instruction for structured output
    fn build_gpt5_input_with_json_instruction(
        &self,
        history: Vec<ResponseMessage>,
        user_text: &str,
        context: &str,
    ) -> Vec<serde_json::Value> {
        let mut messages = vec![];

        // Add a system message that mentions JSON format
        messages.push(serde_json::json!({
            "role": "system",
            "content": [{
                "type": "input_text",
                "text": "You must respond with a valid JSON object containing structured data."
            }]
        }));

        // Add history (already in GPT-5 format)
        for msg in history {
            if let Some(content) = msg.content {
                messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": [{
                        "type": if msg.role == "user" { "input_text" } else { "output_text" },
                        "text": content
                    }]
                }));
            }
        }

        // Add context if present
        if !context.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": format!("Context:\n{}", context)
                }]
            }));
        }

        // Add current user message
        messages.push(serde_json::json!({
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": user_text
            }]
        }));

        messages
    }

    /// Build instructions for GPT-5 (includes persona)
    fn build_instructions(&self) -> String {
        let mut instructions = self.persona.prompt().to_string();
        
        // Add JSON format requirement if needed
        instructions.push_str("\n\nWhen responding, maintain the persona and mood consistently.");
        
        instructions
    }

    /// Parse structured response from GPT-5
    fn parse_structured_response(&self, text: &str) -> Result<ChatResponse> {
        // Try raw JSON first
        if let Ok(parsed) = serde_json::from_str::<MiraStructuredReply>(text) {
            return Ok(self.response_from_struct(parsed));
        }

        // If model wrapped JSON in fences, try to extract
        let trimmed = text.trim();
        if (trimmed.starts_with("```") && trimmed.ends_with("```")) || trimmed.starts_with('{') {
            // Remove possible ```json fences
            let without_fences = trimmed
                .trim_start_matches("```json")
                .trim_start_matches("```JSON")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            if let Ok(parsed) = serde_json::from_str::<MiraStructuredReply>(without_fences) {
                return Ok(self.response_from_struct(parsed));
            }
        }

        // Fallback: treat as plain text
        Ok(self.create_default_response(text))
    }

    fn response_from_struct(&self, s: MiraStructuredReply) -> ChatResponse {
        let sal = s.salience.unwrap_or(5.0);
        let salience = sal.max(0.0).min(10.0) as usize;

        ChatResponse {
            output: s.reply,
            persona: s.persona.unwrap_or_else(|| self.persona.name().to_string()),
            mood: s.mood.unwrap_or_else(|| "neutral".to_string()),
            salience,
            summary: s.summary.unwrap_or_else(|| "General conversation".to_string()),
            memory_type: s.memory_type.unwrap_or_else(|| "other".to_string()),
            tags: s.tags.unwrap_or_default(),
            intent: s.intent.unwrap_or_else(|| "unknown".to_string()),
            monologue: s.monologue,
            reasoning_summary: s.reasoning_summary,
        }
    }

    /// Create a default response when no structured data is available
    fn create_default_response(&self, text: &str) -> ChatResponse {
        ChatResponse {
            output: text.to_string(),
            persona: self.persona.name().to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: "General conversation".to_string(),
            memory_type: "other".to_string(),
            tags: vec![],
            intent: "unknown".to_string(),
            monologue: None,
            reasoning_summary: None,
        }
    }

    /// Placeholder for future token-aware history trimming
    async fn get_trimmed_history(&self, session_id: &str) -> Result<Vec<ResponseMessage>> {
        let history = self
            .threads
            .get_conversation_capped(session_id, self.config.history_message_cap)
            .await;
        Ok(history)
    }
}

/// Structured reply format from GPT-5
#[derive(Debug, Deserialize, Clone)]
struct MiraStructuredReply {
    reply: String,
    persona: Option<String>,
    mood: Option<String>,
    salience: Option<f64>,
    summary: Option<String>,
    memory_type: Option<String>,
    tags: Option<Vec<String>>,
    intent: Option<String>,
    monologue: Option<String>,
    reasoning_summary: Option<String>,
}
