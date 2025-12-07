// src/testing/mock_llm/provider.rs
// Mock LLM provider that replays recorded responses

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::llm::provider::{
    FunctionCall, LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse,
};
use super::matcher::{MatchStrategy, RequestMatcher};
use super::recording::{Recording, RecordedExchange, RecordingMetadata, RecordingStorage, create_exchange};

/// Mock LLM provider that replays recorded responses
pub struct MockLlmProvider {
    /// Request matcher for finding recorded responses
    matcher: Mutex<RequestMatcher>,

    /// Whether to record new exchanges (for capture mode)
    recording_mode: bool,

    /// New exchanges captured during recording mode
    captured: Mutex<Vec<RecordedExchange>>,

    /// Fallback provider for recording mode
    fallback: Option<Arc<dyn LlmProvider>>,

    /// Simulated latency (ms) - 0 for instant responses
    simulated_latency_ms: u64,

    /// Whether to fail on no match (vs returning error response)
    strict_mode: bool,
}

impl MockLlmProvider {
    /// Create a mock provider from recordings with a match strategy
    pub fn from_recordings(recordings: Vec<Recording>, strategy: MatchStrategy) -> Self {
        let strategy_debug = format!("{:?}", strategy);
        let matcher = RequestMatcher::new(recordings, strategy);
        info!(
            "[MockLlmProvider] Loaded {} exchanges with {} strategy",
            matcher.exchange_count(),
            strategy_debug
        );

        Self {
            matcher: Mutex::new(matcher),
            recording_mode: false,
            captured: Mutex::new(Vec::new()),
            fallback: None,
            simulated_latency_ms: 0,
            strict_mode: true,
        }
    }

    /// Create a mock provider from a single recording file
    pub fn from_file(path: &std::path::Path, strategy: MatchStrategy) -> Result<Self> {
        let recording = RecordingStorage::load(path)?;
        Ok(Self::from_recordings(vec![recording], strategy))
    }

    /// Create a mock provider from a directory of recordings
    pub fn from_directory(dir: &std::path::Path, strategy: MatchStrategy) -> Result<Self> {
        let recordings = RecordingStorage::load_directory(dir)?;
        Ok(Self::from_recordings(recordings, strategy))
    }

    /// Create a recording provider that captures exchanges
    pub fn recording(fallback: Arc<dyn LlmProvider>) -> Self {
        Self {
            matcher: Mutex::new(RequestMatcher::new(vec![], MatchStrategy::Sequential)),
            recording_mode: true,
            captured: Mutex::new(Vec::new()),
            fallback: Some(fallback),
            simulated_latency_ms: 0,
            strict_mode: false,
        }
    }

    /// Create an empty mock provider for scripted responses
    pub fn empty() -> Self {
        Self::from_recordings(vec![], MatchStrategy::Sequential)
    }

    /// Set simulated latency
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.simulated_latency_ms = latency_ms;
        self
    }

    /// Set strict mode (fail on no match)
    pub fn with_strict_mode(mut self, strict: bool) -> Self {
        self.strict_mode = strict;
        self
    }

    /// Add a scripted response for a specific prompt pattern
    pub fn add_response(&self, prompt_contains: &str, response_text: &str) {
        let exchange = create_exchange(
            vec![Message::user(prompt_contains.to_string())],
            String::new(),
            vec![],
            response_text.to_string(),
            vec![],
            TokenUsage { input: 10, output: 10, reasoning: 0, cached: 0 },
            0,
            RecordingMetadata::default(),
        );

        let mut captured = self.captured.lock().unwrap();
        captured.push(exchange);
    }

    /// Add a scripted tool call response
    pub fn add_tool_call(&self, prompt_contains: &str, tool_name: &str, args: Value) {
        let exchange = create_exchange(
            vec![Message::user(prompt_contains.to_string())],
            String::new(),
            vec![],
            String::new(),
            vec![FunctionCall {
                id: format!("call_{}", uuid::Uuid::new_v4()),
                name: tool_name.to_string(),
                arguments: args,
            }],
            TokenUsage { input: 10, output: 10, reasoning: 0, cached: 0 },
            0,
            RecordingMetadata::default(),
        );

        let mut captured = self.captured.lock().unwrap();
        captured.push(exchange);
    }

    /// Get captured exchanges (for saving after recording)
    pub fn get_captured(&self) -> Vec<RecordedExchange> {
        self.captured.lock().unwrap().clone()
    }

    /// Save captured exchanges to a recording file
    pub fn save_recording(&self, path: &std::path::Path, description: &str) -> Result<()> {
        let captured = self.captured.lock().unwrap();
        let mut recording = Recording::new(description);

        for exchange in captured.iter() {
            recording.add_exchange(exchange.clone());
        }

        RecordingStorage::save(&recording, path)?;
        info!("[MockLlmProvider] Saved {} exchanges to {}", captured.len(), path.display());
        Ok(())
    }

    /// Simulate latency if configured
    async fn simulate_latency(&self) {
        if self.simulated_latency_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.simulated_latency_ms)).await;
        }
    }

    /// Find a match or record if in recording mode
    async fn find_or_record(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[Value],
    ) -> Result<RecordedExchange> {
        // First try to find a match
        {
            let mut matcher = self.matcher.lock().unwrap();
            if let Some(exchange) = matcher.find_match(messages, system, tools) {
                debug!("[MockLlmProvider] Found matching exchange");
                return Ok(exchange.clone());
            }
        }

        // Check scripted responses in captured
        {
            let captured = self.captured.lock().unwrap();
            if let Some(last_user) = messages.iter().rev().find(|m| m.role == "user") {
                for exchange in captured.iter() {
                    if let Some(scripted_user) = exchange.messages.first() {
                        if last_user.content.contains(&scripted_user.content) {
                            debug!("[MockLlmProvider] Found scripted response");
                            return Ok(exchange.clone());
                        }
                    }
                }
            }
        }

        // If in recording mode, use fallback provider
        if self.recording_mode {
            if let Some(ref fallback) = self.fallback {
                debug!("[MockLlmProvider] Recording mode - calling fallback provider");

                let start = Instant::now();
                let result = fallback
                    .chat_with_tools(messages.to_vec(), system.to_string(), tools.to_vec(), None)
                    .await?;

                let exchange = create_exchange(
                    messages.to_vec(),
                    system.to_string(),
                    tools.to_vec(),
                    result.text_output.clone(),
                    result.function_calls.clone(),
                    result.tokens.clone(),
                    start.elapsed().as_millis() as i64,
                    RecordingMetadata {
                        recorded_at: Some(chrono::Utc::now().to_rfc3339()),
                        provider: Some(fallback.name().to_string()),
                        ..Default::default()
                    },
                );

                // Store the captured exchange
                {
                    let mut captured = self.captured.lock().unwrap();
                    captured.push(exchange.clone());
                }

                return Ok(exchange);
            }
        }

        // No match and not recording
        if self.strict_mode {
            Err(anyhow!(
                "MockLlmProvider: No matching exchange found for request (strict mode)"
            ))
        } else {
            // Return a placeholder response
            warn!("[MockLlmProvider] No match found, returning placeholder response");
            Ok(create_exchange(
                messages.to_vec(),
                system.to_string(),
                tools.to_vec(),
                "[Mock: No matching response found]".to_string(),
                vec![],
                TokenUsage { input: 0, output: 0, reasoning: 0, cached: 0 },
                0,
                RecordingMetadata::default(),
            ))
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &'static str {
        "MockLlmProvider"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        self.simulate_latency().await;

        let exchange = self.find_or_record(&messages, &system, &[]).await?;

        Ok(Response {
            content: exchange.response.text,
            model: "mock".to_string(),
            tokens: exchange.response.tokens,
            latency_ms: self.simulated_latency_ms as i64,
        })
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        self.simulate_latency().await;

        let exchange = self.find_or_record(&messages, &system, &tools).await?;

        Ok(ToolResponse {
            id: format!("mock_{}", uuid::Uuid::new_v4()),
            text_output: exchange.response.text,
            function_calls: exchange.response.function_calls,
            tokens: exchange.response.tokens,
            latency_ms: self.simulated_latency_ms as i64,
            raw_response: Value::Null,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_llm::recording::Recording;

    fn make_test_recording() -> Recording {
        let mut recording = Recording::new("Test");
        recording.add_exchange(create_exchange(
            vec![Message::user("Hello".to_string())],
            "System".to_string(),
            vec![],
            "Hi there!".to_string(),
            vec![],
            TokenUsage { input: 5, output: 5, reasoning: 0, cached: 0 },
            100,
            RecordingMetadata::default(),
        ));
        recording
    }

    #[tokio::test]
    async fn test_mock_chat() {
        let recording = make_test_recording();
        let provider = MockLlmProvider::from_recordings(vec![recording], MatchStrategy::LastUserMessage);

        let messages = vec![Message::user("Hello".to_string())];
        let result = provider.chat(messages, "System".to_string()).await.unwrap();

        assert_eq!(result.content, "Hi there!");
        assert_eq!(result.model, "mock");
    }

    #[tokio::test]
    async fn test_scripted_response() {
        let provider = MockLlmProvider::empty().with_strict_mode(false);
        provider.add_response("weather", "It's sunny today!");

        let messages = vec![Message::user("What's the weather?".to_string())];
        let result = provider.chat(messages, String::new()).await.unwrap();

        assert_eq!(result.content, "It's sunny today!");
    }

    #[tokio::test]
    async fn test_scripted_tool_call() {
        let provider = MockLlmProvider::empty().with_strict_mode(false);
        provider.add_tool_call("create file", "write_project_file", serde_json::json!({
            "path": "test.txt",
            "content": "Hello"
        }));

        let messages = vec![Message::user("Please create file test.txt".to_string())];
        let result = provider.chat_with_tools(messages, String::new(), vec![], None).await.unwrap();

        assert_eq!(result.function_calls.len(), 1);
        assert_eq!(result.function_calls[0].name, "write_project_file");
    }
}
