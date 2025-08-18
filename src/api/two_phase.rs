// src/api/two_phase.rs
//! Two-phase chat: (1) Metadata (structured JSON via non-streaming), (2) Content (plain text)

use anyhow::{anyhow, Result};
use futures::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

use crate::llm::client::{extract_text_from_responses, OpenAIClient};
use crate::llm::streaming::StreamEvent;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::builder::build_system_prompt;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub output: String,
    pub mood: String,
    pub intent: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

fn metadata_instructions(system_prompt: &str) -> String {
    format!(
        "You are Mira's metadata analyzer.\n\
         Output ONLY JSON matching this exact schema; no extra fields:\n\
         {{\
           \"output\": \"string\",\
           \"mood\": \"string\",\
           \"intent\": \"string\",\
           \"salience\": 0,\
           \"summary\": \"string\",\
           \"memory_type\": \"string\",\
           \"tags\": [\"string\"],\
           \"monologue\": \"string|null\",\
           \"reasoning_summary\": \"string|null\"\
         }}\n\n\
         System context:\n{system}",
        system = system_prompt
    )
}

pub async fn get_metadata(
    client: &OpenAIClient,
    user_text: &str,
    persona: &PersonaOverlay,
    context: &RecallContext,
) -> Result<Metadata> {
    // Build system prompt
    let system_prompt = build_system_prompt(persona, context);
    let sys = metadata_instructions(&system_prompt);

    // Use NON-streaming with structured JSON format
    let resp = client.generate_response(user_text, Some(&sys), true).await?;
    let raw = resp.raw.unwrap_or(Value::Null);

    // Try to parse the entire response body as the JSON we asked for:
    if let Some(text) = extract_text_from_responses(&raw) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            return Ok(parse_metadata(v));
        }
    }
    
    tracing::error!("Could not parse metadata. Raw response: {}", serde_json::to_string_pretty(&raw).unwrap_or_default());

    Err(anyhow!("metadata stream produced no valid JSON"))
}

fn parse_metadata(v: Value) -> Metadata {
    let mut md = Metadata::default();
    md.output = v.get("output").and_then(Value::as_str).unwrap_or("").to_string();
    md.mood = v.get("mood").and_then(Value::as_str).unwrap_or("neutral").to_string();
    md.intent = v.get("intent").and_then(Value::as_str).unwrap_or("response").to_string();
    md.salience = v.get("salience").and_then(Value::as_u64).unwrap_or(5) as usize;
    md.summary = v.get("summary").and_then(Value::as_str).unwrap_or("").to_string();
    md.memory_type = v.get("memory_type").and_then(Value::as_str).unwrap_or("event").to_string();
    md.tags = v
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    md.monologue = v
        .get("monologue")
        .and_then(|x| if x.is_null() { None } else { x.as_str().map(|s| s.to_string()) });
    md.reasoning_summary = v
        .get("reasoning_summary")
        .and_then(|x| if x.is_null() { None } else { x.as_str().map(|s| s.to_string()) });
    md
}

/// Build a content-only system prompt (no JSON requirements)
fn build_content_prompt(persona: &PersonaOverlay, context: &RecallContext, mood: &str, intent: &str) -> String {
    let mut prompt = String::new();
    
    // 1. Core persona prompt (without the JSON requirement at the end)
    let persona_text = persona.prompt();
    // Remove the CRITICAL JSON instruction if present
    if let Some(idx) = persona_text.find("CRITICAL: Your") {
        prompt.push_str(&persona_text[..idx]);
    } else {
        prompt.push_str(persona_text);
    }
    prompt.push_str("\n\n");
    
    // 2. Add memory context
    prompt.push_str("You have access to our conversation history and memories. ");
    prompt.push_str("Use these naturally in your responses when relevant.\n\n");
    
    // 3. Include recent conversation history
    if !context.recent.is_empty() {
        prompt.push_str("Recent conversation:\n");
        let mut recent_reversed = context.recent.clone();
        recent_reversed.reverse();
        
        for entry in recent_reversed.iter().take(10) {
            prompt.push_str(&format!("[{}] {}\n", entry.role, entry.content));
        }
        prompt.push_str("\n");
    }
    
    // 4. Add mood/intent context
    if !mood.is_empty() || !intent.is_empty() {
        prompt.push_str("[Current state]\n");
        if !mood.is_empty() {
            prompt.push_str(&format!("Mood: {}\n", mood));
        }
        if !intent.is_empty() {
            prompt.push_str(&format!("Intent: {}\n", intent));
        }
        prompt.push_str("\n");
    }
    
    // 5. CRITICAL: Plain text instruction
    prompt.push_str("Respond naturally as Mira. Just your message, no JSON, no metadata, no structured output. Be real, be yourself.");
    
    prompt
}

pub async fn get_content_stream(
    client: &OpenAIClient,
    user_text: &str,
    persona: &PersonaOverlay,
    context: &RecallContext,
    mood: &str,
    intent: &str,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    // Use the new content-specific prompt builder
    let system_prompt = build_content_prompt(persona, context, mood, intent);

    // Non-streaming generation for content
    let resp = client.generate_response(user_text, Some(&system_prompt), false).await?;
    
    // Extract the plain text content
    let text = if let Some(raw_text) = extract_text_from_responses(&resp.raw.unwrap_or(Value::Null)) {
        raw_text.trim().to_string()
    } else {
        resp.content.trim().to_string()
    };

    // Wrap in a stream for compatibility with the WebSocket layer
    let s = stream::once(async move {
        if text.is_empty() {
            Ok(StreamEvent::Done { full_text: String::new(), raw: None })
        } else {
            Ok(StreamEvent::Delta(text))
        }
    })
    .chain(stream::once(async { Ok(StreamEvent::Done { full_text: String::new(), raw: None }) }))
    .boxed();

    Ok(s)
}
