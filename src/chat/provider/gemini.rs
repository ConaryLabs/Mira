//! Gemini provider for Studio chat (Orchestrator mode)
//!
//! Uses Gemini's generateContent API with function calling.
//! Supports both Flash (cheap, fast) and Pro (complex reasoning) models.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    Capabilities, ChatRequest, ChatResponse, FinishReason, GroundingSource, Provider,
    StreamEvent, ToolCall, ToolContinueRequest, ToolDefinition, UrlFetchResult, Usage,
};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const GEMINI_CACHE_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/cachedContents";
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Minimum tokens required for caching
const FLASH_MIN_CACHE_TOKENS: u32 = 1_024;
const PRO_MIN_CACHE_TOKENS: u32 = 4_096;

// ============================================================================
// Model Selection
// ============================================================================

/// Gemini 3 model variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeminiModel {
    /// Flash: Pro-level intelligence at Flash speed and pricing ($0.50/$3 per 1M)
    /// Best for: simple queries, file ops, search, memory operations
    #[default]
    Flash,
    /// Pro: Complex reasoning and advanced planning ($2/$12 per 1M)
    /// Best for: goal, task, multi-step chains
    Pro,
}

impl GeminiModel {
    /// Get the model ID for the API
    /// Using explicit Gemini 3 preview models (latest aliases still point to 2.5)
    pub fn model_id(&self) -> &'static str {
        match self {
            Self::Flash => "gemini-3-flash-preview",
            Self::Pro => "gemini-3-pro-preview",
        }
    }

    /// Select thinking level based on model and tool configuration.
    ///
    /// Thinking levels:
    /// - Flash: minimal/low/medium/high (use medium as balanced default)
    /// - Pro: low/high only
    ///
    /// Strategy:
    /// - Flash: "medium" for balanced reasoning (works well for most tasks)
    /// - Pro: "low" with tools, "high" without
    pub fn select_thinking_level(&self, has_tools: bool, _tool_count: usize) -> &'static str {
        match self {
            Self::Flash => "medium", // Balanced default for Gemini 3 Flash
            Self::Pro => {
                if has_tools { "low" } else { "high" }
            }
        }
    }

    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Flash => "Gemini 3 Flash",
            Self::Pro => "Gemini 3 Pro",
        }
    }

    /// Build the generateContent URL for this model
    fn generate_url(&self) -> String {
        format!("{}/{}:generateContent", GEMINI_API_BASE, self.model_id())
    }

    /// Build the streamGenerateContent URL for this model
    fn stream_url(&self) -> String {
        format!("{}/{}:streamGenerateContent", GEMINI_API_BASE, self.model_id())
    }

    /// Minimum tokens required for caching
    pub fn min_cache_tokens(&self) -> u32 {
        match self {
            Self::Flash => FLASH_MIN_CACHE_TOKENS,
            Self::Pro => PRO_MIN_CACHE_TOKENS,
        }
    }

    /// Full model path for cache API
    fn model_path(&self) -> String {
        format!("models/{}", self.model_id())
    }
}

/// Gemini 3 provider for chat interface (Flash or Pro)
pub struct GeminiChatProvider {
    client: HttpClient,
    api_key: String,
    model: GeminiModel,
    capabilities: Capabilities,
}

impl GeminiChatProvider {
    /// Create a new Gemini Chat provider with specified model
    pub fn new(api_key: String, model: GeminiModel) -> Self {
        let capabilities = match model {
            GeminiModel::Flash => Capabilities::gemini_3_flash(),
            GeminiModel::Pro => Capabilities::gemini_3_pro(),
        };
        Self {
            client: HttpClient::new(),
            api_key,
            model,
            capabilities,
        }
    }

    /// Create Flash provider from environment variable (default, cheap)
    pub fn flash() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;
        Ok(Self::new(api_key, GeminiModel::Flash))
    }

    /// Create Pro provider from environment variable (for complex reasoning)
    pub fn pro() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;
        Ok(Self::new(api_key, GeminiModel::Pro))
    }

    /// Create from environment variable (defaults to Flash)
    pub fn from_env() -> Result<Self> {
        Self::flash()
    }

    /// Get the current model
    pub fn model(&self) -> GeminiModel {
        self.model
    }

    /// Build Gemini contents from chat request
    fn build_contents(request: &ChatRequest) -> Vec<GeminiContent> {
        let mut contents = Vec::new();

        // Add history messages
        for msg in &request.messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => continue, // Skip system/tool messages in history
            };
            contents.push(GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart::Text { text: msg.content.clone() }],
            });
        }

        // Add current user input ONLY if not already the last message
        // (build_fresh() may have already added it to messages)
        let already_in_history = request
            .messages
            .last()
            .map(|m| m.role.as_str() == "user" && m.content == request.input)
            .unwrap_or(false);

        if !already_in_history && !request.input.is_empty() {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text { text: request.input.clone() }],
            });
        }

        contents
    }

    /// Build Gemini contents for tool continuation
    fn build_tool_contents(request: &ToolContinueRequest) -> Vec<GeminiContent> {
        let mut contents = Vec::new();

        // Add history messages
        for msg in &request.messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => continue,
            };
            contents.push(GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart::Text { text: msg.content.clone() }],
            });
        }

        // Add assistant message with tool calls (reconstructed)
        if !request.tool_results.is_empty() {
            let mut parts = Vec::new();
            for result in &request.tool_results {
                parts.push(GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: result.name.clone(),
                        // Note: We use empty args here since we don't store original args
                        // Gemini API accepts this for tool result continuation
                        args: Value::Object(Default::default()),
                    },
                    // Include thought_signature if available (required for Gemini 2.5+)
                    thought_signature: result.thought_signature.clone(),
                });
            }
            contents.push(GeminiContent {
                role: "model".to_string(),
                parts,
            });

            // Add tool results as user message with function responses
            let mut response_parts = Vec::new();
            for result in &request.tool_results {
                response_parts.push(GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: result.name.clone(),
                        response: serde_json::json!({ "result": result.output }),
                    },
                });
            }
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: response_parts,
            });
        }

        contents
    }

    /// Convert tool definitions to Gemini format, including built-in tools
    fn build_tools(tools: &[ToolDefinition]) -> Option<Vec<GeminiToolEntry>> {
        Self::build_tools_with_stores(tools, &[])
    }

    /// Build tools with optional FileSearch stores for RAG
    ///
    /// IMPORTANT: Gemini 3 does NOT support combining built-in tools (google_search,
    /// code_execution, url_context) with custom function declarations. We must choose:
    /// - Custom functions (Mira tools) when tools are provided
    /// - Built-in tools only when no custom tools are needed
    fn build_tools_with_stores(tools: &[ToolDefinition], file_search_stores: &[String]) -> Option<Vec<GeminiToolEntry>> {
        let mut entries = Vec::new();

        // Add custom function declarations if any
        // When using custom tools, we CANNOT use built-in tools (Gemini 3 limitation)
        if !tools.is_empty() {
            let declarations: Vec<GeminiFunctionDeclaration> = tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                })
                .collect();
            entries.push(GeminiToolEntry::Functions { function_declarations: declarations });

            // FileSearch can work with custom functions
            if !file_search_stores.is_empty() {
                entries.push(GeminiToolEntry::FileSearch {
                    file_search: FileSearchConfig {
                        file_search_store_names: file_search_stores.to_vec(),
                    },
                });
            }
        } else {
            // No custom functions - we can use all built-in tools
            entries.push(GeminiToolEntry::GoogleSearch { google_search: EmptyObject {} });
            entries.push(GeminiToolEntry::CodeExecution { code_execution: EmptyObject {} });
            entries.push(GeminiToolEntry::UrlContext { url_context: EmptyObject {} });

            if !file_search_stores.is_empty() {
                entries.push(GeminiToolEntry::FileSearch {
                    file_search: FileSearchConfig {
                        file_search_store_names: file_search_stores.to_vec(),
                    },
                });
            }
        }

        if entries.is_empty() {
            None
        } else {
            Some(entries)
        }
    }

    /// Process streaming response data and send events
    async fn process_stream_response(
        response: &GeminiResponse,
        tx: &mpsc::Sender<StreamEvent>,
        tool_call_count: &mut usize,
    ) {
        if let Some(candidates) = &response.candidates {
            for candidate in candidates {
                // Track code execution across parts
                let mut exec_code: Option<&GeminiExecutableCode> = None;
                let mut exec_result: Option<&GeminiCodeExecutionResult> = None;

                for part in &candidate.content.parts {
                    if let Some(text) = &part.text {
                        let _ = tx.send(StreamEvent::TextDelta(text.clone())).await;
                    }
                    if let Some(fc) = &part.function_call {
                        let call_id = format!("gemini_{}", *tool_call_count);
                        *tool_call_count += 1;
                        let _ = tx.send(StreamEvent::FunctionCallStart {
                            call_id: call_id.clone(),
                            name: fc.name.clone(),
                            thought_signature: part.thought_signature.clone(),
                        }).await;
                        let _ = tx.send(StreamEvent::FunctionCallDelta {
                            call_id: call_id.clone(),
                            arguments_delta: fc.args.to_string(),
                        }).await;
                        let _ = tx.send(StreamEvent::FunctionCallEnd {
                            call_id,
                        }).await;
                    }
                    // Track code execution parts
                    if let Some(code) = &part.executable_code {
                        exec_code = Some(code);
                    }
                    if let Some(result) = &part.code_execution_result {
                        exec_result = Some(result);
                    }
                }

                // Emit code execution if we have both code and result
                if let (Some(code), Some(result)) = (exec_code, exec_result) {
                    let _ = tx.send(StreamEvent::CodeExecution {
                        language: code.language.clone(),
                        code: code.code.clone(),
                        output: result.output.clone().unwrap_or_default(),
                        outcome: result.outcome.clone(),
                    }).await;
                }

                // Emit grounding metadata if present
                if let Some(grounding) = &candidate.grounding_metadata {
                    if !grounding.web_search_queries.is_empty() || !grounding.grounding_chunks.is_empty() {
                        let sources: Vec<GroundingSource> = grounding.grounding_chunks
                            .iter()
                            .filter_map(|chunk| chunk.web.as_ref().map(|w| GroundingSource {
                                uri: w.uri.clone(),
                                title: w.title.clone(),
                            }))
                            .collect();
                        let _ = tx.send(StreamEvent::GroundingMetadata {
                            search_queries: grounding.web_search_queries.clone(),
                            sources,
                        }).await;
                    }
                }

                // Emit URL context metadata if present
                if let Some(url_ctx) = &candidate.url_context_metadata {
                    if !url_ctx.url_metadata.is_empty() {
                        let urls: Vec<UrlFetchResult> = url_ctx.url_metadata
                            .iter()
                            .map(|u| UrlFetchResult {
                                url: u.retrieved_url.clone(),
                                status: u.url_retrieval_status.clone(),
                            })
                            .collect();
                        let _ = tx.send(StreamEvent::UrlContextMetadata { urls }).await;
                    }
                }
            }
        }
        if let Some(usage) = &response.usage_metadata {
            let _ = tx.send(StreamEvent::Usage(Usage {
                input_tokens: usage.prompt_token_count.unwrap_or(0),
                output_tokens: usage.candidates_token_count.unwrap_or(0),
                reasoning_tokens: 0,
                cached_tokens: usage.cached_content_token_count.unwrap_or(0),
            })).await;
        }
    }

    /// Make a non-streaming request
    async fn make_request(
        &self,
        contents: Vec<GeminiContent>,
        system: Option<String>,
        tools: Option<Vec<GeminiToolEntry>>,
        thinking_level: &str,
    ) -> Result<GeminiResponse> {
        let api_request = GeminiRequest {
            contents,
            system_instruction: system.map(|s| GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: s }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?key={}", self.model.generate_url(), self.api_key);

        let response = self.client
            .post(&url)
            .json(&api_request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error: {} - {}", status, body);
        }

        let api_response: GeminiResponse = response.json().await?;

        if let Some(error) = &api_response.error {
            anyhow::bail!("Gemini error: {}", error.message);
        }

        Ok(api_response)
    }

    /// Parse response into ChatResponse
    fn parse_response(response: GeminiResponse) -> ChatResponse {
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = FinishReason::Stop;

        if let Some(candidates) = response.candidates {
            if let Some(candidate) = candidates.into_iter().next() {
                for part in candidate.content.parts {
                    if let Some(t) = part.text {
                        text.push_str(&t);
                    }
                    if let Some(fc) = part.function_call {
                        finish_reason = FinishReason::ToolCalls;
                        tool_calls.push(ToolCall {
                            call_id: format!("gemini_{}", tool_calls.len()),
                            name: fc.name,
                            arguments: fc.args.to_string(),
                            thought_signature: part.thought_signature.clone(),
                        });
                    }
                }
            }
        }

        let usage = response.usage_metadata.map(|u| Usage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            reasoning_tokens: 0,
            cached_tokens: u.cached_content_token_count.unwrap_or(0),
        });

        ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            reasoning: None,
            tool_calls,
            usage,
            finish_reason,
        }
    }

    // ========================================================================
    // Context Caching
    // ========================================================================

    /// Create a cached context for reuse across requests.
    ///
    /// Returns `Ok(None)` if content is below minimum token threshold.
    /// Cache reduces cost by ~75% on cached input tokens.
    ///
    /// # Arguments
    /// * `system_prompt` - System instructions to cache
    /// * `tools` - Tool definitions to cache (required for tool use with cached content)
    /// * `context` - Optional additional context to cache (e.g., code, docs)
    /// * `ttl_seconds` - Time to live (default 3600 = 1 hour)
    pub async fn create_cache(
        &self,
        system_prompt: &str,
        tools: &[ToolDefinition],
        context: Option<&str>,
        ttl_seconds: u32,
    ) -> Result<Option<CachedContent>> {
        // Rough token estimate (4 chars ≈ 1 token)
        let system_tokens = system_prompt.len() as u32 / 4;
        let context_tokens = context.map(|c| c.len() as u32 / 4).unwrap_or(0);
        let estimated_tokens = system_tokens + context_tokens;

        // Check minimum threshold
        if estimated_tokens < self.model.min_cache_tokens() {
            tracing::debug!(
                "Content too small for caching: {} tokens < {} min",
                estimated_tokens,
                self.model.min_cache_tokens()
            );
            return Ok(None);
        }

        // Build cache request
        let mut contents = Vec::new();
        if let Some(ctx) = context {
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text { text: ctx.to_string() }],
            });
        }

        // Include tools in the cache (required for GenerateContent with cachedContent)
        let gemini_tools = Self::build_tools(tools);

        let request = CreateCacheRequest {
            model: self.model.model_path(),
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: system_prompt.to_string() }],
            }),
            contents,
            tools: gemini_tools,
            ttl: format!("{}s", ttl_seconds),
        };

        let url = format!("{}?key={}", GEMINI_CACHE_BASE, self.api_key);

        let response = self.client
            .post(&url)
            .json(&request)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Cache creation failed: {} - {}", status, body);
        }

        let cache_response: CreateCacheResponse = response.json().await?;
        let token_count = cache_response.usage_metadata
            .and_then(|u| u.total_token_count)
            .unwrap_or(estimated_tokens);

        tracing::info!(
            "Created cache '{}' with {} tokens, expires {}",
            cache_response.name,
            token_count,
            cache_response.expire_time
        );

        Ok(Some(CachedContent {
            name: cache_response.name,
            expire_time: cache_response.expire_time,
            token_count,
        }))
    }

    /// Delete a cached context
    pub async fn delete_cache(&self, cache_name: &str) -> Result<()> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{}?key={}",
            cache_name,
            self.api_key
        );

        let response = self.client
            .delete(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Cache deletion failed: {} - {}", status, body);
        }

        tracing::debug!("Deleted cache '{}'", cache_name);
        Ok(())
    }

    /// Make a streaming request using cached content
    /// Note: tools are included in the cache, not in this request
    pub async fn create_stream_with_cache(
        &self,
        cache: &CachedContent,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        // Only include messages not in the cache (new user messages)
        // Tools are already in the cached content, don't include them here
        let contents = Self::build_contents(&request);
        let thinking_level = self.model.select_thinking_level(!request.tools.is_empty(), request.tools.len());

        let api_request = CachedGeminiRequest {
            cached_content: cache.name.clone(),
            contents,
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
        };

        let url = format!("{}?alt=sse&key={}", self.model.stream_url(), self.api_key);
        let client = self.client.clone();
        let cached_tokens = cache.token_count;

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;
                    let mut usage_sent = false;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(gemini_response) = serde_json::from_str::<GeminiResponse>(data) {
                                            // Process response but intercept usage to add cached_tokens
                                            if let Some(candidates) = &gemini_response.candidates {
                                                for candidate in candidates {
                                                    let mut exec_code: Option<&GeminiExecutableCode> = None;
                                                    let mut exec_result: Option<&GeminiCodeExecutionResult> = None;

                                                    for part in &candidate.content.parts {
                                                        if let Some(text) = &part.text {
                                                            let _ = tx.send(StreamEvent::TextDelta(text.clone())).await;
                                                        }
                                                        if let Some(fc) = &part.function_call {
                                                            let call_id = format!("gemini_{}", tool_call_count);
                                                            tool_call_count += 1;
                                                            let _ = tx.send(StreamEvent::FunctionCallStart {
                                                                call_id: call_id.clone(),
                                                                name: fc.name.clone(),
                                                                thought_signature: part.thought_signature.clone(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallDelta {
                                                                call_id: call_id.clone(),
                                                                arguments_delta: fc.args.to_string(),
                                                            }).await;
                                                            let _ = tx.send(StreamEvent::FunctionCallEnd { call_id }).await;
                                                        }
                                                        if let Some(code) = &part.executable_code {
                                                            exec_code = Some(code);
                                                        }
                                                        if let Some(result) = &part.code_execution_result {
                                                            exec_result = Some(result);
                                                        }
                                                    }

                                                    if let (Some(code), Some(result)) = (exec_code, exec_result) {
                                                        let _ = tx.send(StreamEvent::CodeExecution {
                                                            language: code.language.clone(),
                                                            code: code.code.clone(),
                                                            output: result.output.clone().unwrap_or_default(),
                                                            outcome: result.outcome.clone(),
                                                        }).await;
                                                    }

                                                    if let Some(grounding) = &candidate.grounding_metadata {
                                                        if !grounding.web_search_queries.is_empty() || !grounding.grounding_chunks.is_empty() {
                                                            let sources: Vec<GroundingSource> = grounding.grounding_chunks
                                                                .iter()
                                                                .filter_map(|chunk| chunk.web.as_ref().map(|w| GroundingSource {
                                                                    uri: w.uri.clone(),
                                                                    title: w.title.clone(),
                                                                }))
                                                                .collect();
                                                            let _ = tx.send(StreamEvent::GroundingMetadata {
                                                                search_queries: grounding.web_search_queries.clone(),
                                                                sources,
                                                            }).await;
                                                        }
                                                    }

                                                    // Emit URL context metadata if present
                                                    if let Some(url_ctx) = &candidate.url_context_metadata {
                                                        if !url_ctx.url_metadata.is_empty() {
                                                            let urls: Vec<UrlFetchResult> = url_ctx.url_metadata
                                                                .iter()
                                                                .map(|u| UrlFetchResult {
                                                                    url: u.retrieved_url.clone(),
                                                                    status: u.url_retrieval_status.clone(),
                                                                })
                                                                .collect();
                                                            let _ = tx.send(StreamEvent::UrlContextMetadata { urls }).await;
                                                        }
                                                    }
                                                }
                                            }

                                            // Send usage with cached_tokens
                                            if !usage_sent {
                                                if let Some(usage) = &gemini_response.usage_metadata {
                                                    let _ = tx.send(StreamEvent::Usage(Usage {
                                                        input_tokens: usage.prompt_token_count.unwrap_or(0),
                                                        output_tokens: usage.candidates_token_count.unwrap_or(0),
                                                        reasoning_tokens: 0,
                                                        cached_tokens,
                                                    })).await;
                                                    usage_sent = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    /// Create a streaming request with FileSearch stores for RAG
    pub async fn create_stream_with_file_search(
        &self,
        request: ChatRequest,
        file_search_stores: &[String],
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let contents = Self::build_contents(&request);
        let tools = Self::build_tools_with_stores(&request.tools, file_search_stores);
        let thinking_level = self.model.select_thinking_level(!request.tools.is_empty(), request.tools.len());

        let api_request = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: request.system }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?alt=sse&key={}", self.model.stream_url(), self.api_key);
        let client = self.client.clone();

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(response) = serde_json::from_str::<GeminiResponse>(data) {
                                            Self::process_stream_response(&response, &tx, &mut tool_call_count).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }
}

#[async_trait]
impl Provider for GeminiChatProvider {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        self.model.name()
    }

    async fn create(&self, request: ChatRequest) -> Result<ChatResponse> {
        let contents = Self::build_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = self.model.select_thinking_level(!request.tools.is_empty(), request.tools.len());

        let response = self.make_request(
            contents,
            Some(request.system),
            tools,
            thinking_level,
        ).await?;

        Ok(Self::parse_response(response))
    }

    async fn create_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let contents = Self::build_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = self.model.select_thinking_level(!request.tools.is_empty(), request.tools.len());

        let api_request = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: request.system }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?alt=sse&key={}", self.model.stream_url(), self.api_key);
        let client = self.client.clone();

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                // Parse SSE events
                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(response) = serde_json::from_str::<GeminiResponse>(data) {
                                            Self::process_stream_response(&response, &tx, &mut tool_call_count).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    async fn continue_with_tools_stream(
        &self,
        request: ToolContinueRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let contents = Self::build_tool_contents(&request);
        let tools = Self::build_tools(&request.tools);
        let thinking_level = self.model.select_thinking_level(!request.tools.is_empty(), request.tools.len());

        let api_request = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiTextPart { text: request.system }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                thinking_config: GeminiThinkingConfig {
                    thinking_level: thinking_level.to_string(),
                },
            }),
            tools,
        };

        let url = format!("{}?alt=sse&key={}", self.model.stream_url(), self.api_key);
        let client = self.client.clone();

        tokio::spawn(async move {
            match client
                .post(&url)
                .json(&api_request)
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(StreamEvent::Error(
                            format!("Gemini API error: {} - {}", status, body)
                        )).await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_call_count = 0;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(line_end) = buffer.find('\n') {
                                    let line = buffer[..line_end].to_string();
                                    buffer = buffer[line_end + 1..].to_string();

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(response) = serde_json::from_str::<GeminiResponse>(data) {
                                            Self::process_stream_response(&response, &tx, &mut tool_call_count).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(StreamEvent::Done).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolEntry>>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiTextPart>,
}

#[derive(Serialize, Clone)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
        /// Thought signature for Gemini 2.5+ multi-turn tool use
        #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct GeminiFunctionCall {
    name: String,
    args: Value,
}

#[derive(Serialize, Clone)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

/// Tool entries for Gemini API - supports both function declarations and built-in tools
#[derive(Serialize)]
#[serde(untagged)]
enum GeminiToolEntry {
    /// Custom function declarations
    Functions {
        #[serde(rename = "functionDeclarations")]
        function_declarations: Vec<GeminiFunctionDeclaration>,
    },
    /// Google Search grounding (FREE until Jan 2026)
    GoogleSearch {
        google_search: EmptyObject,
    },
    /// Code execution (Python sandbox)
    CodeExecution {
        code_execution: EmptyObject,
    },
    /// URL context fetching
    UrlContext {
        url_context: EmptyObject,
    },
    /// File Search (RAG) with per-project stores
    FileSearch {
        file_search: FileSearchConfig,
    },
}

/// Empty object for built-in tools that take no configuration
#[derive(Serialize)]
struct EmptyObject {}

/// Configuration for File Search tool
#[derive(Serialize, Clone)]
pub struct FileSearchConfig {
    /// List of store names to search (e.g., ["fileSearchStores/abc123"])
    #[serde(rename = "fileSearchStoreNames")]
    pub file_search_store_names: Vec<String>,
}

#[derive(Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "thinkingConfig")]
    thinking_config: GeminiThinkingConfig,
}

#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingLevel")]
    thinking_level: String,
}

#[derive(Serialize)]
struct GeminiTextPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
    #[serde(rename = "groundingMetadata")]
    grounding_metadata: Option<GeminiGroundingMetadata>,
    #[serde(rename = "urlContextMetadata")]
    url_context_metadata: Option<GeminiUrlContextMetadata>,
}

#[derive(Deserialize)]
struct GeminiGroundingMetadata {
    #[serde(rename = "webSearchQueries", default)]
    web_search_queries: Vec<String>,
    #[serde(rename = "groundingChunks", default)]
    grounding_chunks: Vec<GeminiGroundingChunk>,
}

#[derive(Deserialize)]
struct GeminiGroundingChunk {
    web: Option<GeminiWebChunk>,
}

#[derive(Deserialize)]
struct GeminiWebChunk {
    uri: String,
    title: Option<String>,
}

/// URL context metadata from url_context tool
#[derive(Deserialize)]
struct GeminiUrlContextMetadata {
    #[serde(rename = "urlMetadata", default)]
    url_metadata: Vec<GeminiUrlMetadataEntry>,
}

#[derive(Deserialize)]
struct GeminiUrlMetadataEntry {
    #[serde(rename = "retrievedUrl")]
    retrieved_url: String,
    #[serde(rename = "urlRetrievalStatus")]
    url_retrieval_status: String,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCallResponse>,
    /// Thought signature for Gemini 2.5+ multi-turn tool use
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
    /// Code execution: generated Python code
    #[serde(rename = "executableCode")]
    executable_code: Option<GeminiExecutableCode>,
    /// Code execution: execution result
    #[serde(rename = "codeExecutionResult")]
    code_execution_result: Option<GeminiCodeExecutionResult>,
}

#[derive(Deserialize)]
struct GeminiExecutableCode {
    language: String,
    code: String,
}

#[derive(Deserialize)]
struct GeminiCodeExecutionResult {
    outcome: String,
    output: Option<String>,
}

#[derive(Deserialize)]
struct GeminiFunctionCallResponse {
    name: String,
    args: Value,
}

#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

// ============================================================================
// Context Caching Types
// ============================================================================

/// Request to create a cached context
#[derive(Serialize)]
struct CreateCacheRequest {
    model: String,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolEntry>>,
    ttl: String,
}

/// Response from cache creation
#[derive(Deserialize)]
struct CreateCacheResponse {
    name: String,
    #[serde(rename = "expireTime")]
    expire_time: String,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<CacheUsageMetadata>,
}

#[derive(Deserialize)]
struct CacheUsageMetadata {
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<u32>,
}

/// Cached content reference for use in requests
#[derive(Debug, Clone)]
pub struct CachedContent {
    /// Cache name (e.g., "cachedContents/abc123")
    pub name: String,
    /// Expiration time (ISO 8601)
    pub expire_time: String,
    /// Number of tokens cached
    pub token_count: u32,
}

/// Request format when using cached content
/// Note: tools, system_instruction, and tool_config must be in the cache, not here
#[derive(Serialize)]
struct CachedGeminiRequest {
    /// Reference to cached content (includes system_instruction + tools)
    #[serde(rename = "cachedContent")]
    cached_content: String,
    /// Only new user messages (not in cache)
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities() {
        let flash = GeminiChatProvider::new("test_key".into(), GeminiModel::Flash);
        assert!(flash.capabilities().supports_tools);
        assert!(flash.capabilities().supports_streaming);
        assert_eq!(flash.capabilities().max_context_tokens, 1_000_000);
        assert_eq!(flash.name(), "Gemini 3 Flash");

        let pro = GeminiChatProvider::new("test_key".into(), GeminiModel::Pro);
        assert_eq!(pro.name(), "Gemini 3 Pro");
    }

    #[test]
    fn test_thinking_level_selection() {
        // Flash always uses medium (balanced default)
        assert_eq!(GeminiModel::Flash.select_thinking_level(false, 0), "medium");
        assert_eq!(GeminiModel::Flash.select_thinking_level(true, 1), "medium");
        assert_eq!(GeminiModel::Flash.select_thinking_level(true, 10), "medium");

        // Pro: binary low/high only
        assert_eq!(GeminiModel::Pro.select_thinking_level(false, 0), "high");
        assert_eq!(GeminiModel::Pro.select_thinking_level(true, 1), "low");
        assert_eq!(GeminiModel::Pro.select_thinking_level(true, 10), "low");
    }

    #[test]
    fn test_model_urls() {
        assert_eq!(GeminiModel::Flash.model_id(), "gemini-3-flash-preview");
        assert_eq!(GeminiModel::Pro.model_id(), "gemini-3-pro-preview");
        assert!(GeminiModel::Flash.generate_url().contains("gemini-3-flash-preview"));
        assert!(GeminiModel::Pro.stream_url().contains("gemini-3-pro-preview"));
    }

    #[test]
    fn test_build_contents() {
        use super::super::Message;
        use super::super::MessageRole;

        let request = ChatRequest {
            model: "gemini-2.5-pro".into(),
            system: "You are helpful".into(),
            messages: vec![
                Message { role: MessageRole::User, content: "Hello".into() },
                Message { role: MessageRole::Assistant, content: "Hi there!".into() },
            ],
            input: "How are you?".into(),
            previous_response_id: None,
            reasoning_effort: None,
            tools: vec![],
            max_tokens: None,
        };

        let contents = GeminiChatProvider::build_contents(&request);
        assert_eq!(contents.len(), 3); // 2 history + 1 current
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[1].role, "model");
        assert_eq!(contents[2].role, "user");
    }
}
