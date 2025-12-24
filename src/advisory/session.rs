//! Advisory Session Management
//!
//! Handles multi-turn conversations with external LLMs using tiered memory:
//! - Recent turns verbatim (last 6-12 messages)
//! - Session summaries of older content (cached in DB)
//! - Pinned facts/constraints
//! - Decision log

use anyhow::Result;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{AdvisoryService, AdvisoryModel};
use super::provider::AdvisoryMessage as ProviderMessage;
use super::provider::AdvisoryRole;

// ============================================================================
// Constants
// ============================================================================

/// Maximum recent turns to keep verbatim
const MAX_RECENT_TURNS: usize = 6;

/// Token budget allocation (approximate)
const RECENT_BUDGET_PERCENT: f32 = 0.55;   // 55% for recent turns
const SUMMARY_BUDGET_PERCENT: f32 = 0.25;  // 25% for summaries + pins
const CONTEXT_BUDGET_PERCENT: f32 = 0.20;  // 20% for Mira context (injected externally)

/// Default session expiry (24 hours)
const DEFAULT_EXPIRY_HOURS: i64 = 24;

// ============================================================================
// Types
// ============================================================================

/// Session mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Single model conversation
    Single,
    /// Council mode (multiple models)
    Council,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionMode::Single => "single",
            SessionMode::Council => "council",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "single" => SessionMode::Single,
            _ => SessionMode::Council,
        }
    }
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Summarized,
    Archived,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Summarized => "summarized",
            SessionStatus::Archived => "archived",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "summarized" => SessionStatus::Summarized,
            "archived" => SessionStatus::Archived,
            _ => SessionStatus::Active,
        }
    }
}

/// A message in the advisory session
#[derive(Debug, Clone)]
pub struct AdvisoryMessage {
    pub id: String,
    pub turn_number: i32,
    pub role: String,
    pub provider: Option<String>,
    pub content: String,
    pub token_count: Option<i32>,
    pub synthesis_data: Option<String>,
}

/// A pinned constraint or fact
#[derive(Debug, Clone)]
pub struct AdvisoryPin {
    pub id: String,
    pub content: String,
    pub pin_type: String,
    pub source_turn: Option<i32>,
}

/// A decision made during the session
#[derive(Debug, Clone)]
pub struct AdvisoryDecision {
    pub id: String,
    pub decision_type: String,
    pub topic: String,
    pub rationale: Option<String>,
    pub source_turn: Option<i32>,
}

/// Session summary
#[derive(Debug, Clone)]
pub struct AdvisorySummary {
    pub id: String,
    pub summary: String,
    pub turn_range_start: i32,
    pub turn_range_end: i32,
    pub token_estimate: Option<i32>,
}

/// Full session state
#[derive(Debug, Clone)]
pub struct AdvisorySession {
    pub id: String,
    pub project_id: Option<i64>,
    pub topic: Option<String>,
    pub mode: SessionMode,
    pub provider: Option<String>,
    pub status: SessionStatus,
    pub total_turns: i32,
}

// ============================================================================
// Session Operations
// ============================================================================

/// Create a new advisory session
pub async fn create_session(
    db: &SqlitePool,
    project_id: Option<i64>,
    mode: SessionMode,
    provider: Option<&str>,
    topic: Option<&str>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let expires_at = now + (DEFAULT_EXPIRY_HOURS * 3600);

    sqlx::query(
        r#"
        INSERT INTO advisory_sessions (id, project_id, mode, provider, topic, status, created_at, updated_at, expires_at)
        VALUES (?, ?, ?, ?, ?, 'active', ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(project_id)
    .bind(mode.as_str())
    .bind(provider)
    .bind(topic)
    .bind(now)
    .bind(now)
    .bind(expires_at)
    .execute(db)
    .await?;

    Ok(id)
}

/// Get an existing session
pub async fn get_session(db: &SqlitePool, session_id: &str) -> Result<Option<AdvisorySession>> {
    let row = sqlx::query_as::<_, (String, Option<i64>, Option<String>, String, Option<String>, String, i32)>(
        r#"
        SELECT id, project_id, topic, mode, provider, status, total_turns
        FROM advisory_sessions
        WHERE id = ?
        "#
    )
    .bind(session_id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(id, project_id, topic, mode, provider, status, total_turns)| {
        AdvisorySession {
            id,
            project_id,
            topic,
            mode: SessionMode::from_str(&mode),
            provider,
            status: SessionStatus::from_str(&status),
            total_turns,
        }
    }))
}

/// List active sessions for a project
pub async fn list_sessions(
    db: &SqlitePool,
    project_id: Option<i64>,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<AdvisorySession>> {
    let status_filter = if include_archived {
        "1=1"
    } else {
        "status != 'archived'"
    };

    let query = format!(
        r#"
        SELECT id, project_id, topic, mode, provider, status, total_turns
        FROM advisory_sessions
        WHERE ({}) AND (project_id = ? OR ? IS NULL)
        ORDER BY updated_at DESC
        LIMIT ?
        "#,
        status_filter
    );

    let rows = sqlx::query_as::<_, (String, Option<i64>, Option<String>, String, Option<String>, String, i32)>(&query)
        .bind(project_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, project_id, topic, mode, provider, status, total_turns)| {
        AdvisorySession {
            id,
            project_id,
            topic,
            mode: SessionMode::from_str(&mode),
            provider,
            status: SessionStatus::from_str(&status),
            total_turns,
        }
    }).collect())
}

/// Add a message to the session
pub async fn add_message(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
    provider: Option<&str>,
    synthesis_data: Option<&str>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Get current turn number
    let turn: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(turn_number), 0) FROM advisory_messages WHERE session_id = ?"
    )
    .bind(session_id)
    .fetch_one(db)
    .await?;

    let turn_number = if role == "user" {
        turn.0 + 1  // New turn for user messages
    } else {
        turn.0  // Same turn for assistant responses
    };

    // Estimate token count (rough: ~4 chars per token)
    let token_count = (content.len() / 4) as i32;

    sqlx::query(
        r#"
        INSERT INTO advisory_messages (id, session_id, turn_number, role, provider, content, token_count, synthesis_data, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(session_id)
    .bind(turn_number)
    .bind(role)
    .bind(provider)
    .bind(content)
    .bind(token_count)
    .bind(synthesis_data)
    .bind(now)
    .execute(db)
    .await?;

    // Update session
    sqlx::query(
        r#"
        UPDATE advisory_sessions
        SET total_turns = ?, updated_at = ?
        WHERE id = ?
        "#
    )
    .bind(turn_number)
    .bind(now)
    .bind(session_id)
    .execute(db)
    .await?;

    Ok(id)
}

/// Get recent messages (for context assembly)
pub async fn get_recent_messages(
    db: &SqlitePool,
    session_id: &str,
    limit: i64,
) -> Result<Vec<AdvisoryMessage>> {
    let rows = sqlx::query_as::<_, (String, i32, String, Option<String>, String, Option<i32>, Option<String>)>(
        r#"
        SELECT id, turn_number, role, provider, content, token_count, synthesis_data
        FROM advisory_messages
        WHERE session_id = ?
        ORDER BY turn_number DESC, created_at DESC
        LIMIT ?
        "#
    )
    .bind(session_id)
    .bind(limit)
    .fetch_all(db)
    .await?;

    // Reverse to get chronological order
    Ok(rows.into_iter().rev().map(|(id, turn_number, role, provider, content, token_count, synthesis_data)| {
        AdvisoryMessage {
            id,
            turn_number,
            role,
            provider,
            content,
            token_count,
            synthesis_data,
        }
    }).collect())
}

/// Get all messages for a session (for debugging/export)
pub async fn get_all_messages(db: &SqlitePool, session_id: &str) -> Result<Vec<AdvisoryMessage>> {
    let rows = sqlx::query_as::<_, (String, i32, String, Option<String>, String, Option<i32>, Option<String>)>(
        r#"
        SELECT id, turn_number, role, provider, content, token_count, synthesis_data
        FROM advisory_messages
        WHERE session_id = ?
        ORDER BY turn_number ASC, created_at ASC
        "#
    )
    .bind(session_id)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(id, turn_number, role, provider, content, token_count, synthesis_data)| {
        AdvisoryMessage {
            id,
            turn_number,
            role,
            provider,
            content,
            token_count,
            synthesis_data,
        }
    }).collect())
}

/// Get session summaries
pub async fn get_summaries(db: &SqlitePool, session_id: &str) -> Result<Vec<AdvisorySummary>> {
    let rows = sqlx::query_as::<_, (String, String, i32, i32, Option<i32>)>(
        r#"
        SELECT id, summary, turn_range_start, turn_range_end, token_estimate
        FROM advisory_summaries
        WHERE session_id = ?
        ORDER BY turn_range_end DESC
        "#
    )
    .bind(session_id)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(id, summary, turn_range_start, turn_range_end, token_estimate)| {
        AdvisorySummary {
            id,
            summary,
            turn_range_start,
            turn_range_end,
            token_estimate,
        }
    }).collect())
}

/// Get pinned constraints
pub async fn get_pins(db: &SqlitePool, session_id: &str) -> Result<Vec<AdvisoryPin>> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<i32>)>(
        r#"
        SELECT id, content, pin_type, source_turn
        FROM advisory_pins
        WHERE session_id = ?
        ORDER BY created_at ASC
        "#
    )
    .bind(session_id)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(id, content, pin_type, source_turn)| {
        AdvisoryPin {
            id,
            content,
            pin_type,
            source_turn,
        }
    }).collect())
}

/// Add a pin
pub async fn add_pin(
    db: &SqlitePool,
    session_id: &str,
    content: &str,
    pin_type: &str,
    source_turn: Option<i32>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO advisory_pins (id, session_id, content, pin_type, source_turn, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(session_id)
    .bind(content)
    .bind(pin_type)
    .bind(source_turn)
    .bind(now)
    .execute(db)
    .await?;

    Ok(id)
}

/// Get decisions
pub async fn get_decisions(db: &SqlitePool, session_id: &str) -> Result<Vec<AdvisoryDecision>> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<i32>)>(
        r#"
        SELECT id, decision_type, topic, rationale, source_turn
        FROM advisory_decisions
        WHERE session_id = ?
        ORDER BY created_at ASC
        "#
    )
    .bind(session_id)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(id, decision_type, topic, rationale, source_turn)| {
        AdvisoryDecision {
            id,
            decision_type,
            topic,
            rationale,
            source_turn,
        }
    }).collect())
}

/// Add a decision
pub async fn add_decision(
    db: &SqlitePool,
    session_id: &str,
    decision_type: &str,
    topic: &str,
    rationale: Option<&str>,
    source_turn: Option<i32>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO advisory_decisions (id, session_id, decision_type, topic, rationale, source_turn, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(session_id)
    .bind(decision_type)
    .bind(topic)
    .bind(rationale)
    .bind(source_turn)
    .bind(now)
    .execute(db)
    .await?;

    Ok(id)
}

/// Update session status
pub async fn update_status(db: &SqlitePool, session_id: &str, status: SessionStatus) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "UPDATE advisory_sessions SET status = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status.as_str())
    .bind(now)
    .bind(session_id)
    .execute(db)
    .await?;

    Ok(())
}

/// Archive old sessions
pub async fn cleanup_expired_sessions(db: &SqlitePool) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();

    let result = sqlx::query(
        r#"
        UPDATE advisory_sessions
        SET status = 'archived'
        WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < ?
        "#
    )
    .bind(now)
    .execute(db)
    .await?;

    Ok(result.rows_affected() as i64)
}

// ============================================================================
// Context Assembly
// ============================================================================

/// Assembled context for sending to advisory LLMs
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// Recent messages in chronological order
    pub recent_messages: Vec<AdvisoryMessage>,
    /// Summaries of older turns
    pub summaries: Vec<AdvisorySummary>,
    /// Pinned constraints
    pub pins: Vec<AdvisoryPin>,
    /// Decisions made
    pub decisions: Vec<AdvisoryDecision>,
}

/// Assemble context for a session turn
pub async fn assemble_context(db: &SqlitePool, session_id: &str) -> Result<AssembledContext> {
    // Get recent messages (limited)
    let recent_messages = get_recent_messages(db, session_id, MAX_RECENT_TURNS as i64 * 2).await?;

    // Get summaries
    let summaries = get_summaries(db, session_id).await?;

    // Get pins
    let pins = get_pins(db, session_id).await?;

    // Get decisions
    let decisions = get_decisions(db, session_id).await?;

    Ok(AssembledContext {
        recent_messages,
        summaries,
        pins,
        decisions,
    })
}

/// Format context as provider messages for multi-turn
pub fn format_as_history(context: &AssembledContext) -> Vec<ProviderMessage> {
    let mut history = vec![];

    // Add summaries first (oldest context)
    for summary in &context.summaries {
        history.push(ProviderMessage {
            role: AdvisoryRole::User,
            content: format!("[Previous discussion summary (turns {}-{})]\n{}",
                summary.turn_range_start, summary.turn_range_end, summary.summary),
        });
    }

    // Add pins as a system-like message
    if !context.pins.is_empty() {
        let pins_text: Vec<String> = context.pins.iter()
            .map(|p| format!("- [{}] {}", p.pin_type, p.content))
            .collect();
        history.push(ProviderMessage {
            role: AdvisoryRole::User,
            content: format!("[Pinned constraints]\n{}", pins_text.join("\n")),
        });
    }

    // Add decisions
    if !context.decisions.is_empty() {
        let decisions_text: Vec<String> = context.decisions.iter()
            .map(|d| {
                let rationale = d.rationale.as_deref().unwrap_or("");
                format!("- [{}] {}: {}", d.decision_type, d.topic, rationale)
            })
            .collect();
        history.push(ProviderMessage {
            role: AdvisoryRole::User,
            content: format!("[Decisions made]\n{}", decisions_text.join("\n")),
        });
    }

    // Add recent messages
    for msg in &context.recent_messages {
        let role = match msg.role.as_str() {
            "user" => AdvisoryRole::User,
            _ => AdvisoryRole::Assistant,
        };
        history.push(ProviderMessage {
            role,
            content: msg.content.clone(),
        });
    }

    history
}

/// Create a summary of older turns
pub async fn summarize_older_turns(
    db: &SqlitePool,
    session_id: &str,
    service: &AdvisoryService,
) -> Result<Option<String>> {
    // Get all messages
    let all_messages = get_all_messages(db, session_id).await?;

    if all_messages.len() <= MAX_RECENT_TURNS * 2 {
        // Not enough messages to summarize
        return Ok(None);
    }

    // Get existing summaries to know what's already summarized
    let existing_summaries = get_summaries(db, session_id).await?;
    let last_summarized_turn = existing_summaries.first()
        .map(|s| s.turn_range_end)
        .unwrap_or(0);

    // Find turns that need summarizing (older than recent window, not already summarized)
    let messages_to_summarize: Vec<_> = all_messages.iter()
        .filter(|m| m.turn_number > last_summarized_turn)
        .filter(|m| m.turn_number <= (all_messages.last().map(|m| m.turn_number).unwrap_or(0) - MAX_RECENT_TURNS as i32))
        .collect();

    if messages_to_summarize.is_empty() {
        return Ok(None);
    }

    let turn_range_start = messages_to_summarize.first().map(|m| m.turn_number).unwrap_or(0);
    let turn_range_end = messages_to_summarize.last().map(|m| m.turn_number).unwrap_or(0);

    // Format messages for summarization
    let content_to_summarize: String = messages_to_summarize.iter()
        .map(|m| format!("[{}{}]: {}",
            m.role,
            m.provider.as_ref().map(|p| format!(" - {}", p)).unwrap_or_default(),
            m.content
        ))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Use DeepSeek Reasoner to create summary
    let summary_prompt = format!(
        "Summarize this advisory conversation concisely. Focus on:\n\
         1. Key questions asked\n\
         2. Main recommendations given\n\
         3. Any decisions made or rejected\n\
         4. Open issues or pending items\n\n\
         Conversation:\n{}\n\n\
         Provide a structured summary (max 500 words).",
        content_to_summarize
    );

    let response = service.ask(AdvisoryModel::DeepSeekReasoner, &summary_prompt).await?;
    let summary = response.text;

    // Store summary
    let summary_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let message_ids: Vec<String> = messages_to_summarize.iter().map(|m| m.id.clone()).collect();
    let token_estimate = (summary.len() / 4) as i32;

    sqlx::query(
        r#"
        INSERT INTO advisory_summaries (id, session_id, summary, turn_range_start, turn_range_end, message_ids, token_estimate, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&summary_id)
    .bind(session_id)
    .bind(&summary)
    .bind(turn_range_start)
    .bind(turn_range_end)
    .bind(serde_json::to_string(&message_ids)?)
    .bind(token_estimate)
    .bind(now)
    .execute(db)
    .await?;

    Ok(Some(summary))
}
