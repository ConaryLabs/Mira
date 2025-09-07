//! Rates a message's "emotional weight" from 0.0 (none) to 1.0 (max) using the LLM itself.

use crate::llm::OpenAIClient;
use anyhow::Error;

/// Uses the LLM to rate how emotionally complex or weighty a message is, from 0.0 (not emotional) to 1.0 (maximum).
pub async fn classify(client: &OpenAIClient, message: &str) -> Result<f32, Error> {
    let prompt = format!(
        "On a scale from 0.0 (not emotional) to 1.0 (maximum emotional depth), rate the emotional complexity of the following message:\n\n\"{message}\"\n\nOnly reply with a single number."
    );

    // Build a minimal system prompt for this specific task
    let system_prompt = "You are an emotional weight classifier. Reply with only a single decimal number between 0.0 and 1.0.";

    // Use the simple_chat method that doesn't enforce JSON format
    let response = client
        .simple_chat(&prompt, "gpt-5", system_prompt)
        .await?;

    // Try to parse the LLM's reply as a float (be forgiving of whitespace)
    let val: f32 = response
        .trim()
        .parse()
        .map_err(|_| Error::msg("Failed to parse emotional weight"))?;

    Ok(val.clamp(0.0, 1.0))
}
