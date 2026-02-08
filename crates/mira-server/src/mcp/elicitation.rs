// crates/mira-server/src/elicitation.rs
// MCP elicitation support — interactive user input during tool execution.
//
// Provides a graceful wrapper around rmcp's elicitation API. All call sites
// degrade to existing behavior when the client doesn't support elicitation.

use rmcp::model::{
    CreateElicitationRequestParams, ElicitationAction, ElicitationSchema, EnumSchemaBuilder,
    PrimitiveSchema,
};
use rmcp::service::{Peer, RoleServer};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::llm::Provider;

/// Timeout for elicitation requests (user is typing, so be generous)
const ELICITATION_TIMEOUT: Duration = Duration::from_secs(120);

/// Outcome of an elicitation request, distinguishing all cases.
#[derive(Debug)]
pub enum ElicitationOutcome {
    /// User accepted and provided data
    Accepted(serde_json::Value),
    /// User explicitly declined
    Declined,
    /// User cancelled / dismissed
    Cancelled,
    /// Client doesn't support elicitation
    NotSupported,
    /// Transport or timeout error (logged at warn, treated as unavailable)
    Failed(String),
}

impl ElicitationOutcome {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    pub fn into_value(self) -> Option<serde_json::Value> {
        match self {
            Self::Accepted(v) => Some(v),
            _ => None,
        }
    }
}

/// Wrapper around the MCP peer for elicitation requests.
///
/// Mirrors the existing `SamplingClient` pattern — wraps `Arc<RwLock<Option<Peer>>>`.
#[derive(Clone)]
pub struct ElicitationClient {
    peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
}

impl ElicitationClient {
    pub fn new(peer: Arc<RwLock<Option<Peer<RoleServer>>>>) -> Self {
        Self { peer }
    }

    /// Check if the connected client supports elicitation.
    pub async fn is_available(&self) -> bool {
        let guard = self.peer.read().await;
        guard
            .as_ref()
            .map(|p| p.supports_elicitation())
            .unwrap_or(false)
    }

    /// Send an elicitation request and map the result to an outcome.
    pub async fn request(
        &self,
        message: impl Into<String>,
        schema: ElicitationSchema,
    ) -> ElicitationOutcome {
        let guard = self.peer.read().await;
        let peer = match guard.as_ref() {
            Some(p) if p.supports_elicitation() => p,
            Some(_) => return ElicitationOutcome::NotSupported,
            None => return ElicitationOutcome::NotSupported,
        };

        let params = CreateElicitationRequestParams {
            meta: None,
            message: message.into(),
            requested_schema: schema,
        };

        match peer
            .create_elicitation_with_timeout(params, Some(ELICITATION_TIMEOUT))
            .await
        {
            Ok(result) => match result.action {
                ElicitationAction::Accept => match result.content {
                    Some(data) => ElicitationOutcome::Accepted(data),
                    None => ElicitationOutcome::Declined,
                },
                ElicitationAction::Decline => ElicitationOutcome::Declined,
                ElicitationAction::Cancel => ElicitationOutcome::Cancelled,
            },
            Err(e) => {
                let msg = format!("Elicitation request failed: {}", e);
                tracing::warn!("{}", msg);
                ElicitationOutcome::Failed(msg)
            }
        }
    }
}

// =============================================================================
// Schema helpers
// =============================================================================

/// Build an elicitation schema for API key entry.
///
/// Fields:
/// - `provider`: required enum (deepseek)
/// - `api_key`: required string, min 10 chars
/// - `persist`: optional bool (default false) — save to ~/.mira/.env
pub fn api_key_schema() -> ElicitationSchema {
    #[allow(clippy::expect_used)] // Infallible: hardcoded enum values with matching titles
    let provider_enum = EnumSchemaBuilder::new(vec!["deepseek".to_string()])
        .enum_titles(vec!["DeepSeek".to_string()])
        .expect("enum titles count matches values")
        .description("Which LLM provider is this key for?")
        .build();

    ElicitationSchema::builder()
        .title("API Key Setup")
        .description(
            "Provide an LLM API key for background tasks (summaries, pondering, etc.).",
        )
        .required_property("provider", PrimitiveSchema::Enum(provider_enum))
        .required_string_property("api_key", |s| {
            s.description("API key for the selected provider")
                .min_length(10)
        })
        .optional_bool("persist", false)
        .build_unchecked()
}

/// Build an elicitation schema for free-form context/question input.
pub fn context_schema(question: &str) -> ElicitationSchema {
    ElicitationSchema::builder()
        .title("Additional Context")
        .description(question.to_string())
        .required_string("response")
        .build_unchecked()
}

// =============================================================================
// Request helpers
// =============================================================================

/// Request an API key from the user via elicitation.
///
/// Returns `Some((provider, key, persist))` on success, `None` otherwise.
pub async fn request_api_key(client: &ElicitationClient) -> Option<(Provider, String, bool)> {
    let schema = api_key_schema();
    let outcome = client
        .request(
            "Background tasks require an LLM API key (DeepSeek). \
             Would you like to provide one now?",
            schema,
        )
        .await;

    let data = outcome.into_value()?;
    let obj = data.as_object()?;

    let provider_str = obj.get("provider")?.as_str()?;
    let provider = Provider::from_str(provider_str)?;
    let api_key = obj.get("api_key")?.as_str()?.to_string();
    let persist = obj
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if api_key.len() < 10 {
        return None;
    }

    Some((provider, api_key, persist))
}

/// Request free-form context from the user via elicitation.
///
/// Returns `Some(response_text)` on success, `None` otherwise.
pub async fn request_context(client: &ElicitationClient, question: &str) -> Option<String> {
    let schema = context_schema(question);
    let outcome = client.request(question, schema).await;
    let data = outcome.into_value()?;
    let obj = data.as_object()?;
    let response = obj.get("response")?.as_str()?.to_string();

    if response.trim().is_empty() {
        return None;
    }

    Some(response)
}

/// Best-effort append an API key to ~/.mira/.env.
pub fn persist_api_key(env_var_name: &str, key: &str) {
    let Some(home) = dirs::home_dir() else {
        tracing::warn!("[elicitation] Cannot determine home directory for key persistence");
        return;
    };

    let env_path = home.join(".mira").join(".env");

    // Ensure directory exists
    if let Some(parent) = env_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("[elicitation] Failed to create .mira directory: {}", e);
        return;
    }

    let line = format!("\n{}={}\n", env_var_name, key);
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&env_path)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = file.write_all(line.as_bytes()) {
                tracing::warn!("[elicitation] Failed to write key to {:?}: {}", env_path, e);
            } else {
                tracing::info!("[elicitation] Persisted {} to {:?}", env_var_name, env_path);
            }
        }
        Err(e) => {
            tracing::warn!("[elicitation] Failed to open {:?}: {}", env_path, e);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_schema_has_required_fields() {
        let schema = api_key_schema();
        let required = schema
            .required
            .as_ref()
            .expect("should have required fields");
        assert!(required.contains(&"provider".to_string()));
        assert!(required.contains(&"api_key".to_string()));
        assert!(!required.contains(&"persist".to_string()));
        assert_eq!(schema.properties.len(), 3);
    }

    #[test]
    fn test_context_schema_has_response_field() {
        let schema = context_schema("What additional context?");
        let required = schema
            .required
            .as_ref()
            .expect("should have required fields");
        assert!(required.contains(&"response".to_string()));
        assert_eq!(schema.properties.len(), 1);
    }

    #[test]
    fn test_outcome_is_accepted() {
        let accepted = ElicitationOutcome::Accepted(serde_json::json!({"key": "val"}));
        assert!(accepted.is_accepted());

        let declined = ElicitationOutcome::Declined;
        assert!(!declined.is_accepted());

        let cancelled = ElicitationOutcome::Cancelled;
        assert!(!cancelled.is_accepted());

        let not_supported = ElicitationOutcome::NotSupported;
        assert!(!not_supported.is_accepted());

        let failed = ElicitationOutcome::Failed("err".into());
        assert!(!failed.is_accepted());
    }

    #[test]
    fn test_outcome_into_value() {
        let val = serde_json::json!({"provider": "deepseek"});
        let accepted = ElicitationOutcome::Accepted(val.clone());
        assert_eq!(accepted.into_value(), Some(val));

        assert_eq!(ElicitationOutcome::Declined.into_value(), None);
        assert_eq!(ElicitationOutcome::Cancelled.into_value(), None);
        assert_eq!(ElicitationOutcome::NotSupported.into_value(), None);
        assert_eq!(ElicitationOutcome::Failed("err".into()).into_value(), None);
    }

    #[tokio::test]
    async fn test_no_peer_returns_not_supported() {
        let client = ElicitationClient::new(Arc::new(RwLock::new(None)));
        assert!(!client.is_available().await);

        let schema = api_key_schema();
        let outcome = client.request("test", schema).await;
        assert!(matches!(outcome, ElicitationOutcome::NotSupported));
    }
}
