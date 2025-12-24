//! Council tool - consult multiple AI models in parallel
//!
//! Uses the unified AdvisoryService to call GPT-5.2, Opus 4.5, and Gemini 3 Pro.
//! In chat context (running on DeepSeek Reasoner), the host synthesizes inline.

use anyhow::Result;
use serde_json::json;

use crate::advisory::{AdvisoryService, AdvisoryModel};

// ============================================================================
// Council Tools
// ============================================================================

/// Council tool implementations - individual model calls and parallel council
pub struct CouncilTools;

impl CouncilTools {
    /// Ask GPT 5.2 directly
    pub async fn ask_gpt(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = build_message(message, context);
        let service = AdvisoryService::from_env()?;
        let response = service.ask(AdvisoryModel::Gpt52, &full_message).await?;

        Ok(json!({
            "provider": "gpt-5.2",
            "response": response.text
        }).to_string())
    }

    /// Ask Opus 4.5 directly
    pub async fn ask_opus(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = build_message(message, context);
        let service = AdvisoryService::from_env()?;
        let response = service.ask(AdvisoryModel::Opus45, &full_message).await?;

        Ok(json!({
            "provider": "opus-4.5",
            "response": response.text
        }).to_string())
    }

    /// Ask Gemini 3 Pro directly
    pub async fn ask_gemini(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = build_message(message, context);
        let service = AdvisoryService::from_env()?;
        let response = service.ask(AdvisoryModel::Gemini3Pro, &full_message).await?;

        Ok(json!({
            "provider": "gemini-3-pro",
            "response": response.text
        }).to_string())
    }

    /// Call the council - all three models in parallel
    /// In chat context, no synthesis is done here - DeepSeek Reasoner (host) synthesizes inline
    pub async fn council(message: &str, context: Option<&str>) -> Result<String> {
        let full_message = build_message(message, context);
        let service = AdvisoryService::from_env()?;

        // Use council_raw since chat host (DeepSeek Reasoner) will synthesize inline
        // Include all 3 models (GPT, Opus, Gemini) - no exclusion needed
        let responses = service.council_raw(&full_message, None).await?;

        // Format responses for chat - it expects "council" key with model responses
        let mut council = serde_json::Map::new();
        for (model, response) in responses {
            council.insert(model.as_str().to_string(), json!(response));
        }

        let result = json!({ "council": council });
        Ok(serde_json::to_string_pretty(&result)?)
    }
}

/// Build full message with optional context
fn build_message(message: &str, context: Option<&str>) -> String {
    if let Some(ctx) = context {
        format!("Context: {}\n\n{}", ctx, message)
    } else {
        message.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires API keys
    async fn test_council() {
        let result = CouncilTools::council("What is 2+2?", None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("council"));
    }
}
