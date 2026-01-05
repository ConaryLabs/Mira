// web/chat/context.rs
// System prompt and personal context building

use chrono::Local;
use chrono::{DateTime, Utc};

use crate::persona;
use crate::web::state::AppState;

use super::summarization::get_summary_context;

/// Build system prompt with persona overlays and personal context
/// KV-cache optimized ordering (static → semi-static → dynamic → volatile)
/// Most stable content first, most volatile (date/time) last
pub async fn build_system_prompt(state: &AppState, user_message: &str) -> String {
    let project_id = state.project_id().await;
    let session_persona = state.get_session_persona().await;

    tracing::info!("Building system prompt with project_id: {:?}", project_id);

    // Start with base persona stack (most stable - includes persona, project context, capabilities)
    let mut prompt = persona::build_system_prompt_with_persona(
        &state.db,
        project_id,
        session_persona.as_deref(),
    );

    // Add conversation summaries (semi-dynamic, changes less frequently)
    let summary_context = get_summary_context(&state.db, 5);
    if !summary_context.is_empty() {
        prompt.push_str(&format!("\n\n=== CONVERSATION HISTORY ===\n{}", summary_context));
    }
    // Add session recap (contextual awareness)
    let session_recap = build_session_recap(state, project_id).await;
    if !session_recap.is_empty() {
        prompt.push_str(&format!("\n\n=== SESSION RECAP ===\n{}", session_recap));
    }

    // Inject personal context (global memories) - dynamic but query-dependent
    let personal_context = build_personal_context(state, user_message).await;
    if !personal_context.is_empty() {
        prompt.push_str(&format!("\n\n=== ABOUT THE USER ===\n{}", personal_context));
    }

    // Current date at the END (most volatile - changes daily, not per-minute)
    // Using just date, not time, to maximize cache hits within a day
    let now = Local::now();
    let date = now.format("%A, %B %d, %Y (%Z)").to_string();
    prompt.push_str(&format!("\n\nCurrent date: {}", date));

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

/// Build session recap with recent activity, pending tasks, and active goals
pub async fn build_session_recap(state: &AppState, project_id: Option<i64>) -> String {
    let mut recap_parts = Vec::new();

    // Get project name if available
    let project_name = if let Some(pid) = project_id {
        if let Ok(Some((name, _path))) = state.db.get_project_info(pid) {
            name
        } else {
            None
        }
    } else {
        None
    };

    // Welcome header
    let welcome = if let Some(name) = project_name {
        format!("Welcome back to {} project!", name)
    } else {
        "Welcome back!".to_string()
    };
    recap_parts.push(format!("╔══════════════════════════════════════╗\n║   {}      ║\n╚══════════════════════════════════════╝", welcome));

    // Time since last chat
    if let Ok(Some(last_chat_time)) = state.db.get_last_chat_time() {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(&last_chat_time) {
            let now = Utc::now();
            let duration = now.signed_duration_since(parsed);
            let hours = duration.num_hours();
            let minutes = duration.num_minutes() % 60;
            let time_ago = if hours > 0 {
                format!("{} hours, {} minutes ago", hours, minutes)
            } else {
                format!("{} minutes ago", minutes)
            };
            recap_parts.push(format!("Last chat: {}", time_ago));
        }
    }

    // Recent sessions (excluding current)
    if let Some(pid) = project_id {
        if let Ok(sessions) = state.db.get_recent_sessions(pid, 2) {
            let recent: Vec<_> = sessions.iter().filter(|s| s.status != "active").collect();
            if !recent.is_empty() {
                let mut session_lines = Vec::new();
                for sess in recent {
                    let short_id = &sess.id[..8];
                    let timestamp = &sess.last_activity[..16]; // YYYY-MM-DD HH:MM
                    if let Some(ref summary) = sess.summary {
                        session_lines.push(format!("• [{}] {} - {}", short_id, timestamp, summary));
                    } else {
                        session_lines.push(format!("• [{}] {}", short_id, timestamp));
                    }
                }
                recap_parts.push(format!("Recent sessions:\n{}", session_lines.join("\n")));
            }
        }
    }

    // Pending tasks
    if let Ok(tasks) = state.db.get_pending_tasks(project_id, 3) {
        if !tasks.is_empty() {
            let task_lines: Vec<String> = tasks.iter()
                .map(|t| format!("• [ ] {} ({})", t.title, t.priority))
                .collect();
            recap_parts.push(format!("Pending tasks:\n{}", task_lines.join("\n")));
        }
    }

    // Active goals
    if let Ok(goals) = state.db.get_active_goals(project_id, 3) {
        if !goals.is_empty() {
            let goal_lines: Vec<String> = goals.iter()
                .map(|g| format!("• {} ({}%) - {}", g.title, g.progress_percent, g.status))
                .collect();
            recap_parts.push(format!("Active goals:\n{}", goal_lines.join("\n")));
        }
    }

    // If we have any recap content, format it nicely
    if recap_parts.len() > 1 { // More than just welcome header
        recap_parts.join("\n\n")
    } else {
        String::new()
    }
}
