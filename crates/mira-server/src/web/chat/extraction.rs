// web/chat/extraction.rs
// LLM-based fact extraction from conversations

use tracing::{debug, info, warn};

use crate::web::deepseek::Message;
use crate::web::state::AppState;

/// Prompt for extracting facts from conversation exchanges
const FACT_EXTRACTION_PROMPT: &str = r#"Extract any new facts about the user from this conversation exchange. Focus on:
- Personal details (name, family members, interests, life events)
- Preferences (communication style, likes/dislikes)
- Work context (role, projects, technology preferences)

Return ONLY a JSON array of facts. Each fact should have:
- "content": the fact itself (clear, concise statement)
- "category": one of "profile", "personal", "preferences", "work"
- "key": optional unique identifier for deduplication (e.g. "user_name", "daughter_name")

Example output:
[
  {"content": "User's name is Peter", "category": "profile", "key": "user_name"},
  {"content": "Has a daughter named Emma who is 5 years old", "category": "personal", "key": "daughter"}
]

If no new facts worth remembering, return: []

Respond with ONLY the JSON array, no other text."#;

/// Extract facts from a conversation exchange and store as global memories
/// Runs in background to not block the response
pub fn spawn_fact_extraction(
    state: AppState,
    user_message: String,
    assistant_response: String,
) {
    tokio::spawn(async move {
        if let Err(e) = extract_and_store_facts(&state, &user_message, &assistant_response).await {
            warn!("Fact extraction failed: {}", e);
        }
    });
}

/// Actually perform the extraction
async fn extract_and_store_facts(
    state: &AppState,
    user_message: &str,
    assistant_response: &str,
) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Build extraction prompt
    let exchange = format!(
        "User: {}\n\nAssistant: {}",
        user_message,
        assistant_response
    );

    let messages = vec![
        Message::system(FACT_EXTRACTION_PROMPT.to_string()),
        Message::user(exchange),
    ];

    // Call DeepSeek (no tools needed for extraction)
    let result = deepseek.chat(messages, None).await?;

    let content = result.content
        .ok_or_else(|| anyhow::anyhow!("No content in extraction response"))?;

    // Parse JSON array of facts
    let facts: Vec<ExtractedFact> = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to parse extraction response as JSON: {} - content: {}", e, content);
            return Ok(()); // Not an error, just no facts extracted
        }
    };

    if facts.is_empty() {
        debug!("No facts extracted from exchange");
        return Ok(());
    }

    info!("Extracted {} facts from conversation", facts.len());

    // Store each fact as global memory
    for fact in facts {
        let id = state.db.store_global_memory(
            &fact.content,
            &fact.category,
            fact.key.as_deref(),
            Some(0.9), // Slightly lower confidence for auto-extracted facts
        )?;

        // Also store embedding if available
        if let Some(ref embeddings) = state.embeddings {
            if let Ok(embedding) = embeddings.embed(&fact.content).await {
                let conn = state.db.conn();
                let embedding_bytes: Vec<u8> = embedding
                    .iter()
                    .flat_map(|f| f.to_le_bytes())
                    .collect();

                let _ = conn.execute(
                    "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
                    rusqlite::params![id, embedding_bytes, id, &fact.content],
                );
            }
        }

        debug!("Stored fact: {} (category: {}, key: {:?})", fact.content, fact.category, fact.key);
    }

    Ok(())
}

/// A fact extracted from conversation
#[derive(Debug, serde::Deserialize)]
struct ExtractedFact {
    content: String,
    category: String,
    key: Option<String>,
}
