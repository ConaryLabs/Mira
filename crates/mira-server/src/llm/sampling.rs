// crates/mira-server/src/llm/sampling.rs
// MCP Sampling client — forwards LLM requests to the host (Claude Code) via
// the MCP sampling/createMessage protocol. Zero API keys required.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rmcp::model::{
    Content, CreateMessageRequestParams, ModelHint, ModelPreferences, Role, SamplingMessage,
};
use rmcp::service::{Peer, RoleServer};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::provider::{LlmClient, Provider};
use super::types::ChatResult;
use super::{Message, Tool};

/// LLM client that delegates to the MCP host via sampling/createMessage.
///
/// This enables LLM calls without API keys — the host client
/// (e.g. Claude Code) handles the actual LLM call. Limitations:
/// - No tool support (MCP sampling spec has no tool calling)
/// - Single-shot only (no agentic loops)
/// - Requires user approval per request (host-controlled)
pub struct SamplingClient {
    peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
}

impl SamplingClient {
    pub fn new(peer: Arc<RwLock<Option<Peer<RoleServer>>>>) -> Self {
        Self { peer }
    }

    /// Check if the client's peer supports sampling
    pub async fn is_available(&self) -> bool {
        let guard = self.peer.read().await;
        if let Some(ref peer) = *guard {
            peer.peer_info()
                .map(|info| info.capabilities.sampling.is_some())
                .unwrap_or(false)
        } else {
            false
        }
    }
}

#[async_trait]
impl LlmClient for SamplingClient {
    async fn chat(&self, messages: Vec<Message>, _tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let guard = self.peer.read().await;
        let peer = guard
            .as_ref()
            .ok_or_else(|| anyhow!("MCP peer not connected — sampling unavailable"))?;

        // Separate system prompt from conversation messages
        let mut system_prompt = None;
        let mut sampling_messages = Vec::new();

        for msg in &messages {
            match msg.role.as_str() {
                "system" => {
                    // Concatenate multiple system messages (rare, but handle it)
                    if let Some(content) = &msg.content {
                        system_prompt = Some(match system_prompt.take() {
                            Some(existing) => format!("{}\n\n{}", existing, content),
                            None => content.clone(),
                        });
                    }
                }
                "user" => {
                    if let Some(content) = &msg.content {
                        sampling_messages.push(SamplingMessage {
                            role: Role::User,
                            content: Content::text(content.as_str()),
                        });
                    }
                }
                "assistant" => {
                    if let Some(content) = &msg.content {
                        sampling_messages.push(SamplingMessage {
                            role: Role::Assistant,
                            content: Content::text(content.as_str()),
                        });
                    }
                }
                // Skip tool messages — sampling doesn't support them
                _ => {}
            }
        }

        if sampling_messages.is_empty() {
            return Err(anyhow!("No user/assistant messages to send via sampling"));
        }

        let params = CreateMessageRequestParams {
            meta: None,
            task: None,
            messages: sampling_messages,
            model_preferences: Some(ModelPreferences {
                hints: Some(vec![ModelHint {
                    name: Some("claude".into()),
                }]),
                cost_priority: None,
                speed_priority: None,
                intelligence_priority: Some(0.8),
            }),
            system_prompt,
            include_context: None,
            temperature: Some(0.3),
            max_tokens: 8192,
            stop_sequences: None,
            metadata: None,
        };

        let start = std::time::Instant::now();

        let result = peer
            .create_message(params)
            .await
            .map_err(|e| anyhow!("MCP sampling request failed: {}", e))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Extract text from response
        let content = result.message.content.as_text().map(|t| t.text.clone());

        Ok(ChatResult {
            request_id: String::new(),
            content,
            reasoning_content: None,
            tool_calls: None,
            usage: None, // Sampling doesn't report token usage
            duration_ms,
        })
    }

    fn provider_type(&self) -> Provider {
        Provider::Sampling
    }

    fn model_name(&self) -> String {
        "mcp-sampling".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type() {
        let peer = Arc::new(RwLock::new(None));
        let client = SamplingClient::new(peer);
        assert_eq!(client.provider_type(), Provider::Sampling);
        assert_eq!(client.model_name(), "mcp-sampling");
    }

    #[tokio::test]
    async fn test_no_peer_returns_error() {
        let peer = Arc::new(RwLock::new(None));
        let client = SamplingClient::new(peer);
        let messages = vec![Message::user("test")];
        let result = client.chat(messages, None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("peer not connected")
        );
    }

    #[tokio::test]
    async fn test_empty_messages_returns_error() {
        // Even with a peer, empty messages should fail at our validation layer
        let peer = Arc::new(RwLock::new(None));
        let client = SamplingClient::new(peer);
        // Only system messages — no user/assistant
        let messages = vec![Message::system("You are an expert.")];
        let result = client.chat(messages, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_is_available_no_peer() {
        let peer = Arc::new(RwLock::new(None));
        let client = SamplingClient::new(peer);
        assert!(!client.is_available().await);
    }
}
