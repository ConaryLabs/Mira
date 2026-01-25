// crates/mira-server/src/llm/gemini/types.rs
// Gemini API types (Google's format)

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Request Types
// ============================================================================

/// Gemini request
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    pub generation_config: GenerationConfig,
}

/// Thinking configuration for Gemini 3 (nested inside GenerationConfig)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Thinking level - Pro supports: "low", "high" (default)
    /// Flash also supports: "minimal", "medium"
    pub thinking_level: String,
    /// Include thought summaries in response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
}

/// Generation config
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    pub max_output_tokens: u32,
    /// Temperature - keep at 1.0 for reasoning tasks per Google docs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Thinking configuration (nested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}

// ============================================================================
// Content Types
// ============================================================================

/// Gemini content (message)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String, // "user" | "model"
    pub parts: Vec<GeminiPart>,
}

/// Gemini part (content can have multiple parts)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GeminiPart {
    Text {
        text: String,
        /// If true, this is a thought summary (reasoning)
        #[serde(default)]
        thought: bool,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
        /// Gemini 3 thought signature - must be preserved and sent back
        #[serde(rename = "thoughtSignature")]
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(default)]
        thought_signature: Option<String>,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

/// Gemini function call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: Value,
}

/// Gemini function response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: Value,
}

// ============================================================================
// Tool Types
// ============================================================================

/// Gemini tool definition - can be functions or built-in tools
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum GeminiTool {
    Functions(GeminiFunctionsTool),
    GoogleSearch(GoogleSearchTool),
}

/// Functions tool wrapper
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionsTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

/// Google Search built-in tool
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSearchTool {
    pub google_search: GoogleSearchConfig,
}

/// Google Search configuration (empty for default)
#[derive(Debug, Serialize)]
pub struct GoogleSearchConfig {}

/// Gemini function declaration
#[derive(Debug, Serialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

// ============================================================================
// Response Types
// ============================================================================

/// Gemini response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub usage_metadata: Option<GeminiUsage>,
}

/// Gemini candidate
#[derive(Debug, Deserialize)]
pub struct GeminiCandidate {
    pub content: GeminiContent,
}

/// Gemini usage metadata
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsage {
    pub prompt_token_count: u32,
    pub candidates_token_count: Option<u32>,
    pub total_token_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // GeminiPart serialization tests
    // ============================================================================

    #[test]
    fn test_gemini_part_text_serialize() {
        let part = GeminiPart::Text {
            text: "Hello".to_string(),
            thought: false,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_gemini_part_text_thought_serialize() {
        let part = GeminiPart::Text {
            text: "Thinking...".to_string(),
            thought: true,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"thought\":true"));
    }

    #[test]
    fn test_gemini_part_function_call_serialize() {
        let part = GeminiPart::FunctionCall {
            function_call: GeminiFunctionCall {
                name: "search".to_string(),
                args: serde_json::json!({"query": "test"}),
            },
            thought_signature: None,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("functionCall"));
        assert!(json.contains("search"));
    }

    #[test]
    fn test_gemini_part_function_response_serialize() {
        let part = GeminiPart::FunctionResponse {
            function_response: GeminiFunctionResponse {
                name: "search".to_string(),
                response: serde_json::json!({"result": "found"}),
            },
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("functionResponse"));
    }

    // ============================================================================
    // GeminiContent tests
    // ============================================================================

    #[test]
    fn test_gemini_content_user() {
        let content = GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hello".to_string(),
                thought: false,
            }],
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_gemini_content_model() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hi there".to_string(),
                thought: false,
            }],
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"role\":\"model\""));
    }

    // ============================================================================
    // ThinkingConfig tests
    // ============================================================================

    #[test]
    fn test_thinking_config_serialize() {
        let config = ThinkingConfig {
            thinking_level: "high".to_string(),
            include_thoughts: Some(true),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("thinkingLevel"));
        assert!(json.contains("includeThoughts"));
    }

    // ============================================================================
    // GenerationConfig tests
    // ============================================================================

    #[test]
    fn test_generation_config_serialize() {
        let config = GenerationConfig {
            max_output_tokens: 8192,
            temperature: Some(1.0),
            thinking_config: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("maxOutputTokens"));
        assert!(json.contains("8192"));
    }
}
