// src/testing/mock_llm/recording.rs
// Recording format for LLM request/response capture

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::llm::provider::{FunctionCall, Message, TokenUsage};

/// A single recorded LLM exchange (request + response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedExchange {
    /// Hash of the request for matching
    pub request_hash: String,

    /// The messages sent to the LLM
    pub messages: Vec<Message>,

    /// The system prompt
    pub system_prompt: String,

    /// Tools available (if any)
    #[serde(default)]
    pub tools: Vec<Value>,

    /// The response from the LLM
    pub response: RecordedResponse,

    /// Optional metadata
    #[serde(default)]
    pub metadata: RecordingMetadata,
}

/// Recorded LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedResponse {
    /// Text content of the response
    pub text: String,

    /// Function calls made (if any)
    #[serde(default)]
    pub function_calls: Vec<FunctionCall>,

    /// Token usage (for cost estimation)
    pub tokens: TokenUsage,

    /// Original latency (for reference)
    pub latency_ms: i64,
}

/// Metadata about the recording
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordingMetadata {
    /// When this was recorded
    pub recorded_at: Option<String>,

    /// Which provider was used
    pub provider: Option<String>,

    /// Which model was used
    pub model: Option<String>,

    /// Scenario name (for organization)
    pub scenario: Option<String>,

    /// Step name within scenario
    pub step: Option<String>,
}

/// A complete recording file containing multiple exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    /// Version of the recording format
    pub version: String,

    /// Description of what this recording covers
    #[serde(default)]
    pub description: String,

    /// All recorded exchanges
    pub exchanges: Vec<RecordedExchange>,
}

impl Recording {
    /// Create a new empty recording
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            version: "1.0".to_string(),
            description: description.into(),
            exchanges: Vec::new(),
        }
    }

    /// Add an exchange to the recording
    pub fn add_exchange(&mut self, exchange: RecordedExchange) {
        self.exchanges.push(exchange);
    }

    /// Find an exchange by request hash
    pub fn find_by_hash(&self, hash: &str) -> Option<&RecordedExchange> {
        self.exchanges.iter().find(|e| e.request_hash == hash)
    }

    /// Find exchanges by partial message content
    pub fn find_by_content(&self, content: &str) -> Vec<&RecordedExchange> {
        self.exchanges
            .iter()
            .filter(|e| {
                e.messages
                    .iter()
                    .any(|m| m.content.contains(content))
            })
            .collect()
    }
}

/// Storage operations for recordings
pub struct RecordingStorage;

impl RecordingStorage {
    /// Save recording to a JSON file
    pub fn save(recording: &Recording, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(recording)
            .context("Failed to serialize recording")?;
        std::fs::write(path, json)
            .with_context(|| format!("Failed to write recording to {}", path.display()))?;
        Ok(())
    }

    /// Load recording from a JSON file
    pub fn load(path: &Path) -> Result<Recording> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read recording from {}", path.display()))?;
        let recording: Recording = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse recording from {}", path.display()))?;
        Ok(recording)
    }

    /// Load all recordings from a directory
    pub fn load_directory(dir: &Path) -> Result<Vec<Recording>> {
        let mut recordings = Vec::new();

        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                match Self::load(&path) {
                    Ok(recording) => recordings.push(recording),
                    Err(e) => {
                        tracing::warn!("Failed to load recording {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(recordings)
    }
}

/// Compute a hash of a request for matching
pub fn compute_request_hash(messages: &[Message], system: &str, tools: &[Value]) -> String {
    let mut hasher = Sha256::new();

    // Hash system prompt
    hasher.update(system.as_bytes());
    hasher.update(b"|system|");

    // Hash each message
    for msg in messages {
        hasher.update(msg.role.as_bytes());
        hasher.update(b":");
        hasher.update(msg.content.as_bytes());
        hasher.update(b"|");
    }

    // Hash tool names (not full definitions for flexibility)
    for tool in tools {
        if let Some(name) = tool.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()) {
            hasher.update(name.as_bytes());
            hasher.update(b",");
        }
    }

    let result = hasher.finalize();
    hex::encode(result)
}

/// Create a RecordedExchange from request/response data
pub fn create_exchange(
    messages: Vec<Message>,
    system: String,
    tools: Vec<Value>,
    text: String,
    function_calls: Vec<FunctionCall>,
    tokens: TokenUsage,
    latency_ms: i64,
    metadata: RecordingMetadata,
) -> RecordedExchange {
    let request_hash = compute_request_hash(&messages, &system, &tools);

    RecordedExchange {
        request_hash,
        messages,
        system_prompt: system,
        tools,
        response: RecordedResponse {
            text,
            function_calls,
            tokens,
            latency_ms,
        },
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_hash_consistency() {
        let messages = vec![Message::user("Hello".to_string())];
        let system = "You are a helpful assistant";
        let tools: Vec<Value> = vec![];

        let hash1 = compute_request_hash(&messages, system, &tools);
        let hash2 = compute_request_hash(&messages, system, &tools);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_request_hash_differs_for_different_inputs() {
        let messages1 = vec![Message::user("Hello".to_string())];
        let messages2 = vec![Message::user("Goodbye".to_string())];
        let system = "You are a helpful assistant";
        let tools: Vec<Value> = vec![];

        let hash1 = compute_request_hash(&messages1, system, &tools);
        let hash2 = compute_request_hash(&messages2, system, &tools);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_recording_serialization() {
        let mut recording = Recording::new("Test recording");
        recording.add_exchange(RecordedExchange {
            request_hash: "abc123".to_string(),
            messages: vec![Message::user("Hello".to_string())],
            system_prompt: "System".to_string(),
            tools: vec![],
            response: RecordedResponse {
                text: "Hi there!".to_string(),
                function_calls: vec![],
                tokens: TokenUsage {
                    input: 10,
                    output: 5,
                    reasoning: 0,
                    cached: 0,
                },
                latency_ms: 100,
            },
            metadata: RecordingMetadata::default(),
        });

        let json = serde_json::to_string(&recording).unwrap();
        let restored: Recording = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.exchanges.len(), 1);
        assert_eq!(restored.exchanges[0].response.text, "Hi there!");
    }
}
