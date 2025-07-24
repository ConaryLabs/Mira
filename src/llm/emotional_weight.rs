//! Rates a message's "emotional weight" from 0.0 (none) to 1.0 (max) using the LLM itself.

use crate::llm::OpenAIClient;
use anyhow::Error;

/// Uses the LLM to rate how emotionally complex or weighty a message is, from 0.0 (not emotional) to 1.0 (maximum).
pub async fn classify(client: &OpenAIClient, message: &str) -> Result<f32, Error> {
    let prompt = format!(
        "On a scale from 0.0 (not emotional) to 1.0 (maximum emotional depth), rate the emotional complexity of the following message:\n\n\"{}\"\n\nOnly reply with a single number.",
        message
    );

    // Use the structured output method and grab .output (which is just the string the model returns)
    let response = client
        .chat_with_model(&prompt, "gpt-4.1")
        .await?;

    // Try to parse the LLM's reply as a float (be forgiving of whitespace)
    let val: f32 = response
        .output
        .trim()
        .parse()
        .map_err(|_| Error::msg("Failed to parse emotional weight"))?;

    Ok(val.clamp(0.0, 1.0))
}
