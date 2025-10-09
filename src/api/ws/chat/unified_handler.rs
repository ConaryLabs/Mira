// src/api/ws/chat/unified_handler.rs
// Unified chat handler - GPT-5 native structured outputs

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{info, warn, debug};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, LLMMetadata};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::{StructuredLLMResponse, MessageAnalysis};
use crate::llm::provider::Message;
use crate::llm::router::TaskType;
use crate::memory::storage::sqlite::structured_ops::{save_structured_response, process_embeddings};
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
}

// ===== SESSION TOOL CACHE =====

#[derive(Clone)]
struct CachedToolResult {
    result: Value,
    cached_at: Instant,
}

struct SessionToolCache {
    cache: HashMap<(String, String), CachedToolResult>,
    project_context_ttl: Duration,
}

impl SessionToolCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            project_context_ttl: Duration::from_secs(300),
        }
    }
    
    fn get(&self, project_id: &str, tool_name: &str, ttl: Duration) -> Option<Value> {
        let key = (project_id.to_string(), tool_name.to_string());
        
        if let Some(cached) = self.cache.get(&key) {
            if cached.cached_at.elapsed() < ttl {
                debug!("Cache HIT for {}:{}", tool_name, project_id);
                return Some(cached.result.clone());
            }
        }
        None
    }
    
    fn set(&mut self, project_id: &str, tool_name: &str, result: Value) {
        let key = (project_id.to_string(), tool_name.to_string());
        self.cache.insert(key, CachedToolResult {
            result,
            cached_at: Instant::now(),
        });
    }
    
    fn is_cacheable(&self, tool_name: &str) -> bool {
        matches!(tool_name, "get_project_context")
    }
    
    fn get_ttl(&self, tool_name: &str) -> Duration {
        match tool_name {
            "get_project_context" => self.project_context_ttl,
            _ => Duration::from_secs(0),
        }
    }
}

pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    pub async fn handle_message(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        info!("Processing message for session: {}", request.session_id);
        self.handle_chat_with_tools(request).await
    }
    
    async fn handle_chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        // Save user message
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // Build context
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Define optional tools (file ops, code search, etc.)
        // GPT-5 will use these only when needed
        let tools = if request.project_id.is_some() {
            vec![
                get_create_artifact_tool_schema(),
                get_code_fix_tool_schema(),
                get_read_file_tool_schema(),
                get_code_search_tool_schema(),
                get_list_files_tool_schema(),
                get_project_context_tool_schema(),
                get_read_files_tool_schema(),
                get_write_files_tool_schema(),
            ]
        } else {
            vec![]
        };
        debug!("Available tools: {}", tools.len());
        
        // Build message history
        let mut chat_messages = Vec::new();
        for entry in context.recent.iter().rev() {
            chat_messages.push(Message {
                role: if entry.role == "user" { "user".to_string() } else { "assistant".to_string() },
                content: entry.content.clone(),
            });
        }
        chat_messages.push(Message {
            role: "user".to_string(),
            content: request.content.clone(),
        });
        
        // Initialize cache and artifacts
        let mut tool_cache = SessionToolCache::new();
        let mut collected_artifacts: Vec<Value> = Vec::new();
        
        info!("🎙️ Mira (GPT-5) processing request");
        
        // Tool execution loop - continue until response is complete
        for iteration in 0..10 {
            info!("Iteration {}", iteration);
            
            // Call GPT-5 with structured output + optional tools
            let raw_response = self.app_state.llm_router.chat_with_tools(
                TaskType::Chat,
                chat_messages.clone(),
                system_prompt.clone(),
                tools.clone(),
                None,
            ).await?;
            
            // Log tokens
            info!(
                "🤖 GPT-5 | Tokens: in={} out={} reasoning={} | latency={}ms",
                raw_response.tokens.input,
                raw_response.tokens.output,
                raw_response.tokens.reasoning,
                raw_response.latency_ms
            );
            
            // DEBUG: Log what we got
            debug!("text_output length: {}", raw_response.text_output.len());
            debug!("text_output preview: {}", &raw_response.text_output[..raw_response.text_output.len().min(200)]);
            debug!("raw_response keys: {:?}", raw_response.raw_response.as_object().map(|o| o.keys().collect::<Vec<_>>()));
            
            // Parse structured JSON response
            let structured = self.parse_structured_output(&raw_response.text_output)?;
            
            // If no tool calls, we're done - return the response
            if raw_response.function_calls.is_empty() {
                info!("Response complete (no tool calls needed)");
                
                let metadata = LLMMetadata {
                    response_id: Some(raw_response.id.clone()),
                    prompt_tokens: Some(raw_response.tokens.input),
                    completion_tokens: Some(raw_response.tokens.output),
                    thinking_tokens: Some(raw_response.tokens.reasoning),
                    total_tokens: Some(raw_response.tokens.input + raw_response.tokens.output + raw_response.tokens.reasoning),
                    model_version: "gpt-5".to_string(),
                    finish_reason: Some("stop".to_string()),
                    latency_ms: raw_response.latency_ms,
                    temperature: 0.7,
                    max_tokens: 128000,
                };
                
                let complete_response = CompleteResponse {
                    structured,
                    metadata,
                    raw_response: raw_response.raw_response.clone(),
                    artifacts: if collected_artifacts.is_empty() { None } else { Some(collected_artifacts) },
                };
                
                // Save to database
                let message_id = save_structured_response(
                    &self.app_state.sqlite_pool,
                    &request.session_id,
                    &complete_response,
                    Some(user_message_id),
                ).await?;
                
                // Process embeddings
                if let Err(e) = process_embeddings(
                    &self.app_state.sqlite_pool,
                    message_id,
                    &request.session_id,
                    &complete_response.structured,
                    &self.app_state.embedding_client,
                    &self.app_state.memory_service.get_multi_store(),
                ).await {
                    warn!("Failed to process embeddings: {}", e);
                }
                
                return Ok(complete_response);
            }
            
            // Execute tool calls
            info!("Executing {} tools", raw_response.function_calls.len());
            
            for func_call in &raw_response.function_calls {
                let tool_name = &func_call.name;
                let tool_input = &func_call.arguments;
                
                debug!("Tool: {}", tool_name);
                
                // Execute tool with caching
                let result = if let Some(project_id) = request.project_id.as_deref() {
                    if tool_cache.is_cacheable(tool_name) {
                        let ttl = tool_cache.get_ttl(tool_name);
                        
                        if let Some(cached) = tool_cache.get(project_id, tool_name, ttl) {
                            Ok(cached)
                        } else {
                            match self.execute_tool(tool_name, tool_input, &request).await {
                                Ok(r) => {
                                    tool_cache.set(project_id, tool_name, r.clone());
                                    Ok(r)
                                }
                                Err(e) => Err(e)
                            }
                        }
                    } else {
                        self.execute_tool(tool_name, tool_input, &request).await
                    }
                } else {
                    self.execute_tool(tool_name, tool_input, &request).await
                };
                
                // Handle result
                let result_value = match result {
                    Ok(r) => {
                        // Collect artifacts
                        if tool_name == "create_artifact" {
                            if let Some(artifact) = r.get("artifact") {
                                collected_artifacts.push(artifact.clone());
                            }
                        } else if tool_name == "provide_code_fix" {
                            if let Some(artifacts_array) = r.get("artifacts").and_then(|a| a.as_array()) {
                                collected_artifacts.extend(artifacts_array.iter().cloned());
                            }
                        }
                        r
                    }
                    Err(e) => {
                        warn!("Tool error: {}", e);
                        json!({
                            "error": e.to_string(),
                            "status": "failed"
                        })
                    }
                };
                
                // Add tool result to conversation
                chat_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result ({}): {}", tool_name, result_value.to_string()),
                });
            }
        }
        
        Err(anyhow!("Tool loop exceeded max iterations"))
    }
    
    /// Parse GPT-5's structured JSON output
    fn parse_structured_output(&self, text_output: &str) -> Result<StructuredLLMResponse> {
        let parsed: Value = serde_json::from_str(text_output)
            .map_err(|e| anyhow!("Failed to parse structured output: {}", e))?;
        
        let output = parsed["output"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'output' field"))?
            .to_string();
        
        let analysis_obj = parsed["analysis"]
            .as_object()
            .ok_or_else(|| anyhow!("Missing 'analysis' field"))?;
        
        let analysis = MessageAnalysis {
            salience: analysis_obj["salience"].as_f64().unwrap_or(0.5),
            topics: analysis_obj["topics"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["general".to_string()]),
            contains_code: analysis_obj["contains_code"].as_bool().unwrap_or(false),
            programming_lang: analysis_obj["programming_lang"].as_str().map(String::from),
            contains_error: analysis_obj["contains_error"].as_bool().unwrap_or(false),
            error_type: analysis_obj["error_type"].as_str().map(String::from),
            routed_to_heads: analysis_obj["routed_to_heads"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["semantic".to_string()]),
            language: analysis_obj["language"]
                .as_str()
                .unwrap_or("en")
                .to_string(),
            mood: None,
            intensity: None,
            intent: None,
            summary: None,
            relationship_impact: None,
            error_file: None,
            error_severity: None,
        };
        
        Ok(StructuredLLMResponse {
            output,
            analysis,
            reasoning: None,
            schema_name: Some("gpt5_structured".to_string()),
            validation_status: Some("valid".to_string()),
        })
    }
    
    async fn execute_tool(
        &self,
        tool_name: &str,
        input: &Value,
        request: &ChatRequest,
    ) -> Result<Value> {
        let executor = crate::tools::ToolExecutor::new(
            self.app_state.code_intelligence.clone(),
            self.app_state.sqlite_pool.clone(),
            self.app_state.llm_router.clone(),
        );

        let project_id = request.project_id.as_deref()
            .ok_or_else(|| anyhow!("No project context for {}", tool_name))?;

        executor.execute_tool(tool_name, input, project_id).await
    }
    
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(
                &request.session_id,
                &request.content,
                request.project_id.as_deref()
            )
            .await
    }
    
    async fn build_context(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> Result<RecallContext> {
        let mut context = self.app_state.memory_service
            .parallel_recall_context(session_id, user_message, 20, 15)
            .await?;
        
        context.rolling_summary = self.app_state.memory_service
            .get_rolling_summary(session_id)
            .await?;
        
        context.session_summary = self.app_state.memory_service
            .get_session_summary(session_id)
            .await?;
        
        Ok(context)
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
}
