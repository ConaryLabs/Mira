// web/chat/context.rs
// System prompt and personal context building

use chrono::Local;

use crate::persona;
use crate::web::state::AppState;

use super::summarization::get_summary_context;

/// Build system prompt with persona overlays and personal context
/// KV-cache optimized ordering (static → semi-static → dynamic → volatile)
/// Layers: date -> base persona -> capabilities -> profile -> project -> summaries -> semantic recall
pub async fn build_system_prompt(state: &AppState, user_message: &str) -> String {
    let project_id = state.project_id().await;
    let session_persona = state.get_session_persona().await;

    tracing::info!("Building system prompt with project_id: {:?}", project_id);

    // Start with current date and time (important for research/current events)
    let now = Local::now();
    let datetime = now.format("%A, %B %d, %Y at %I:%M %p %Z").to_string();
    let mut prompt = format!("Current date/time: {}\n\n", datetime);

    // Get base persona stack (includes persona, project context, session, capabilities)
    prompt.push_str(&persona::build_system_prompt_with_persona(
        &state.db,
        project_id,
        session_persona.as_deref(),
    ));

    // Add conversation summaries (semi-dynamic, changes less frequently)
    let summary_context = get_summary_context(&state.db, 5);
    if !summary_context.is_empty() {
        prompt.push_str(&format!("\n\n=== CONVERSATION HISTORY ===\n{}", summary_context));
    }

    // Inject personal context (global memories) - most dynamic part of system prompt
    let personal_context = build_personal_context(state, user_message).await;
    if !personal_context.is_empty() {
        prompt.push_str(&format!("\n\n=== ABOUT THE USER ===\n{}", personal_context));
    }

    prompt
}

/// Build personal context from global memories
/// Combines user profile (always present) with semantic recall based on current message
pub async fn build_personal_context(state: &AppState, user_message: &str) -> String {
    let mut context_parts = Vec::new();

    // 1. Get user profile (core facts - always included)
    if let Ok(profile) = state.db.get_user_profile() {
        if !profile.is_empty() {
            let profile_text: Vec<String> = profile
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            context_parts.push(format!("Profile:\n{}", profile_text.join("\n")));
        }
    }

    // 2. Semantic recall based on current message (if embeddings available)
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(user_message).await {
            if let Ok(memories) = state.db.recall_global_semantic(&query_embedding, 5) {
                if !memories.is_empty() {
                    let relevant: Vec<String> = memories
                        .iter()
                        .filter(|(_, _, distance)| *distance < 0.5) // Only include similar
                        .map(|(_, content, _)| format!("- {}", content))
                        .collect();
                    if !relevant.is_empty() {
                        context_parts.push(format!("Relevant context:\n{}", relevant.join("\n")));
                    }
                }
            }
        }
    }

    context_parts.join("\n\n")
}
