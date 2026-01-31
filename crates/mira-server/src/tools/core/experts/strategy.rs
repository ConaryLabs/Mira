// crates/mira-server/src/tools/core/experts/strategy.rs
// ReasoningStrategy: replaces the (chat_client, Option<reasoner_client>) tuple

use crate::llm::LlmClient;
use std::sync::Arc;

/// Encapsulates how an expert consultation splits reasoning work.
///
/// - `Single`: One model handles everything (tool use + synthesis).
/// - `Decoupled`: A fast "actor" model handles tool loops, then a
///   "thinker" model (e.g. deepseek-reasoner) produces the final synthesis.
pub enum ReasoningStrategy {
    /// One client for all phases.
    Single(Arc<dyn LlmClient>),
    /// Separate clients for tool-loop (actor) vs synthesis (thinker).
    Decoupled {
        actor: Arc<dyn LlmClient>,
        thinker: Arc<dyn LlmClient>,
    },
}

impl ReasoningStrategy {
    /// The client used during tool-calling agentic loops.
    pub fn actor(&self) -> &Arc<dyn LlmClient> {
        match self {
            Self::Single(c) => c,
            Self::Decoupled { actor, .. } => actor,
        }
    }

    /// The client used for final synthesis / deep reasoning.
    pub fn thinker(&self) -> &Arc<dyn LlmClient> {
        match self {
            Self::Single(c) => c,
            Self::Decoupled { thinker, .. } => thinker,
        }
    }

    /// Whether this strategy uses separate models for acting vs thinking.
    pub fn is_decoupled(&self) -> bool {
        matches!(self, Self::Decoupled { .. })
    }

    /// Construct from the legacy dual-mode tuple.
    pub fn from_dual_mode(chat: Arc<dyn LlmClient>, reasoner: Option<Arc<dyn LlmClient>>) -> Self {
        match reasoner {
            Some(thinker) => Self::Decoupled {
                actor: chat,
                thinker,
            },
            None => Self::Single(chat),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatResult, Message, Provider, Tool};
    use anyhow::Result;
    use async_trait::async_trait;

    /// Minimal mock LlmClient for unit tests.
    struct MockClient {
        name: String,
    }

    impl MockClient {
        fn create(name: &str) -> Arc<dyn LlmClient> {
            Arc::new(Self {
                name: name.to_string(),
            })
        }
    }

    #[async_trait]
    impl LlmClient for MockClient {
        async fn chat(
            &self,
            _messages: Vec<Message>,
            _tools: Option<Vec<Tool>>,
        ) -> Result<ChatResult> {
            Ok(ChatResult {
                request_id: String::new(),
                content: None,
                reasoning_content: None,
                tool_calls: None,
                usage: None,
                duration_ms: 0,
            })
        }
        fn provider_type(&self) -> Provider {
            Provider::DeepSeek
        }
        fn model_name(&self) -> String {
            self.name.clone()
        }
    }

    #[test]
    fn test_single_strategy() {
        let client = MockClient::create("deepseek-chat");
        let strategy = ReasoningStrategy::Single(client);
        assert!(!strategy.is_decoupled());
        assert_eq!(
            strategy.actor().model_name(),
            strategy.thinker().model_name()
        );
    }

    #[test]
    fn test_decoupled_strategy() {
        let actor = MockClient::create("deepseek-chat");
        let thinker = MockClient::create("deepseek-reasoner");
        let strategy = ReasoningStrategy::Decoupled { actor, thinker };
        assert!(strategy.is_decoupled());
        assert!(strategy.actor().model_name().contains("chat"));
        assert!(strategy.thinker().model_name().contains("reasoner"));
    }

    #[test]
    fn test_from_dual_mode_with_reasoner() {
        let chat = MockClient::create("deepseek-chat");
        let reasoner = MockClient::create("deepseek-reasoner");
        let strategy = ReasoningStrategy::from_dual_mode(chat, Some(reasoner));
        assert!(strategy.is_decoupled());
    }

    #[test]
    fn test_from_dual_mode_without_reasoner() {
        let chat = MockClient::create("deepseek-chat");
        let strategy = ReasoningStrategy::from_dual_mode(chat, None);
        assert!(!strategy.is_decoupled());
    }
}
