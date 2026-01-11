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

    // Get project_id for project-scoped facts
    let project_id = state.project_id().await;

    // Store each fact
    for fact in facts {
        // Work-related facts are project-scoped, others are global
        let fact_project_id = if fact.category == "work" {
            project_id
        } else {
            None
        };

        let id = state.db.store_memory(
            fact_project_id,
            fact.key.as_deref(),
            &fact.content,
            "personal",
            Some(&fact.category),
            0.9, // Slightly lower confidence for auto-extracted facts
        )?;

        // Store embedding if available (also marks fact as having embedding)
        if let Some(ref embeddings) = state.embeddings {
            if let Ok(embedding) = embeddings.embed(&fact.content).await {
                if let Err(e) = state.db.store_fact_embedding(id, &fact.content, &embedding) {
                    warn!("Failed to store embedding for fact {}: {}", id, e);
                }
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
