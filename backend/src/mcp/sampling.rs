// backend/src/mcp/sampling.rs
// MCP Sampling support - allows MCP servers to request LLM completions through the client

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

/// Sampling request from an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingRequest {
    /// Messages to send to the LLM
    pub messages: Vec<SamplingMessage>,

    /// Model preferences (hints, not requirements)
    #[serde(default, rename = "modelPreferences")]
    pub model_preferences: Option<ModelPreferences>,

    /// System prompt
    #[serde(default, rename = "systemPrompt")]
    pub system_prompt: Option<String>,

    /// Include context from MCP servers
    #[serde(default, rename = "includeContext")]
    pub include_context: Option<IncludeContext>,

    /// Maximum tokens to generate
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: Option<u32>,

    /// Stop sequences
    #[serde(default, rename = "stopSequences")]
    pub stop_sequences: Option<Vec<String>>,

    /// Sampling temperature
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Metadata about the request
    #[serde(default)]
    pub metadata: Option<Value>,
}

/// Message in a sampling request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    /// Role: "user" or "assistant"
    pub role: String,
    /// Content of the message
    pub content: SamplingContent,
}

/// Content types for sampling messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SamplingContent {
    /// Simple text content
    Text(String),
    /// Structured content parts
    Parts(Vec<ContentPart>),
}

impl SamplingContent {
    /// Get text representation of the content
    pub fn as_text(&self) -> String {
        match self {
            SamplingContent::Text(t) => t.clone(),
            SamplingContent::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.clone()),
                    ContentPart::Image { .. } => Some("[image]".to_string()),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// Content part types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text content
    #[serde(rename = "text")]
    Text { text: String },

    /// Image content
    #[serde(rename = "image")]
    Image {
        /// Base64-encoded image data
        data: String,
        /// MIME type of the image
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

/// Model preferences for sampling
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPreferences {
    /// Hints about desired model capabilities
    #[serde(default)]
    pub hints: Vec<ModelHint>,

    /// Cost priority (0.0-1.0, lower = cheaper preferred)
    #[serde(default, rename = "costPriority")]
    pub cost_priority: Option<f32>,

    /// Speed priority (0.0-1.0, higher = faster preferred)
    #[serde(default, rename = "speedPriority")]
    pub speed_priority: Option<f32>,

    /// Intelligence priority (0.0-1.0, higher = smarter preferred)
    #[serde(default, rename = "intelligencePriority")]
    pub intelligence_priority: Option<f32>,
}

/// Model hint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHint {
    /// Model name hint
    #[serde(default)]
    pub name: Option<String>,
}

/// Context inclusion preference
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IncludeContext {
    /// No context
    None,
    /// This server's context only
    ThisServer,
    /// All servers' context
    AllServers,
}

impl Default for IncludeContext {
    fn default() -> Self {
        IncludeContext::None
    }
}

/// Sampling response to return to the MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingResponse {
    /// Role of the response (always "assistant")
    pub role: String,
    /// Content of the response
    pub content: SamplingContent,
    /// Model used
    pub model: String,
    /// Stop reason
    #[serde(default, rename = "stopReason")]
    pub stop_reason: Option<StopReason>,
}

/// Reason why generation stopped
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Reached end of content
    EndTurn,
    /// Hit a stop sequence
    StopSequence,
    /// Hit max tokens
    MaxTokens,
}

/// Approval result from the approval handler
#[derive(Debug, Clone)]
pub enum SamplingApproval {
    /// Approved, proceed with the request
    Approved,
    /// Approved with modifications (e.g., max tokens reduced)
    ApprovedWithModifications(SamplingRequest),
    /// Denied with reason
    Denied(String),
}

/// Trait for handling sampling approval
/// MCP sampling requires human-in-the-loop approval for security
#[async_trait]
pub trait SamplingApprovalHandler: Send + Sync {
    /// Check if a sampling request should be approved
    /// Returns approval status
    async fn check_approval(
        &self,
        server_name: &str,
        request: &SamplingRequest,
    ) -> SamplingApproval;
}

/// Default approval handler that denies all requests
/// This is the safe default - implementers must explicitly enable sampling
pub struct DenyAllSamplingHandler;

#[async_trait]
impl SamplingApprovalHandler for DenyAllSamplingHandler {
    async fn check_approval(
        &self,
        server_name: &str,
        _request: &SamplingRequest,
    ) -> SamplingApproval {
        warn!(
            "[MCP:{}] Sampling request denied (default handler)",
            server_name
        );
        SamplingApproval::Denied(
            "Sampling is disabled. Configure a SamplingApprovalHandler to enable.".to_string(),
        )
    }
}

/// Auto-approve handler with configurable limits
/// Use with caution - only for trusted MCP servers
pub struct AutoApproveSamplingHandler {
    /// Maximum tokens allowed per request
    pub max_tokens_limit: u32,
    /// Allowed server names (empty = all)
    pub allowed_servers: Vec<String>,
}

impl AutoApproveSamplingHandler {
    pub fn new(max_tokens_limit: u32) -> Self {
        Self {
            max_tokens_limit,
            allowed_servers: Vec::new(),
        }
    }

    pub fn with_allowed_servers(mut self, servers: Vec<String>) -> Self {
        self.allowed_servers = servers;
        self
    }
}

#[async_trait]
impl SamplingApprovalHandler for AutoApproveSamplingHandler {
    async fn check_approval(
        &self,
        server_name: &str,
        request: &SamplingRequest,
    ) -> SamplingApproval {
        // Check if server is allowed
        if !self.allowed_servers.is_empty() && !self.allowed_servers.contains(&server_name.to_string()) {
            return SamplingApproval::Denied(format!(
                "Server '{}' is not in the allowed list",
                server_name
            ));
        }

        // Check and potentially modify max tokens
        let requested_tokens = request.max_tokens.unwrap_or(1000);
        if requested_tokens > self.max_tokens_limit {
            info!(
                "[MCP:{}] Sampling request tokens reduced from {} to {}",
                server_name, requested_tokens, self.max_tokens_limit
            );
            let mut modified = request.clone();
            modified.max_tokens = Some(self.max_tokens_limit);
            return SamplingApproval::ApprovedWithModifications(modified);
        }

        debug!(
            "[MCP:{}] Sampling request auto-approved ({} messages)",
            server_name,
            request.messages.len()
        );
        SamplingApproval::Approved
    }
}

/// Interactive approval handler that logs requests (for CLI/UI integration)
/// Real implementations should prompt the user for approval
pub struct InteractiveSamplingHandler {
    /// Default action when no interaction is possible
    pub default_action: SamplingApproval,
}

impl InteractiveSamplingHandler {
    /// Create handler that denies by default (safe)
    pub fn deny_by_default() -> Self {
        Self {
            default_action: SamplingApproval::Denied("Interactive approval not available".to_string()),
        }
    }

    /// Create handler that approves by default (unsafe, testing only)
    pub fn approve_by_default() -> Self {
        Self {
            default_action: SamplingApproval::Approved,
        }
    }
}

#[async_trait]
impl SamplingApprovalHandler for InteractiveSamplingHandler {
    async fn check_approval(
        &self,
        server_name: &str,
        request: &SamplingRequest,
    ) -> SamplingApproval {
        // Log the request for visibility
        info!(
            "[MCP:{}] Sampling request: {} messages, max_tokens={:?}, system_prompt_len={}",
            server_name,
            request.messages.len(),
            request.max_tokens,
            request.system_prompt.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        // In a real implementation, this would prompt the user
        // For now, return the default action
        self.default_action.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sampling_request_parse() {
        let json = r#"{
            "messages": [
                {"role": "user", "content": "Hello, world!"}
            ],
            "maxTokens": 100,
            "temperature": 0.7
        }"#;

        let request: SamplingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.max_tokens, Some(100));
        assert_eq!(request.temperature, Some(0.7));
    }

    #[test]
    fn test_sampling_content_text() {
        let content = SamplingContent::Text("Hello".to_string());
        assert_eq!(content.as_text(), "Hello");
    }

    #[test]
    fn test_sampling_content_parts() {
        let content = SamplingContent::Parts(vec![
            ContentPart::Text {
                text: "Hello".to_string(),
            },
            ContentPart::Text {
                text: "World".to_string(),
            },
        ]);
        assert_eq!(content.as_text(), "Hello\nWorld");
    }

    #[tokio::test]
    async fn test_deny_all_handler() {
        let handler = DenyAllSamplingHandler;
        let request = SamplingRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            max_tokens: None,
            stop_sequences: None,
            temperature: None,
            metadata: None,
        };

        let result = handler.check_approval("test-server", &request).await;
        assert!(matches!(result, SamplingApproval::Denied(_)));
    }

    #[tokio::test]
    async fn test_auto_approve_handler() {
        let handler = AutoApproveSamplingHandler::new(1000);
        let request = SamplingRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            max_tokens: Some(500),
            stop_sequences: None,
            temperature: None,
            metadata: None,
        };

        let result = handler.check_approval("test-server", &request).await;
        assert!(matches!(result, SamplingApproval::Approved));
    }

    #[tokio::test]
    async fn test_auto_approve_handler_reduces_tokens() {
        let handler = AutoApproveSamplingHandler::new(500);
        let request = SamplingRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            max_tokens: Some(1000),
            stop_sequences: None,
            temperature: None,
            metadata: None,
        };

        let result = handler.check_approval("test-server", &request).await;
        match result {
            SamplingApproval::ApprovedWithModifications(modified) => {
                assert_eq!(modified.max_tokens, Some(500));
            }
            _ => panic!("Expected ApprovedWithModifications"),
        }
    }

    #[tokio::test]
    async fn test_auto_approve_handler_server_allowlist() {
        let handler =
            AutoApproveSamplingHandler::new(1000).with_allowed_servers(vec!["allowed".to_string()]);

        let request = SamplingRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            max_tokens: None,
            stop_sequences: None,
            temperature: None,
            metadata: None,
        };

        // Allowed server
        let result = handler.check_approval("allowed", &request).await;
        assert!(matches!(result, SamplingApproval::Approved));

        // Denied server
        let result = handler.check_approval("not-allowed", &request).await;
        assert!(matches!(result, SamplingApproval::Denied(_)));
    }
}
