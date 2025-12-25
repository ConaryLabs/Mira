//! Advisory session management handler

use anyhow::Result;
use sqlx::SqlitePool;

use crate::advisory::session::{
    list_sessions, get_session, get_all_messages, get_pins, get_decisions,
    update_status, SessionStatus, add_pin, add_decision,
};
use crate::tools::AdvisorySessionRequest;

/// List active advisory sessions
pub async fn list(db: &SqlitePool, project_id: Option<i64>, limit: i64) -> Result<serde_json::Value> {
    let sessions = list_sessions(db, project_id, false, limit).await?;
    let result: Vec<serde_json::Value> = sessions.iter().map(|s| {
        serde_json::json!({
            "id": s.id,
            "topic": s.topic,
            "mode": s.mode.as_str(),
            "status": s.status.as_str(),
            "total_turns": s.total_turns,
        })
    }).collect();
    Ok(serde_json::json!({ "sessions": result }))
}

/// Get a specific session with all its messages, pins, and decisions
pub async fn get(db: &SqlitePool, session_id: &str) -> Result<serde_json::Value> {
    let session = get_session(db, session_id).await?
        .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
    let messages = get_all_messages(db, session_id).await?;
    let pins = get_pins(db, session_id).await?;
    let decisions = get_decisions(db, session_id).await?;

    Ok(serde_json::json!({
        "session": {
            "id": session.id,
            "topic": session.topic,
            "mode": session.mode.as_str(),
            "status": session.status.as_str(),
            "total_turns": session.total_turns,
        },
        "messages": messages.iter().map(|m| serde_json::json!({
            "turn": m.turn_number,
            "role": m.role,
            "provider": m.provider,
            "content": m.content,
        })).collect::<Vec<_>>(),
        "pins": pins.iter().map(|p| serde_json::json!({
            "type": p.pin_type,
            "content": p.content,
        })).collect::<Vec<_>>(),
        "decisions": decisions.iter().map(|d| serde_json::json!({
            "type": d.decision_type,
            "topic": d.topic,
            "rationale": d.rationale,
        })).collect::<Vec<_>>(),
    }))
}

/// Close/archive a session
pub async fn close(db: &SqlitePool, session_id: &str) -> Result<serde_json::Value> {
    update_status(db, session_id, SessionStatus::Archived).await?;
    Ok(serde_json::json!({ "status": "closed", "session_id": session_id }))
}

/// Pin content to a session
pub async fn pin(db: &SqlitePool, session_id: &str, content: &str, pin_type: &str) -> Result<serde_json::Value> {
    add_pin(db, session_id, content, pin_type, None).await?;
    Ok(serde_json::json!({ "status": "pinned", "content": content }))
}

/// Record a decision in a session
pub async fn decide(
    db: &SqlitePool,
    session_id: &str,
    decision_type: &str,
    topic: &str,
    rationale: Option<&str>,
) -> Result<serde_json::Value> {
    add_decision(db, session_id, decision_type, topic, rationale, None).await?;
    Ok(serde_json::json!({ "status": "recorded", "topic": topic }))
}

/// Dispatch advisory session action
pub async fn handle(db: &SqlitePool, project_id: Option<i64>, req: &AdvisorySessionRequest) -> Result<serde_json::Value> {
    match req.action.as_str() {
        "list" => list(db, project_id, req.limit.unwrap_or(10)).await,
        "get" => {
            let session_id = req.session_id.as_ref()
                .ok_or_else(|| anyhow::anyhow!("session_id required"))?;
            get(db, session_id).await
        }
        "close" => {
            let session_id = req.session_id.as_ref()
                .ok_or_else(|| anyhow::anyhow!("session_id required"))?;
            close(db, session_id).await
        }
        "pin" => {
            let session_id = req.session_id.as_ref()
                .ok_or_else(|| anyhow::anyhow!("session_id required"))?;
            let content = req.content.as_ref()
                .ok_or_else(|| anyhow::anyhow!("content required for pin"))?;
            let pin_type = req.pin_type.as_deref().unwrap_or("constraint");
            pin(db, session_id, content, pin_type).await
        }
        "decide" => {
            let session_id = req.session_id.as_ref()
                .ok_or_else(|| anyhow::anyhow!("session_id required"))?;
            let decision_type = req.decision_type.as_ref()
                .ok_or_else(|| anyhow::anyhow!("decision_type required"))?;
            let topic = req.topic.as_ref()
                .ok_or_else(|| anyhow::anyhow!("topic required"))?;
            decide(db, session_id, decision_type, topic, req.rationale.as_deref()).await
        }
        action => Err(anyhow::anyhow!("Unknown action: {}. Use list/get/close/pin/decide", action)),
    }
}
