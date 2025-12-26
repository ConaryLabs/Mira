//! Provider capability matrix
//!
//! Defines what each provider supports to enable proper routing and state management.

/// How conversation state is managed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateMode {
    /// Server maintains conversation state (OpenAI Responses API)
    /// Uses `previous_response_id` for continuity
    Server,

    /// Client maintains conversation state (DeepSeek, most chat APIs)
    /// Must send message history with each request
    Client,
}

/// How usage information is reported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageReporting {
    /// Full usage with cached tokens (OpenAI)
    Full,
    /// Basic prompt/completion tokens only
    Basic,
    /// No usage information
    None,
}

/// Provider capabilities for routing and state decisions
#[derive(Debug, Clone)]
pub struct Capabilities {
    /// How conversation state is managed
    pub state_mode: StateMode,

    /// Whether the provider supports tool/function calling
    pub supports_tools: bool,

    /// Whether the provider supports streaming responses
    pub supports_streaming: bool,

    /// Whether cached token counts are available
    pub supports_cached_tokens: bool,

    /// Whether JSON mode / structured output is supported
    pub supports_json_mode: bool,

    /// How usage is reported
    pub usage_reporting: UsageReporting,

    /// Maximum context window in tokens
    pub max_context_tokens: u32,

    /// Maximum output tokens
    pub max_output_tokens: u32,

    /// Recommended context budget per turn (for client-state providers)
    /// This is the "12k per turn" budget for DeepSeek
    pub recommended_turn_budget: u32,
}

impl Capabilities {
    /// OpenAI GPT-5.2 via Responses API
    pub fn openai_responses() -> Self {
        Self {
            state_mode: StateMode::Server,
            supports_tools: true,
            supports_streaming: true,
            supports_cached_tokens: true,
            supports_json_mode: true,
            usage_reporting: UsageReporting::Full,
            max_context_tokens: 400_000,
            max_output_tokens: 16_000,
            // Server-state: no per-turn budget needed
            recommended_turn_budget: 400_000,
        }
    }

    /// DeepSeek Chat via Chat Completions API
    pub fn deepseek_chat() -> Self {
        Self {
            state_mode: StateMode::Client,
            supports_tools: true,
            supports_streaming: true,
            supports_cached_tokens: false,
            supports_json_mode: true,
            usage_reporting: UsageReporting::Basic,
            max_context_tokens: 128_000,
            max_output_tokens: 8_000,
            // Critical: keep turns small for efficiency
            recommended_turn_budget: 12_000,
        }
    }

    /// DeepSeek Reasoner (V3.2 supports tools, large output)
    pub fn deepseek_reasoner() -> Self {
        Self {
            state_mode: StateMode::Client,
            supports_tools: true,  // V3.2 supports tool calls (confirmed 2025-12-23)
            supports_streaming: true,
            supports_cached_tokens: false,
            supports_json_mode: true,
            usage_reporting: UsageReporting::Basic,
            max_context_tokens: 128_000,
            max_output_tokens: 64_000,
            // For planning, can use more context
            recommended_turn_budget: 20_000,
        }
    }

    /// Gemini 3 Flash (default, cheap, fast)
    /// Pro-level intelligence at Flash speed and pricing ($0.50/$3 per 1M)
    /// Supports context caching with 1,024 token minimum (~75% cost reduction)
    pub fn gemini_3_flash() -> Self {
        Self {
            state_mode: StateMode::Client,
            supports_tools: true,
            supports_streaming: true,
            supports_cached_tokens: true, // Context caching supported (min 1024 tokens)
            supports_json_mode: true,
            usage_reporting: UsageReporting::Basic,
            max_context_tokens: 1_000_000,
            max_output_tokens: 65_536,
            // Flash: moderate turn budget
            recommended_turn_budget: 30_000,
        }
    }

    /// Gemini 3 Pro (complex reasoning, advanced planning)
    /// Higher cost ($2/$12 per 1M) but better for council/goal/task
    /// Supports context caching with 4,096 token minimum (~75% cost reduction)
    pub fn gemini_3_pro() -> Self {
        Self {
            state_mode: StateMode::Client,
            supports_tools: true,
            supports_streaming: true,
            supports_cached_tokens: true, // Context caching supported (min 4096 tokens)
            supports_json_mode: true,
            usage_reporting: UsageReporting::Basic,
            max_context_tokens: 1_000_000,
            max_output_tokens: 65_536,
            // Pro: generous turn budget for complex reasoning
            recommended_turn_budget: 50_000,
        }
    }

    /// Check if this is a client-state provider (needs explicit history)
    pub fn needs_client_history(&self) -> bool {
        self.state_mode == StateMode::Client
    }

    /// Check if escalation to another provider might be needed
    pub fn may_need_escalation(&self) -> bool {
        // Client-state providers with tool support may have flaky tool calling
        self.state_mode == StateMode::Client && self.supports_tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_capabilities() {
        let cap = Capabilities::openai_responses();
        assert_eq!(cap.state_mode, StateMode::Server);
        assert!(cap.supports_cached_tokens);
        assert!(!cap.needs_client_history());
    }

    #[test]
    fn test_deepseek_capabilities() {
        let chat = Capabilities::deepseek_chat();
        assert_eq!(chat.state_mode, StateMode::Client);
        assert!(chat.needs_client_history());
        assert!(chat.supports_tools);
        assert_eq!(chat.recommended_turn_budget, 12_000);

        let reasoner = Capabilities::deepseek_reasoner();
        assert!(reasoner.supports_tools);  // V3.2 supports tools
        assert_eq!(reasoner.max_output_tokens, 64_000);
    }

    #[test]
    fn test_escalation_needed() {
        let openai = Capabilities::openai_responses();
        assert!(!openai.may_need_escalation());

        let deepseek = Capabilities::deepseek_chat();
        assert!(deepseek.may_need_escalation());
    }
}
