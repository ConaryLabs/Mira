// src/studio/context.rs
// Context building for chat - persona, memories, work context
// Now returns Vec<SystemBlock> for Anthropic prompt caching

use sqlx::SqlitePool;
use tracing::{info, error, debug};

use crate::tools::{memory, types::RecallRequest};
use super::types::{StudioState, WorkspaceEvent, ChatMessage, SystemBlock, WorkContextCounts};

/// Build tiered context as SystemBlocks with cache_control for Anthropic prompt caching
/// Returns Vec<SystemBlock> with strategic cache breakpoints:
/// - Block 1: Persona (1h cache - rarely changes)
/// - Block 2: Work context (5m cache - goals, tasks, corrections)
/// - Block 3: Session context + memories (5m cache - conversation-specific)
pub async fn build_tiered_context(state: &StudioState, conversation_id: &str) -> Vec<SystemBlock> {
    let mut blocks = Vec::new();

    // === Block 1: Persona (1h cache - rarely changes mid-session) ===
    state.emit(WorkspaceEvent::ToolStart {
        tool: "load_persona".to_string(),
        args: None,
    });
    let persona = load_persona(&state.db).await;
    blocks.push(SystemBlock::cached_1h(persona));
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_persona".to_string(),
        result: Some("loaded (1h cache)".to_string()),
        success: true,
    });

    // === Block 2: Work context (5m cache - changes occasionally) ===
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
        blocks.push(SystemBlock::cached(work_context));
    }
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_work_context".to_string(),
        result: Some(format!("{} items (5m cache)", counts.total())),
        success: true,
    });

    // === Block 3: Session context + rolling summary + memories (5m cache) ===
    let mut session_parts = Vec::new();

    // Rolling summary for this conversation
    if let Some(rolling) = load_rolling_summary(&state.db, conversation_id).await {
        state.emit(WorkspaceEvent::Context {
            kind: "rolling_summary".to_string(),
            count: 1,
        });
        session_parts.push(format!(
            "<conversation_history>\nSummary of earlier parts of this conversation:\n{}\n</conversation_history>",
            rolling
        ));
    }

    // Recent session summaries from Claude Code
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
        session_parts.push(format!(
            "<session_context>\nRecent work sessions (what we've been working on):\n{}\n</session_context>",
            sessions
        ));
    }
    state.emit(WorkspaceEvent::ToolEnd {
        tool: "load_sessions".to_string(),
        result: Some(format!("{} sessions", session_count)),
        success: true,
    });

    // Semantic memories based on recent messages
    let recent = get_recent_messages_raw(&state.db, conversation_id, 3).await;
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
            session_parts.push(format!(
                "<memories>\nRelevant details from memory:\n{}\n</memories>",
                memories
            ));
        }
        state.emit(WorkspaceEvent::ToolEnd {
            tool: "semantic_recall".to_string(),
            result: Some(format!("{} memories", memory_count)),
            success: true,
        });
    }

    // Combine all session context into one block with cache control
    if !session_parts.is_empty() {
        blocks.push(SystemBlock::cached(session_parts.join("\n\n")));
    }

    blocks
}

/// Get recent messages from the database as ChatMessage with MessageContent
/// The last user message gets cache_control for incremental caching
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
    let msgs: Vec<(String, String)> = messages.into_iter().rev().collect();
    let len = msgs.len();

    msgs.into_iter().enumerate().map(|(i, (role, content))| {
        // Put cache_control on the last user message for incremental caching
        if i == len - 1 && role == "user" {
            ChatMessage::cached(role, content)
        } else {
            ChatMessage::text(role, content)
        }
    }).collect()
}

/// Get recent messages as raw (role, content) tuples - for internal use
async fn get_recent_messages_raw(db: &SqlitePool, conversation_id: &str, limit: usize) -> Vec<(String, String)> {
    sqlx::query_as::<_, (String, String)>(r#"
        SELECT role, content FROM studio_messages
        WHERE conversation_id = $1
        ORDER BY created_at DESC
        LIMIT $2
    "#)
    .bind(conversation_id)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .unwrap_or_default()
    .into_iter()
    .rev()
    .collect()
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
    let base_persona = sqlx::query_scalar::<_, String>(
        "SELECT content FROM coding_guidelines WHERE category = 'persona' ORDER BY priority DESC LIMIT 1"
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(default_persona);

    // Always append Claude Code integration instructions
    format!("{}\n\n{}", base_persona, claude_code_instructions())
}

/// Instructions for launching Claude Code sessions
fn claude_code_instructions() -> &'static str {
    r#"Claude Code Integration:
When the user wants you to do actual coding work (implement features, fix bugs, write code), you can launch Claude Code to handle it. Use this directive in your response:

[LAUNCH_CC]detailed task description here[/LAUNCH_CC]

The directive will be stripped from your message and Claude Code will start working on the task. The terminal panel will open automatically. Only use this for real coding tasks, not for explaining code or answering questions.

Example: "Let me handle that for you. [LAUNCH_CC]Add a new endpoint /api/users that returns a list of users from the database[/LAUNCH_CC]""#
}

/// Recall memories relevant to the conversation (returns string and count)
async fn recall_relevant_memories_with_count(state: &StudioState, messages: &[(String, String)]) -> (String, usize) {
    let user_messages: Vec<&str> = messages
        .iter()
        .filter(|(role, _)| role == "user")
        .map(|(_, content)| content.as_str())
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
