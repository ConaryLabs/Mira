//! Advisory session management handler

use anyhow::Result;
use sqlx::SqlitePool;

use crate::advisory::session::{
    list_sessions, get_session, get_all_messages, get_pins, get_decisions,
    update_status, SessionStatus, add_pin, add_decision, get_deliberation_progress,
};
use crate::advisory::{AdvisoryModel, AdvisoryUsage};
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
/// For deliberating sessions, includes progress information
pub async fn get(db: &SqlitePool, session_id: &str) -> Result<serde_json::Value> {
    let session = get_session(db, session_id).await?
        .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

    // For deliberating sessions, return progress info prominently
    if session.status == SessionStatus::Deliberating {
        if let Some(progress) = get_deliberation_progress(db, session_id).await? {
            let elapsed = chrono::Utc::now().timestamp() - progress.started_at;
            return Ok(serde_json::json!({
                "status": "deliberating",
                "session_id": session_id,
                "progress": {
                    "current_round": progress.current_round,
                    "max_rounds": progress.max_rounds,
                    "phase": progress.status,
                    "models_responded": progress.models_responded,
                    "elapsed_seconds": elapsed,
                },
                "message": format!(
                    "Round {}/{}: {} models responded. Phase: {:?}",
                    progress.current_round,
                    progress.max_rounds,
                    progress.models_responded.len(),
                    progress.status
                ),
                "model_metadata": AdvisoryModel::council_metadata(),
            }));
        }
    }

    // For completed/failed sessions, include the result if available
    let (deliberation_result, duration_seconds) = if session.status == SessionStatus::Active || session.status == SessionStatus::Failed {
        if let Some(progress) = get_deliberation_progress(db, session_id).await? {
            let duration = progress.duration_seconds();
            (progress.result, duration)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let messages = get_all_messages(db, session_id).await?;
    let pins = get_pins(db, session_id).await?;
    let decisions = get_decisions(db, session_id).await?;

    // Calculate usage and costs per provider
    let mut total_cost_usd = 0.0;
    let mut provider_costs: std::collections::HashMap<String, (AdvisoryUsage, f64)> = std::collections::HashMap::new();

    let message_data: Vec<serde_json::Value> = messages.iter().map(|m| {
        // Parse usage from JSON if available
        let (usage, cost) = if let Some(ref usage_str) = m.usage_json {
            if let Ok(usage) = serde_json::from_str::<AdvisoryUsage>(usage_str) {
                // Get model from provider string
                let model = m.provider.as_deref()
                    .and_then(|p| AdvisoryModel::from_provider_str(p));
                let cost = model.map(|m| m.calculate_cost(&usage)).unwrap_or(0.0);
                total_cost_usd += cost;

                // Aggregate by provider
                if let Some(provider) = &m.provider {
                    let entry = provider_costs.entry(provider.clone()).or_insert((AdvisoryUsage::default(), 0.0));
                    entry.0.input_tokens += usage.input_tokens;
                    entry.0.output_tokens += usage.output_tokens;
                    entry.0.reasoning_tokens += usage.reasoning_tokens;
                    entry.0.cache_read_tokens += usage.cache_read_tokens;
                    entry.0.cache_write_tokens += usage.cache_write_tokens;
                    entry.1 += cost;
                }

                (Some(usage), Some(cost))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let mut msg = serde_json::json!({
            "turn": m.turn_number,
            "role": m.role,
            "provider": m.provider,
            "content": m.content,
        });

        if let Some(usage) = usage {
            msg["usage"] = serde_json::json!(usage);
        }
        if let Some(cost) = cost {
            msg["cost_usd"] = serde_json::json!(cost);
        }

        msg
    }).collect();

    // Build provider usage summary
    let usage_by_provider: serde_json::Value = provider_costs.iter().map(|(provider, (usage, cost))| {
        let model = AdvisoryModel::from_provider_str(provider);
        serde_json::json!({
            "provider": provider,
            "model_id": model.map(|m| m.as_str()),
            "display_name": model.map(|m| m.display_name()),
            "usage": usage,
            "cost_usd": cost,
            "pricing": model.map(|m| {
                let p = m.pricing();
                serde_json::json!({
                    "input_per_m": p.input_per_m,
                    "output_per_m": p.output_per_m,
                    "cache_read_per_m": p.cache_read_per_m,
                    "reasoning_per_m": p.reasoning_per_m,
                })
            }),
        })
    }).collect();

    let mut result = serde_json::json!({
        "session": {
            "id": session.id,
            "topic": session.topic,
            "mode": session.mode.as_str(),
            "status": session.status.as_str(),
            "total_turns": session.total_turns,
        },
        "messages": message_data,
        "pins": pins.iter().map(|p| serde_json::json!({
            "type": p.pin_type,
            "content": p.content,
        })).collect::<Vec<_>>(),
        "decisions": decisions.iter().map(|d| serde_json::json!({
            "type": d.decision_type,
            "topic": d.topic,
            "rationale": d.rationale,
        })).collect::<Vec<_>>(),
        "usage_by_provider": usage_by_provider,
        "total_cost_usd": total_cost_usd,
    });

    // Include deliberation result for completed council sessions
    if let Some(delib_result) = deliberation_result {
        result["deliberation_result"] = delib_result;
        // Add model metadata for frontend rendering
        result["model_metadata"] = AdvisoryModel::council_metadata();
        // Add duration if available
        if let Some(duration) = duration_seconds {
            result["duration_seconds"] = serde_json::json!(duration);
        }
    }

    Ok(result)
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
