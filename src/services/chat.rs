// src/services/chat.rs
// Phase 8: Enhanced with configurable parameters and structured logging

use std::env;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn, instrument};

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::{ResponseMessage, ThreadManager};
use crate::llm::responses::vector_store::{VectorStoreManager, SearchResult};
use crate::llm::schema::{ChatResponse, MiraStructuredReply};
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::services::{ContextService, MemoryService};

/// Configuration for chat service parameters
#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: u32,
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub enable_debug_logging: bool,
}

impl ChatConfig {
    /// Load configuration from environment variables with defaults
    pub fn from_env() -> Self {
        let enable_debug = env::var("MIRA_DEBUG_LOGGING")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        Self {
            model: env::var("MIRA_MODEL")
                .unwrap_or_else(|_| "gpt-5".to_string()),
            verbosity: env::var("MIRA_VERBOSITY")
                .unwrap_or_else(|_| "medium".to_string()),
            reasoning_effort: env::var("MIRA_REASONING_EFFORT")
                .unwrap_or_else(|_| "medium".to_string()),
            max_output_tokens: env::var("MIRA_MAX_OUTPUT_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024),
            history_message_cap: env::var("MIRA_HISTORY_MESSAGE_CAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            history_token_limit: env::var("MIRA_HISTORY_TOKEN_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8192),
            max_retrieval_tokens: env::var("MIRA_MAX_RETRIEVAL_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2000),
            enable_debug_logging,
        }
    }
}

#[derive(Clone)]
pub struct ChatService {
    client: Arc<OpenAIClient>,
    threads: Arc<ThreadManager>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
    vector_store_manager: Arc<VectorStoreManager>,
    persona: PersonaOverlay,
    config: ChatConfig,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        threads: Arc<ThreadManager>,
        memory_service: Arc<MemoryService>,
        context_service: Arc<ContextService>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
    ) -> Self {
        let config = ChatConfig::from_env();
        
        // Log configuration at startup
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
            context_service,
            vector_store_manager,
            persona,
            config,
        }
    }

    /// Main unified chat processing method with enhanced logging
    #[instrument(skip(self), fields(session_id = %session_id, project_id = ?project_id))]
    pub async fn process_message(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
        structured_json: bool,
    ) -> Result<ChatResponse> {
        let start_time = Instant::now();
        
        info!("🚀 Processing chat message");
        debug!("User input: {} chars", user_text.len());
        
        // 1. Get or create thread for this session
        let thread_id = self.threads.get_or_create_thread(session_id).await?;
        debug!("Thread ID: {}", thread_id);

        // 2. Add user message to thread
        self.threads
            .add_message(&thread_id, ResponseMessage::user(user_text))
            .await?;

        // 3. Get conversation history with token-based trimming
        let history = self.get_trimmed_history(&thread_id).await?;
        info!("📜 History: {} messages after trimming", history.len());

        // 4. Build context from memory (local Qdrant)
        let embedding_start = Instant::now();
        let embedding = self.client.get_embedding(user_text).await.ok();
        if embedding.is_some() {
            debug!("Embedding generated in {:?}", embedding_start.elapsed());
        }
        
        let personal_context = self.context_service
            .build_context(session_id, embedding.as_deref(), project_id)
            .await?;
        
        if !personal_context.recent.is_empty() || !personal_context.semantic.is_empty() {
            info!("💭 Personal context: {} recent, {} semantic matches", 
                  personal_context.recent.len(), 
                  personal_context.semantic.len());
        }

        // 5. Search vector store for relevant documents (OpenAI)
        let vector_results = self.search_vector_store(user_text, project_id).await?;
        if !vector_results.is_empty() {
            let scores: Vec<f32> = vector_results.iter().map(|r| r.score).collect();
            info!("📚 Vector store: {} results, scores: {:?}", vector_results.len(), scores);
        }

        // 6. Combine all context sources and enrich instructions
        let enriched_instructions = self.build_enriched_instructions(
            &personal_context,
            &vector_results,
        );
        
        if self.config.enable_debug_logging {
            debug!("Instructions length: {} chars", enriched_instructions.len());
        }

        // 7. Build GPT-5 input messages
        let input_messages = self.build_gpt5_input(&history)?;
        
        // 8. Set up parameters for GPT-5
        let parameters = json!({
            "verbosity": self.config.verbosity,
            "reasoning_effort": self.config.reasoning_effort,
            "max_output_tokens": self.config.max_output_tokens,
            "persona": self.persona.name(),
            "temperature": self.persona.temperature()
        });

        info!("🤖 Calling GPT-5 with parameters: verbosity={}, reasoning={}, max_tokens={}", 
              self.config.verbosity, self.config.reasoning_effort, self.config.max_output_tokens);

        // 9. Request structured JSON if needed
        let response_format = if structured_json {
            json!({ 
                "type": "json_object",
                "schema": self.get_response_schema()
            })
        } else {
            json!({ "type": "text" })
        };

        // 10. Make the GPT-5 API call
        let api_start = Instant::now();
        let body = json!({
            "model": self.config.model,
            "input": input_messages,
            "instructions": enriched_instructions,
            "parameters": parameters,
            "response_format": response_format
        });

        let v = self.client.post_response(body).await?;
        let api_duration = api_start.elapsed();
        
        info!("✅ GPT-5 responded in {:?}", api_duration);
        
        // Log token usage if available
        if let Some(usage) = v.get("usage") {
            if let (Some(prompt_tokens), Some(completion_tokens)) = 
                (usage.get("prompt_tokens").and_then(|t| t.as_u64()),
                 usage.get("completion_tokens").and_then(|t| t.as_u64())) {
                info!("📊 Token usage: prompt={}, completion={}, total={}", 
                      prompt_tokens, completion_tokens, prompt_tokens + completion_tokens);
            }
        }

        // 11. Extract and parse the response
        let (chat_response, raw_text) = self.parse_response(&v, structured_json)?;
        
        info!("💬 Response: salience={}/10, mood={}, tags={:?}", 
              chat_response.salience, chat_response.mood, chat_response.tags);

        // 12. Save assistant response to thread
        self.threads
            .add_message(&thread_id, ResponseMessage::assistant(&raw_text))
            .await?;

        // 13. Evaluate and save to memory if salience is high enough
        if chat_response.salience >= 7 {
            info!("💾 High-salience response ({}), saving to memory", chat_response.salience);
            
            let memory_reply = MiraStructuredReply {
                output: chat_response.output.clone(),
                persona: chat_response.persona.clone(),
                mood: chat_response.mood.clone(),
                salience: chat_response.salience,
                summary: chat_response.summary.clone(),
                memory_type: chat_response.memory_type.clone(),
                tags: chat_response.tags.clone(),
                intent: chat_response.intent.clone(),
                monologue: chat_response.monologue.clone(),
                reasoning_summary: chat_response.reasoning_summary.clone(),
                aside_intensity: chat_response.aside_intensity,
            };

            self.memory_service
                .evaluate_and_save_response(session_id, &memory_reply, project_id)
                .await?;
        }

        let total_duration = start_time.elapsed();
        info!("⏱️ Total processing time: {:?}", total_duration);

        Ok(chat_response)
    }

    /// Get conversation history with token-based trimming
    async fn get_trimmed_history(&self, thread_id: &str) -> Result<Vec<ResponseMessage>> {
        let full_history = self.threads.get_conversation(thread_id).await;
        
        // First apply message count cap
        let mut history = if full_history.len() > self.config.history_message_cap {
            let start = full_history.len() - self.config.history_message_cap;
            full_history[start..].to_vec()
        } else {
            full_history
        };

        // Then apply token limit trimming
        let mut total_tokens = 0;
        let mut keep_from = 0;
        
        // Count from most recent backwards
        for (i, msg) in history.iter().enumerate().rev() {
            let msg_tokens = self.estimate_tokens(&msg.content.as_deref().unwrap_or(""));
            total_tokens += msg_tokens;
            
            if total_tokens > self.config.history_token_limit {
                keep_from = i + 1;
                break;
            }
        }
        
        if keep_from > 0 {
            debug!("Trimming history: removing {} oldest messages to stay under token limit", keep_from);
            history = history[keep_from..].to_vec();
        }
        
        Ok(history)
    }

    /// Estimate token count (rough approximation)
    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough estimate: 1 token ≈ 4 characters
        text.len() / 4
    }

    /// Search vector store for relevant documents
    async fn search_vector_store(
        &self,
        query: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        if project_id.is_none() {
            debug!("No project ID, skipping vector store search");
            return Ok(vec![]);
        }

        debug!("Searching vector store for project: {:?}", project_id);
        
        let search_start = Instant::now();
        let results = self.vector_store_manager
            .search_documents(project_id, query, 5)
            .await
            .unwrap_or_else(|e| {
                warn!("Vector store search failed: {:?}", e);
                vec![]
            });

        if !results.is_empty() {
            debug!("Vector search completed in {:?}", search_start.elapsed());
        }

        Ok(results)
    }

    /// Build enriched instructions with all context sources
    fn build_enriched_instructions(
        &self,
        personal_context: &RecallContext,
        vector_results: &[SearchResult],
    ) -> String {
        let mut instructions = self.persona.prompt();
        
        let mut context_parts = Vec::new();
        let mut total_tokens = 0;
        
        // Add vector store results first (highest priority)
        if !vector_results.is_empty() {
            let mut doc_parts = Vec::new();
            for result in vector_results.iter().take(3) {
                let formatted = result.format_for_context();
                let estimated_tokens = self.estimate_tokens(&formatted);
                
                if total_tokens + estimated_tokens > self.config.max_retrieval_tokens {
                    debug!("Stopping document inclusion due to token limit");
                    break;
                }
                
                doc_parts.push(formatted);
                total_tokens += estimated_tokens;
            }
            
            if !doc_parts.is_empty() {
                context_parts.push(format!(
                    "## Retrieved Documents\n{}",
                    doc_parts.join("\n\n")
                ));
            }
        }
        
        // Add personal memory context (if room remains)
        let remaining_tokens = self.config.max_retrieval_tokens.saturating_sub(total_tokens);
        
        if !personal_context.recent.is_empty() && remaining_tokens > 200 {
            let recent_texts: Vec<String> = personal_context.recent.iter()
                .take(3)
                .map(|m| format!("[{}]: {}", m.role, m.content.chars().take(100).collect::<String>()))
                .collect();
            context_parts.push(format!(
                "## Recent Conversation\n{}",
                recent_texts.join("\n")
            ));
        }

        if !personal_context.semantic.is_empty() && remaining_tokens > 400 {
            let semantic_texts: Vec<String> = personal_context.semantic.iter()
                .take(3)
                .map(|m| format!("- {}", m.content.chars().take(100).collect::<String>()))
                .collect();
            context_parts.push(format!(
                "## Related Memories\n{}",
                semantic_texts.join("\n")
            ));
        }

        // Append all context to instructions
        if !context_parts.is_empty() {
            instructions.push_str("\n\n---\n# Context Information\n");
            instructions.push_str(&context_parts.join("\n\n"));
            instructions.push_str("\n\nUse the above context to inform your response when relevant.");
        }

        instructions
    }

    /// Parse response from GPT-5
    fn parse_response(&self, v: &Value, structured_json: bool) -> Result<(ChatResponse, String)> {
        if structured_json {
            let text = extract_output_text(v)
                .or_else(|| extract_message_text(v))
                .unwrap_or_default();
            
            let parsed: MiraStructuredReply = serde_json::from_str(&text)
                .unwrap_or_else(|e| {
                    warn!("Failed to parse structured response: {}", e);
                    self.create_fallback_response(text.clone())
                });

            Ok((parsed.clone(), text))
        } else {
            let text = extract_output_text(v)
                .or_else(|| extract_message_text(v))
                .unwrap_or_default();
            
            let response = ChatResponse {
                output: text.clone(),
                persona: self.persona.name().to_string(),
                mood: "neutral".to_string(),
                salience: 5,
                summary: None,
                memory_type: "event".to_string(),
                tags: vec![],
                intent: "response".to_string(),
                monologue: None,
                reasoning_summary: None,
                aside_intensity: None,
            };

            Ok((response, text))
        }
    }

    /// Create fallback response when parsing fails
    fn create_fallback_response(&self, text: String) -> MiraStructuredReply {
        MiraStructuredReply {
            output: text,
            persona: self.persona.name().to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: Some("Response parsing failed".to_string()),
            memory_type: "event".to_string(),
            tags: vec![],
            intent: "response".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        }
    }

    /// Get JSON schema for structured responses
    fn get_response_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "output": { "type": "string" },
                "persona": { "type": "string" },
                "mood": { "type": "string" },
                "salience": { "type": "number", "minimum": 1, "maximum": 10 },
                "summary": { "type": ["string", "null"] },
                "memory_type": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "intent": { "type": "string" },
                "monologue": { "type": ["string", "null"] },
                "reasoning_summary": { "type": ["string", "null"] },
                "aside_intensity": { "type": ["number", "null"] }
            },
            "required": ["output", "persona", "mood", "salience", "memory_type", "tags", "intent"]
        })
    }

    /// Legacy method for backward compatibility
    pub async fn process_message_gpt5(
        &self,
        thread_id: &str,
        user_text: &str,
        structured_json: bool,
    ) -> Result<ChatResult> {
        let response = self.process_message(thread_id, user_text, None, structured_json).await?;
        
        Ok(ChatResult {
            thread_id: thread_id.to_string(),
            text: serde_json::to_string(&response).unwrap_or(response.output),
            raw: json!({}),
        })
    }

    fn build_gpt5_input(&self, history: &[ResponseMessage]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(history.len());
        for msg in history {
            let role = if msg.role == "user" { "user" } else { "assistant" };
            let text = msg
                .content
                .as_deref()
                .ok_or_else(|| anyhow!("empty message content in history"))?;
            out.push(json!({
                "role": role,
                "content": [
                    { "type": if role == "user" { "input_text" } else { "output_text" }, "text": text }
                ]
            }));
        }
        Ok(out)
    }
}

pub struct ChatResult {
    pub thread_id: String,
    pub text: String,
    pub raw: Value,
}

fn extract_output_text(v: &Value) -> Option<String> {
    let arr = v.get("output")?.as_array()?;
    let mut buf = String::new();
    for item in arr {
        if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
            if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                buf.push_str(s);
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

fn extract_message_text(v: &Value) -> Option<String> {
    let parts = v.pointer("/choices/0/message/content")?.as_array()?;
    let mut buf = String::new();
    for part in parts {
        if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
            if let Some(s) = part.get("text").and_then(|t| t.as_str()) {
                buf.push_str(s);
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

// Convenience constructors for ResponseMessage
impl ResponseMessage {
    pub fn user(text: &str) -> Self {
        Self { role: "user".into(), content: Some(text.to_string()) }
    }
    pub fn assistant(text: &str) -> Self {
        Self { role: "assistant".into(), content: Some(text.to_string()) }
    }
}
