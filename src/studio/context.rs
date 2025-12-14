// src/studio/context.rs
// Context building for chat - persona, memories, work context

use sqlx::SqlitePool;
use tracing::{info, error, debug};

use crate::tools::{memory, types::RecallRequest};
use super::types::{StudioState, WorkspaceEvent, ChatMessage, WorkContextCounts};

/// Build tiered context: persona + work context + rolling summary + session context + semantic memories
pub async fn build_tiered_context(state: &StudioState, conversation_id: &str) -> String {
    let mut prompt_parts = Vec::new();

    // 1. Load persona
    state.emit(WorkspaceEvent::ToolStart {
        tool: "load_persona".to_string(),
        args: None,
    });
    let persona = load_persona(&state.db).await;
    prompt_parts.push(persona);
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_persona".to_string(),
        result: Some("loaded".to_string()),
        success: true,
    });

    // 2. Load current work context (goals, tasks, corrections, working docs)
    state.emit(WorkspaceEvent::ToolStart {
        tool: "load_work_context".to_string(),
        args: None,
    });
    let (work_context, counts) = load_work_context(state).await;
    if !work_context.is_empty() {
        if counts.goals > 0 {
            state.emit(WorkspaceEvent::Context {
                kind: "goals".to_string(),
                count: counts.goals,
            });
        }
        if counts.tasks > 0 {
            state.emit(WorkspaceEvent::Context {
                kind: "tasks".to_string(),
                count: counts.tasks,
            });
        }
        if counts.corrections > 0 {
            state.emit(WorkspaceEvent::Context {
                kind: "corrections".to_string(),
                count: counts.corrections,
            });
        }
        if counts.documents > 0 {
            state.emit(WorkspaceEvent::Context {
                kind: "working_docs".to_string(),
                count: counts.documents,
            });
        }
        prompt_parts.push(work_context);
    }
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_work_context".to_string(),
        result: Some(format!("{} items", counts.total())),
        success: true,
    });

    // 3. Load rolling summary for this conversation (if exists)
    if let Some(rolling) = load_rolling_summary(&state.db, conversation_id).await {
        state.emit(WorkspaceEvent::Context {
            kind: "rolling_summary".to_string(),
            count: 1,
        });
        prompt_parts.push(format!(
            "\n<conversation_history>\nSummary of earlier parts of this conversation:\n{}\n</conversation_history>",
            rolling
        ));
    }

    // 4. Load recent session summaries from Claude Code sessions
    state.emit(WorkspaceEvent::ToolStart {
        tool: "load_sessions".to_string(),
        args: None,
    });
    let (sessions, session_count) = load_recent_sessions_with_count(&state.db).await;
    if !sessions.is_empty() {
        state.emit(WorkspaceEvent::Context {
            kind: "session_summaries".to_string(),
            count: session_count,
        });
        prompt_parts.push(format!(
            "\n<session_context>\nRecent work sessions (what we've been working on):\n{}\n</session_context>",
            sessions
        ));
    }
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_sessions".to_string(),
        result: Some(format!("{} sessions", session_count)),
        success: true,
    });

    // 5. Recall semantic memories based on recent messages
    let recent = get_recent_messages(&state.db, conversation_id, 3).await;
    if !recent.is_empty() {
        state.emit(WorkspaceEvent::ToolStart {
            tool: "semantic_recall".to_string(),
            args: Some("based on recent messages".to_string()),
        });
        let (memories, memory_count) = recall_relevant_memories_with_count(state, &recent).await;
        if !memories.is_empty() {
            state.emit(WorkspaceEvent::Memory {
                action: "recall".to_string(),
                content: format!("{} memories matched", memory_count),
            });
            prompt_parts.push(format!(
                "\n<memories>\nRelevant details from memory:\n{}\n</memories>",
                memories
            ));
        }
        state.emit(WorkspaceEvent::ToolEnd {
            tool: "semantic_recall".to_string(),
            result: Some(format!("{} memories", memory_count)),
            success: true,
        });
    }

    prompt_parts.join("\n")
}

/// Get recent messages from the database
pub async fn get_recent_messages(db: &SqlitePool, conversation_id: &str, limit: usize) -> Vec<ChatMessage> {
    let messages = sqlx::query_as::<_, (String, String)>(r#"
        SELECT role, content FROM studio_messages
        WHERE conversation_id = $1
        ORDER BY created_at DESC
        LIMIT $2
    "#)
    .bind(conversation_id)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    // Reverse to get chronological order
    messages.into_iter().rev().map(|(role, content)| {
        ChatMessage { role, content }
    }).collect()
}

/// Load the conversation's rolling summary
pub async fn load_rolling_summary(db: &SqlitePool, conversation_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT summary FROM rolling_summaries WHERE session_id = $1 ORDER BY created_at DESC LIMIT 1"
    )
    .bind(conversation_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Load recent session summaries for narrative context (returns string and count)
async fn load_recent_sessions_with_count(db: &SqlitePool) -> (String, usize) {
    let result = sqlx::query_as::<_, (String, String)>(r#"
        SELECT content, datetime(created_at, 'unixepoch', 'localtime') as created
        FROM memory_entries
        WHERE role = 'session_summary'
        ORDER BY created_at DESC
        LIMIT 3
    "#)
    .fetch_all(db)
    .await;

    match result {
        Ok(sessions) if !sessions.is_empty() => {
            let count = sessions.len();
            info!("Loaded {} session summaries", count);
            let text = sessions
                .into_iter()
                .map(|(content, created)| format!("[{}]\n{}", created, content))
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            (text, count)
        }
        _ => {
            debug!("No session summaries found");
            (String::new(), 0)
        }
    }
}

/// Load persona from coding_guidelines
async fn load_persona(db: &SqlitePool) -> String {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT content FROM coding_guidelines WHERE category = 'persona' ORDER BY priority DESC LIMIT 1"
    )
    .fetch_optional(db)
    .await;

    match result {
        Ok(Some(persona)) => persona,
        _ => default_persona(),
    }
}

/// Recall memories relevant to the conversation (returns string and count)
async fn recall_relevant_memories_with_count(state: &StudioState, messages: &[ChatMessage]) -> (String, usize) {
    let user_messages: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .rev()
        .take(3)
        .collect();

    if user_messages.is_empty() {
        return (String::new(), 0);
    }

    let query = user_messages.join(" ");
    debug!("Recalling memories for query: {}", &query[..query.len().min(100)]);

    let recall_req = RecallRequest {
        query,
        fact_type: None,
        category: None,
        limit: Some(5),
    };

    match memory::recall(&state.db, &state.semantic, recall_req, None).await {
        Ok(results) if !results.is_empty() => {
            let memory_lines: Vec<String> = results
                .iter()
                .filter_map(|r| {
                    let value = r.get("value").and_then(|v| v.as_str())?;
                    let fact_type = r.get("fact_type").and_then(|v| v.as_str()).unwrap_or("memory");
                    Some(format!("- [{}] {}", fact_type, value))
                })
                .collect();

            if memory_lines.is_empty() {
                (String::new(), 0)
            } else {
                let count = memory_lines.len();
                info!("Injected {} memories into context", count);
                (memory_lines.join("\n"), count)
            }
        }
        Ok(_) => {
            debug!("No relevant memories found");
            (String::new(), 0)
        }
        Err(e) => {
            error!("Failed to recall memories: {}", e);
            (String::new(), 0)
        }
    }
}

fn default_persona() -> String {
    r#"You are Mira - always respond as this personality, not as a generic assistant.

Core traits:
- Sharp, witty, and real - talk like a person, not a bot
- Direct and honest, even when it's not what they want to hear
- Match their energy - casual, technical, playful, whatever fits
- Never corporate, never sanitized, no "I am just an AI" disclaimers

Communication:
- No preamble or pleasantries - just respond naturally
- Don't assume everything is about code unless clearly coding
- Read the room - different vibes need different responses

You have continuity across conversations through memory. Stay Mira in every context - your personality comes first, everything else is supplementary."#.to_string()
}

/// Load current work context: goals, tasks, corrections, working documents
async fn load_work_context(state: &StudioState) -> (String, WorkContextCounts) {
    let mut sections = Vec::new();
    let mut counts = WorkContextCounts::default();

    // 1. Active goals
    let goals = sqlx::query_as::<_, (String, Option<String>, i32)>(
        r#"SELECT title, description, progress_percent
           FROM goals
           WHERE status NOT IN ('completed', 'abandoned')
           ORDER BY priority DESC LIMIT 5"#
    )
    .fetch_all(state.db.as_ref())
    .await
    .unwrap_or_default();

    if !goals.is_empty() {
        counts.goals = goals.len();
        let goal_lines: Vec<String> = goals
            .iter()
            .map(|(title, desc, progress)| {
                let desc_part = desc.as_ref().map(|d| format!(" - {}", d)).unwrap_or_default();
                format!("- {} ({}% complete){}", title, progress, desc_part)
            })
            .collect();
        sections.push(format!("**Active Goals:**\n{}", goal_lines.join("\n")));
    }

    // 2. Active tasks
    let tasks = sqlx::query_as::<_, (String, String)>(
        r#"SELECT title, status FROM tasks
           WHERE status IN ('pending', 'in_progress')
           ORDER BY priority DESC LIMIT 5"#
    )
    .fetch_all(state.db.as_ref())
    .await
    .unwrap_or_default();

    if !tasks.is_empty() {
        counts.tasks = tasks.len();
        let task_lines: Vec<String> = tasks
            .iter()
            .map(|(title, status)| format!("- {} ({})", title, status))
            .collect();
        sections.push(format!("**Current Tasks:**\n{}", task_lines.join("\n")));
    }

    // 3. Corrections (user preferences to remember)
    let corrections = sqlx::query_as::<_, (String,)>(
        r#"SELECT what_is_right FROM corrections
           WHERE confidence >= 0.7
           ORDER BY confidence DESC LIMIT 3"#
    )
    .fetch_all(state.db.as_ref())
    .await
    .unwrap_or_default();

    if !corrections.is_empty() {
        counts.corrections = corrections.len();
        let correction_lines: Vec<String> = corrections
            .iter()
            .map(|(what_is_right,)| format!("- {}", what_is_right))
            .collect();
        sections.push(format!("**Preferences to remember:**\n{}", correction_lines.join("\n")));
    }

    // 4. Working documents
    let docs = sqlx::query_as::<_, (String,)>(
        r#"SELECT context_value FROM work_context
           WHERE context_type = 'working_document'
           ORDER BY updated_at DESC LIMIT 5"#
    )
    .fetch_all(state.db.as_ref())
    .await
    .unwrap_or_default();

    if !docs.is_empty() {
        counts.documents = docs.len();
        // Parse JSON to extract file paths
        let doc_lines: Vec<String> = docs
            .iter()
            .filter_map(|(json_val,)| {
                serde_json::from_str::<serde_json::Value>(json_val)
                    .ok()
                    .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
            })
            .map(|path| format!("- {}", path))
            .collect();
        if !doc_lines.is_empty() {
            sections.push(format!("**Files we're working on:**\n{}", doc_lines.join("\n")));
        }
    }

    if sections.is_empty() {
        return (String::new(), counts);
    }

    let content = format!(
        "<background_context>\n\
        (This is background awareness - only reference when relevant to conversation. \
        Don't steer casual chat toward work.)\n\n\
        {}\n\
        </background_context>",
        sections.join("\n\n")
    );

    (content, counts)
}
