// crates/mira-server/src/proxy/usage.rs
// Token usage tracking and cost estimation

use serde::{Deserialize, Serialize};

/// Usage data extracted from an API response
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageData {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl UsageData {
    /// Parse usage from an Anthropic API response
    pub fn from_anthropic_response(json: &serde_json::Value) -> Option<Self> {
        let usage = json.get("usage")?;
        Some(Self {
            input_tokens: usage.get("input_tokens")?.as_u64()?,
            output_tokens: usage.get("output_tokens")?.as_u64()?,
            cache_creation_input_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_read_input_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        })
    }

    /// Parse usage from a streaming message_stop event
    pub fn from_stream_event(json: &serde_json::Value) -> Option<Self> {
        // In streaming, usage comes in message_delta with stop_reason
        // or in the final message_stop event
        if json.get("type")?.as_str()? == "message_delta" {
            let usage = json.get("usage")?;
            return Some(Self {
                input_tokens: 0, // Input tokens are in message_start
                output_tokens: usage.get("output_tokens")?.as_u64()?,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            });
        }

        // message_start contains input token info
        if json.get("type")?.as_str()? == "message_start" {
            let message = json.get("message")?;
            let usage = message.get("usage")?;
            return Some(Self {
                input_tokens: usage.get("input_tokens")?.as_u64()?,
                output_tokens: 0,
                cache_creation_input_tokens: usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_read_input_tokens: usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            });
        }

        None
    }

    /// Total tokens (input + output)
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Merge two usage records (for combining streaming events)
    pub fn merge(&mut self, other: &UsageData) {
        self.input_tokens = self.input_tokens.max(other.input_tokens);
        self.output_tokens = self.output_tokens.max(other.output_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .max(other.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .max(other.cache_read_input_tokens);
    }
}

/// A single usage record for storage
#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub backend_name: String,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_estimate: Option<f64>,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub project_id: Option<i64>,
}

/// Summary of usage over a time period
#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub backend_name: String,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cost: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_anthropic_response() {
        let json = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        });

        let usage = UsageData::from_anthropic_response(&json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn test_parse_anthropic_response_with_cache() {
        let json = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 20,
                "cache_read_input_tokens": 30
            }
        });

        let usage = UsageData::from_anthropic_response(&json).unwrap();
        assert_eq!(usage.cache_creation_input_tokens, 20);
        assert_eq!(usage.cache_read_input_tokens, 30);
    }

    #[test]
    fn test_merge_usage() {
        let mut usage1 = UsageData {
            input_tokens: 100,
            output_tokens: 0,
            cache_creation_input_tokens: 10,
            cache_read_input_tokens: 20,
        };

        let usage2 = UsageData {
            input_tokens: 0,
            output_tokens: 50,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        usage1.merge(&usage2);
        assert_eq!(usage1.input_tokens, 100);
        assert_eq!(usage1.output_tokens, 50);
        assert_eq!(usage1.cache_creation_input_tokens, 10);
    }
}
