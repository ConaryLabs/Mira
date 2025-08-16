// src/services/chat.rs
// Updated for GPT-5 Responses API - August 15, 2025
// Changes:
// - Implemented vector store retrieval in build_context (removed TODO)
// - Added previous_response_id support through ResponsesManager
// - Integrated tool calling capabilities
// - Added configuration for vector search
// - Integrated summarization service call

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, debug, warn, instrument};

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::{ThreadManager, ResponseMessage};
use crate::llm::responses::vector_store::VectorStoreManager;
use crate::llm::responses::manager::ResponsesManager;
use crate::llm::responses::types::Message;
use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
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
pub struct ChatConfig { // --- FIXED: Made this struct public ---
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub max_vector_search_results: usize,
    pub enable_vector_search: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_debug_logging: bool,
    pub enable_summarization: bool,
    pub summary_chunk_size: usize,
    pub summary_token_limit: usize,
    pub summary_output_tokens: usize,
}

impl ChatConfig {
    pub fn from_env() -> Self {
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
                .unwrap_or(128000),
            history_message_cap: std::env::var("MIRA_HISTORY_MESSAGE_CAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            history_token_limit: std::env::var("MIRA_HISTORY_TOKEN_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8000),
            max_retrieval_tokens: std::env::var("MIRA_MAX_RETRIEVAL_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2000),
            max_vector_search_results: std::env::var("MIRA_MAX_VECTOR_RESULTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            enable_vector_search: std::env::var("MIRA_ENABLE_VECTOR_SEARCH")
                .unwrap_or_else(|_| "true".to_string())
                .parse::<bool>()
                .unwrap_or(true),
            enable_web_search: std::env::var("MIRA_ENABLE_WEB_SEARCH")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
            enable_code_interpreter: std::env::var("MIRA_ENABLE_CODE_INTERPRETER")
                .unwrap_or_else(|_| "false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
            enable_debug_logging,
            enable_summarization: std::env::var("MIRA_ENABLE_SUMMARIZATION")
                .unwrap_or_else(|_| "true".to_string())
                .parse::<bool>()
                .unwrap_or(true),
            summary_chunk_size: std::env::var("MIRA_SUMMARY_CHUNK_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(6),
            summary_token_limit: std::env::var("MIRA_SUMMARY_TOKEN_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2000),
            summary_output_tokens: std::env::var("MIRA_SUMMARY_OUTPUT_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(512),
        }
    }
}

/// Main chat service orchestrator
pub struct ChatService {
    client: Arc<OpenAIClient>,
    responses_manager: Arc<ResponsesManager>,
    threads: Arc<ThreadManager>,
    memory_service: Arc<MemoryService>,
    vector_store_manager: Arc<VectorStoreManager>,
    summarization_service: Arc<SummarizationService>,
    persona: PersonaOverlay,
    config: Arc<ChatConfig>,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        threads: Arc<ThreadManager>,
        memory_service: Arc<MemoryService>,
        vector_store_manager: Arc<VectorStoreManager>,
        persona: PersonaOverlay,
    ) -> Self {
        let config = Arc::new(ChatConfig::from_env());
        
        let responses_manager = Arc::new(
            ResponsesManager::with_thread_manager(client.clone(), threads.clone())
        );

        let summarization_service = Arc::new(SummarizationService::new(
            threads.clone(),
            memory_service.clone(),
            client.clone(),
            config.clone(),
        ));
        
        Self {
            client,
            responses_manager,
            threads,
            memory_service,
            vector_store_manager,
            summarization_service,
            persona,
            config,
        }
    }

    #[instrument(skip(self))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
        return_structured: bool,
    ) -> Result<ChatResponse> {
        let start_time = Instant::now();
        info!("💬 Starting chat for session: {}", session_id);

        self.memory_service
            .save_user_message(session_id, user_text, project_id)
            .await?;

        self.threads.add_message(session_id, ResponseMessage {
            role: "user".to_string(),
            content: Some(user_text.to_string()),
            name: None,
            function_call: None,
            tool_calls: None,
        }).await?;

        if self.config.enable_summarization {
            debug!("Checking for summarization trigger");
            self.summarization_service
                .summarize_if_needed(session_id)
                .await?;
        }

        let history = self.threads
            .get_conversation_capped(session_id, self.config.history_message_cap)
            .await;
        info!("📜 History: {} messages", history.len());

        let context = self.build_context(session_id, user_text, project_id).await?;

        let gpt5_response = self.respond_gpt5(
            session_id,
            user_text,
            &context,
            return_structured,
        ).await?;

        self.threads.add_message(session_id, ResponseMessage {
            role: "assistant".to_string(),
            content: Some(gpt5_response.output.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        }).await?;

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
        let mut total_tokens = 0;

        let recent_context = self.memory_service
            .get_recent_context(session_id, 4)
            .await?;
        
        if !recent_context.is_empty() {
            let context_strings: Vec<String> = recent_context
                .iter()
                .map(|entry| entry.content.clone())
                .collect();
            let recent_text = context_strings.join("\n");
            total_tokens += recent_text.len() / 4;
            context_parts.push(format!("Recent context:\n{}", recent_text));
        }

        let embedding = self.client.get_embedding(user_text).await?;
        let similar_memories = self.memory_service
            .search_similar(session_id, &embedding, 3)
            .await?;
        
        if !similar_memories.is_empty() {
            let memories_text = similar_memories
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect::<Vec<_>>()
                .join("\n");
            total_tokens += memories_text.len() / 4;
            context_parts.push(format!("Related memories:\n{}", memories_text));
        }

        if self.config.enable_vector_search && project_id.is_some() {
            debug!("🔍 Searching vector store for project: {:?}", project_id);
            
            match self.vector_store_manager
                .search_documents(
                    project_id,
                    user_text,
                    self.config.max_vector_search_results,
                )
                .await {
                Ok(vector_results) if !vector_results.is_empty() => {
                    info!("📚 Found {} relevant documents", vector_results.len());
                    
                    let mut doc_content = String::new();
                    let mut doc_tokens = 0;
                    
                    for (idx, result) in vector_results.iter().enumerate() {
                        let content_preview = if result.content.len() > 500 {
                            format!("{}...", &result.content[..500])
                        } else {
                            result.content.clone()
                        };
                        
                        let entry_tokens = content_preview.len() / 4;
                        
                        if doc_tokens + entry_tokens > self.config.max_retrieval_tokens {
                            debug!("Reached retrieval token limit, stopping at {} documents", idx);
                            break;
                        }
                        
                        doc_content.push_str(&format!(
                            "Document {} (score: {:.2}): {}\n",
                            idx + 1,
                            result.score,
                            content_preview
                        ));
                        doc_tokens += entry_tokens;
                    }
                    
                    if !doc_content.is_empty() {
                        total_tokens += doc_tokens;
                        context_parts.push(format!("Relevant documents:\n{}", doc_content));
                    }
                },
                Ok(_) => {
                    debug!("No relevant documents found in vector store");
                },
                Err(e) => {
                    warn!("Vector store search failed: {}", e);
                }
            }
        }

        debug!("📊 Context built with ~{} tokens across {} parts", total_tokens, context_parts.len());

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
        let history = self.get_trimmed_history(session_id).await?;
        let input = self.build_gpt5_input(history, user_text, context, return_structured);

        let instructions = if return_structured {
            format!(
                "{}\n\nIMPORTANT: You must respond with a valid JSON object containing: reply, mood, salience, summary, memory_type, tags, intent, and optionally monologue and reasoning_summary.",
                self.persona.prompt()
            )
        } else {
            self.build_instructions()
        };

        let parameters = ResponsesManager::build_gpt5_parameters(
            &self.config.verbosity,
            &self.config.reasoning_effort,
            Some(self.config.max_output_tokens as i32),
            None,
        );

        let response_format = if return_structured {
            Some(serde_json::json!({ "type": "json_object" }))
        } else {
            None
        };

        let tools = if self.config.enable_web_search || self.config.enable_code_interpreter {
            Some(ResponsesManager::build_standard_tools(
                self.config.enable_web_search,
                self.config.enable_code_interpreter,
            ))
        } else {
            None
        };

        let response_text = self.responses_manager
            .create_response_with_context(
                &self.config.model,
                input,
                Some(instructions),
                Some(session_id),
                response_format,
                Some(parameters),
                tools,
            )
            .await?;

        if return_structured {
            self.parse_structured_response(&response_text)
        } else {
            Ok(self.build_simple_response(response_text))
        }
    }

    /// Build input messages for GPT-5
    fn build_gpt5_input(
        &self,
        history: Vec<ResponseMessage>,
        user_text: &str,
        context: &str,
        return_structured: bool,
    ) -> Vec<Message> {
        let mut input = Vec::new();

        let system_content = if return_structured {
            format!(
                "{}\n\nContext:\n{}\n\nRespond with valid JSON.",
                self.persona.prompt(),
                context
            )
        } else {
            format!(
                "{}\n\nContext:\n{}",
                self.persona.prompt(),
                context
            )
        };

        input.push(Message {
            role: "system".to_string(),
            content: Some(system_content),
            name: None,
            function_call: None,
            tool_calls: None,
        });

        for msg in history {
            input.push(Message {
                role: msg.role,
                content: msg.content,
                name: msg.name,
                function_call: msg.function_call,
                tool_calls: msg.tool_calls,
            });
        }

        input.push(Message {
            role: "user".to_string(),
            content: Some(user_text.to_string()),
            name: None,
            function_call: None,
            tool_calls: None,
        });

        input
    }

    /// Build instructions for the model
    fn build_instructions(&self) -> String {
        format!(
            "{}\n\nBe helpful, accurate, and stay in character.",
            self.persona.prompt()
        )
    }

    /// Parse structured JSON response
    fn parse_structured_response(&self, response_text: &str) -> Result<ChatResponse> {
        let parsed: MiraStructuredReply = serde_json::from_str(response_text)?;
        
        Ok(ChatResponse {
            output: parsed.reply,
            persona: parsed.persona.unwrap_or_else(|| self.persona.name().to_string()),
            mood: parsed.mood.unwrap_or_else(|| "neutral".to_string()),
            salience: parsed.salience.unwrap_or(5.0) as usize,
            summary: parsed.summary.unwrap_or_else(|| "Chat response".to_string()),
            memory_type: parsed.memory_type.unwrap_or_else(|| "conversation".to_string()),
            tags: parsed.tags.unwrap_or_default(),
            intent: parsed.intent.unwrap_or_else(|| "unknown".to_string()),
            monologue: parsed.monologue,
            reasoning_summary: parsed.reasoning_summary,
        })
    }

    /// Build a simple response when not using structured output
    fn build_simple_response(&self, text: String) -> ChatResponse {
        ChatResponse {
            output: text,
            persona: self.persona.name().to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: "Chat response".to_string(),
            memory_type: "conversation".to_string(),
            tags: vec![],
            intent: "unknown".to_string(),
            monologue: None,
            reasoning_summary: None,
        }
    }

    /// Get trimmed history with token awareness
    async fn get_trimmed_history(&self, session_id: &str) -> Result<Vec<ResponseMessage>> {
        let history = self.threads
            .get_conversation_with_token_limit(
                session_id,
                self.config.history_token_limit,
            )
            .await;
        
        let capped = if history.len() > self.config.history_message_cap {
            history[history.len() - self.config.history_message_cap..].to_vec()
        } else {
            history
        };
        
        Ok(capped)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        std::env::set_var("MIRA_MODEL", "gpt-5-mini");
        std::env::set_var("MIRA_ENABLE_VECTOR_SEARCH", "true");
        std::env::set_var("MIRA_SUMMARY_CHUNK_SIZE", "10");
        
        let config = ChatConfig::from_env();
        assert_eq!(config.model, "gpt-5-mini");
        assert!(config.enable_vector_search);
        assert_eq!(config.summary_chunk_size, 10);
        
        std::env::remove_var("MIRA_MODEL");
        std::env::remove_var("MIRA_ENABLE_VECTOR_SEARCH");
        std::env::remove_var("MIRA_SUMMARY_CHUNK_SIZE");
    }
}
