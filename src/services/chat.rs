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
                .unwrap_or(1024),
            history_message_cap: std::env::var("MIRA_HISTORY_MESSAGE_CAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            history_token_limit: std::env::var("MIRA_HISTORY_TOKEN_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8192),
            max_retrieval_tokens: std::env::var("MIRA_MAX_RETRIEVAL_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2000),
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

        // 5) instructions
        let system_instructions = self.build_instructions(&context);

        // 6) model call
        let response = self.client
            .generate_response(user_text, Some(&system_instructions), return_structured)
            .await?;

        // 7) parse
        let parsed = if return_structured {
            self.parse_structured_response(&response.output)?
        } else {
            self.create_default_response(&response.output)
        };

        // 8) save assistant memory
        self.memory_service.save_assistant_response(session_id, &parsed).await?;

        // 9) add assistant turn to thread
        self.threads.add_message(session_id, ResponseMessage {
            role: "assistant".to_string(),
            content: Some(parsed.output.clone()),
        }).await?;

        // 10) enqueue for embedding / similarity
        let embedding = self.client.get_embedding(user_text).await.ok();

        // 11) optional: vector store (left as TODO until text-ingest API exists)
        if embedding.is_some() && parsed.salience >= 3 {
            // TODO: VectorStoreManager ingest-text method (current one expects a file path)
        }

        if self.config.enable_debug_logging {
            info!(
                "📊 Response metadata: mood={} salience={} tags={:?}",
                parsed.mood, parsed.salience, parsed.tags
            );
            info!("⏱️ Processing time: {:?}", start_time.elapsed());
        }

        Ok(parsed)
    }

    /// Build context from memory and vector stores
    async fn build_context(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<String> {
        let mut context_parts = Vec::new();

        // Recent conversation (memory service abstraction)
        let recent_messages = self.memory_service.get_recent_context(session_id, 5).await?;
        if !recent_messages.is_empty() {
            context_parts.push(format!(
                "## Recent Conversation\n{}",
                recent_messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // Similar memories + project docs (via embeddings)
        if let Ok(embedding) = self.client.get_embedding(user_text).await {
            let similar_memories = self.memory_service.search_similar(session_id, &embedding, 3).await?;
            if !similar_memories.is_empty() {
                context_parts.push(format!(
                    "## Related Memories\n{}",
                    similar_memories.iter()
                        .map(|m| format!("- {}", m.content))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }

            if let Some(proj_id) = project_id {
                if let Ok(docs) = self.vector_store_manager.search_documents(Some(proj_id), proj_id, 3).await {
                    if !docs.is_empty() {
                        context_parts.push(format!(
                            "## Project Documents\n{}",
                            docs.iter()
                                .map(|d| format!("- {}", d.content))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ));
                    }
                }
            }
        }

        Ok(context_parts.join("\n\n---\n"))
    }

    /// Build system instructions with persona and context
    fn build_instructions(&self, context: &str) -> String {
        let mut instructions = String::new();

        // Persona
        instructions.push_str(self.persona.prompt());
        instructions.push('\n');
        instructions.push('\n');

        // Core guardrails
        instructions.push_str("You are Mira — empathetic, capable, and consistent.\n");
        instructions.push_str("Remember previous conversations and build on shared context.\n\n");

        if !context.is_empty() {
            instructions.push_str("---\n# Context Information\n");
            instructions.push_str(context);
            instructions.push_str("\n\nUse the above context when relevant.\n");
        }

        instructions
    }

    /// Parse structured response from GPT-5 (expects JSON when return_structured=true)
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
