// src/api/two_phase.rs
use std::sync::Arc;
use anyhow::Result;
use futures::{Stream, StreamExt};
use tracing::{info, warn};

use crate::api::types::ResponseMetadata;
use crate::llm::client::OpenAIClient;
use crate::llm::streaming::{stream_response, StreamEvent};
use crate::persona::PersonaOverlay;
use crate::memory::recall::RecallContext;
use crate::services::chat::ChatResponse;

/// Build system prompt for any response
pub fn build_system_prompt(
    persona: &PersonaOverlay,
    context: &RecallContext,
) -> String {
    let mut prompt = String::new();

    // Add persona
    prompt.push_str(persona.prompt());
    prompt.push_str("\n\n");

    // Add context if available
    if !context.recent.is_empty() {
        prompt.push_str("Recent conversation:\n");
        for entry in &context.recent {
            prompt.push_str(&format!("{}: {}\n", entry.role, entry.content));
        }
        prompt.push_str("\n");
    }

    prompt
}

/// Phase 1: Get metadata with structured JSON
pub async fn get_metadata(
    client: &OpenAIClient,
    user_message: &str,
    persona: &PersonaOverlay,
    context: &RecallContext,
) -> Result<ResponseMetadata> {
    let metadata_prompt = format!(
        r#"{}

You are responding to: "{}"

Provide a JSON object with these fields:
{{
    "output": "A preview or initial part of your response - can be a greeting, acknowledgment, or the first part of your answer. This can be up to 500 tokens if you want to include substantial content here.",
    "mood": "your current emotional state (e.g., excited, contemplative, focused, playful)",
    "salience": 1-10 (importance/memorability of this interaction),
    "memory_type": "event|fact|emotion|preference|context",
    "tags": ["relevant", "tags", "for", "categorization"],
    "intent": "what you're trying to accomplish with this response",
    "summary": "comprehensive summary for memory storage - be detailed",
    "monologue": "your internal thoughts, reactions, and meta-commentary about this interaction (optional but encouraged)",
    "reasoning_summary": "detailed explanation of your reasoning process, assumptions, and approach (optional but encouraged)"
}}

You have up to 1280 tokens for this entire JSON response. Use the space wisely:
- If the response will be simple, put more in "output" as a preview
- For complex responses, use the space for rich metadata, monologue, and reasoning
- Be expressive and detailed in mood, intent, and summary fields
- Include relevant tags that will help with memory retrieval
- The monologue field is your space for personality and internal thoughts"#,
        build_system_prompt(persona, context),
        user_message
    );

    info!("📊 Phase 1: Getting metadata (up to 1280 tokens)");

    let metadata_stream = stream_response(
        client,
        user_message,
        Some(&metadata_prompt),
        true,  // structured JSON
    ).await?;

    extract_metadata(metadata_stream).await
}

/// Phase 2: Get full content with plain text streaming
pub async fn get_content_stream(
    client: &OpenAIClient,
    user_message: &str,
    persona: &PersonaOverlay,
    context: &RecallContext,
    metadata: &ResponseMetadata,
) -> Result<impl Stream<Item = Result<StreamEvent>>> {
    let content_prompt = format!(
        r#"{}

Previous context: {}
User message: "{}"

Provide your complete response as Mira. Be thorough and detailed.
No JSON formatting, no metadata fields - just your natural response.
Your current mood is: {}
Your intent is: {}

Write as much as needed - there are no length limits."#,
        build_system_prompt(persona, context),
        build_conversation_context(context, 10),
        user_message,
        metadata.mood,
        metadata.intent
    );

    info!("📝 Phase 2: Streaming content");

    stream_response(
        client,
        user_message,
        Some(&content_prompt),
        false,  // plain text
    ).await
}

/// Extract metadata from JSON stream
async fn extract_metadata(
    mut stream: impl Stream<Item = Result<StreamEvent>> + Unpin,
) -> Result<ResponseMetadata> {
    let mut json_buffer = String::new();

    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::Delta(chunk) => {
                json_buffer.push_str(&chunk);
            }
            StreamEvent::Done { full_text, .. } => {
                // Try to parse the complete JSON
                if let Ok(metadata) = serde_json::from_str::<ResponseMetadata>(&full_text) {
                    info!("✅ Metadata extracted successfully");
                    return Ok(metadata);
                }
                // Try buffer as fallback
                if let Ok(metadata) = serde_json::from_str::<ResponseMetadata>(&json_buffer) {
                    return Ok(metadata);
                }
                warn!("⚠️ Could not parse metadata, using defaults");
                break;
            }
            StreamEvent::Error(e) => {
                warn!("Metadata stream error: {}", e);
                break;
            }
        }
    }

    Ok(ResponseMetadata::default())
}

fn build_conversation_context(context: &RecallContext, limit: usize) -> String {
    context.recent
        .iter()
        .take(limit)
        .map(|entry| format!("{}: {}", entry.role, entry.content))
        .collect::<Vec<_>>()
        .join("\n")
}
