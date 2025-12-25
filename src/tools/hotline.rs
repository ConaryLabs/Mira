// src/tools/hotline.rs
// Hotline - Talk to other AI models for collaboration/second opinion
// Uses the unified AdvisoryService for all provider calls
// Supports: GPT-5.2 (default), DeepSeek Reasoner, Gemini 3 Pro, Opus 4.5
// All providers support agentic tool calling with 10 read-only Mira tools
//
// Council mode: Multi-round deliberation with all 3 models (GPT, Gemini, Opus)
// - Up to 4 rounds of actual discussion (stops early on consensus)
// - DeepSeek Reasoner moderates between rounds, identifying disagreements
// - All models participate, including Opus (fresh perspective even in MCP context)

use anyhow::Result;
use sqlx::SqlitePool;

use crate::advisory::{AdvisoryService, AdvisoryModel, tool_bridge, context};
use crate::core::SemanticSearch;
use std::sync::Arc;
use super::types::HotlineRequest;

// ============================================================================
// Provider Functions (using AdvisoryService)
// ============================================================================

async fn call_gpt(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;
    let response = service.ask(AdvisoryModel::Gpt52, message).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "gpt-5.2",
    }))
}

async fn call_gpt_with_tools(
    message: &str,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Create tool context
    let mut ctx = tool_bridge::ToolContext::new(
        Arc::new(db.clone()),
        semantic.clone(),
        project_id,
    );

    let response = service.ask_with_tools(
        AdvisoryModel::Gpt52,
        message,
        Some("You are an AI assistant with access to Mira's read-only tools. \
              Use the tools to gather relevant context before answering. \
              Available tools: recall (search memories), get_corrections (user preferences), \
              get_goals (active goals), semantic_code_search (find code), get_symbols (file analysis), \
              find_similar_fixes (past error solutions), get_related_files, get_recent_commits, search_commits.".to_string()),
        &mut ctx,
    ).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "gpt-5.2",
        "tools_used": ctx.tracker.session_total,
    }))
}

async fn call_deepseek(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;
    let response = service.ask(AdvisoryModel::DeepSeekReasoner, message).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "deepseek-reasoner",
    }))
}

async fn call_deepseek_with_tools(
    message: &str,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Create tool context
    let mut ctx = tool_bridge::ToolContext::new(
        Arc::new(db.clone()),
        semantic.clone(),
        project_id,
    );

    let response = service.ask_with_tools(
        AdvisoryModel::DeepSeekReasoner,
        message,
        Some("You are an AI assistant with access to Mira's read-only tools. \
              Use the tools to gather relevant context before answering. \
              Available tools: recall (search memories), get_corrections (user preferences), \
              get_goals (active goals), semantic_code_search (find code), get_symbols (file analysis), \
              find_similar_fixes (past error solutions), get_related_files, get_recent_commits, search_commits, list_tasks.".to_string()),
        &mut ctx,
    ).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "deepseek-reasoner",
        "tools_used": ctx.tracker.session_total,
    }))
}

async fn call_gemini(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;
    let response = service.ask(AdvisoryModel::Gemini3Pro, message).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "gemini-3-pro",
    }))
}

async fn call_gemini_with_tools(
    message: &str,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Create tool context
    let mut ctx = tool_bridge::ToolContext::new(
        Arc::new(db.clone()),
        semantic.clone(),
        project_id,
    );

    let response = service.ask_with_tools(
        AdvisoryModel::Gemini3Pro,
        message,
        Some("You are an AI assistant with access to Mira's read-only tools. \
              Use the tools to gather relevant context before answering. \
              Available tools: recall (search memories), get_corrections (user preferences), \
              get_goals (active goals), semantic_code_search (find code), get_symbols (file analysis), \
              find_similar_fixes (past error solutions), get_related_files, get_recent_commits, search_commits.".to_string()),
        &mut ctx,
    ).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "gemini-3-pro",
        "tools_used": ctx.tracker.session_total,
    }))
}

async fn call_opus(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;
    let response = service.ask(AdvisoryModel::Opus45, message).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "opus-4.5",
    }))
}

async fn call_opus_with_tools(
    message: &str,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Create tool context
    let mut ctx = tool_bridge::ToolContext::new(
        Arc::new(db.clone()),
        semantic.clone(),
        project_id,
    );

    let response = service.ask_with_tools(
        AdvisoryModel::Opus45,
        message,
        Some("You are an AI assistant with access to Mira's read-only tools. \
              Use the tools to gather relevant context before answering. \
              Available tools: recall (search memories), get_corrections (user preferences), \
              get_goals (active goals), semantic_code_search (find code), get_symbols (file analysis), \
              find_similar_fixes (past error solutions), get_related_files, get_recent_commits, search_commits, list_tasks.".to_string()),
        &mut ctx,
    ).await?;

    Ok(serde_json::json!({
        "response": response.text,
        "provider": "opus-4.5",
        "tools_used": ctx.tracker.session_total,
    }))
}

/// Spawn async council deliberation - returns immediately with session info
///
/// Deliberation runs in background, progress tracked in database.
/// Use advisory_session(action: "get", session_id: ...) to check status.
async fn spawn_council_deliberation(
    message: String,
    db: SqlitePool,
    session_id: String,
) -> Result<serde_json::Value> {
    use crate::advisory::session::{SessionStatus, update_status, DeliberationProgress, update_deliberation_progress};

    // Set session status to Deliberating
    update_status(&db, &session_id, SessionStatus::Deliberating).await?;

    // Initialize progress
    let progress = DeliberationProgress::new(4); // max_rounds from default config
    update_deliberation_progress(&db, &session_id, &progress).await?;

    // Spawn the deliberation task in background
    let db_clone = db.clone();
    let session_id_clone = session_id.clone();
    let message_clone = message.clone();

    tokio::spawn(async move {
        tracing::info!(session_id = %session_id_clone, "Starting background council deliberation");

        let result = async {
            let service = AdvisoryService::from_env()?;
            service.council_deliberate_with_progress(
                &message_clone,
                None,
                &db_clone,
                &session_id_clone,
            ).await
        }.await;

        match result {
            Ok(_) => {
                // Progress already updated by council_deliberate_with_progress
                // Just update session status to Active (deliberation complete)
                let _ = update_status(&db_clone, &session_id_clone, SessionStatus::Active).await;
                tracing::info!(session_id = %session_id_clone, "Council deliberation complete");
            }
            Err(e) => {
                tracing::error!(session_id = %session_id_clone, error = %e, "Council deliberation failed");
                let _ = update_status(&db_clone, &session_id_clone, SessionStatus::Failed).await;
            }
        }
    });

    // Return immediately with session info
    Ok(serde_json::json!({
        "status": "deliberating",
        "session_id": session_id,
        "message": "Council deliberation started in background. Use advisory_session(action: 'get', session_id: '...') to check progress.",
        "provider": "council"
    }))
}

/// Legacy single-shot council (for backward compatibility if needed)
#[allow(dead_code)]
async fn call_council_single_shot(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Single-shot: exclude Opus in MCP context, GPT + Gemini only
    let response = service.council(message, Some(AdvisoryModel::Opus45)).await?;

    Ok(response.to_json())
}

// ============================================================================
// Public API
// ============================================================================

/// Call Mira hotline - talk to another AI model
/// Providers: openai (GPT-5.2, default), deepseek, gemini, council (all three)
///
/// If inject_context is true (default), automatically injects project context
/// (corrections, goals, decisions, memories, rejected approaches, recent commits)
///
/// If session_id is provided, resumes a multi-turn conversation with history.
pub async fn call_mira(
    req: HotlineRequest,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> Result<serde_json::Value> {
    use crate::advisory::session::{
        SessionMode, assemble_context, format_as_history,
        add_message, create_session, get_session, summarize_older_turns,
    };

    let mut context_parts = vec![];

    // Handle session - create or resume
    let session_id = if let Some(sid) = &req.session_id {
        // Resume existing session
        if get_session(db, sid).await?.is_none() {
            anyhow::bail!("Session {} not found", sid);
        }
        sid.clone()
    } else if req.provider.as_deref() == Some("council") {
        // Auto-create session for council mode
        let mode = SessionMode::Council;
        create_session(db, project_id, mode, None, Some("Advisory council session")).await?
    } else {
        // No session for single-shot calls
        String::new()
    };

    let has_session = !session_id.is_empty();

    // If we have a session, get session context and add user message
    // Note: session_history is built for future use with history-aware API calls
    let _session_history = if has_session {
        // Add user message to session
        add_message(db, &session_id, "user", &req.message, None, None).await?;

        // Check if we need to summarize older turns
        let service = AdvisoryService::from_env()?;
        let _ = summarize_older_turns(db, &session_id, &service).await;

        // Get assembled context
        let ctx = assemble_context(db, &session_id).await?;

        // Format as history for the API call (excludes the current message we just added)
        let history = format_as_history(&ctx);

        // Build session context string for non-history-aware calls
        let mut session_ctx = vec![];
        for summary in &ctx.summaries {
            session_ctx.push(format!("[Previous discussion (turns {}-{})]: {}",
                summary.turn_range_start, summary.turn_range_end, summary.summary));
        }
        if !ctx.pins.is_empty() {
            let pins: Vec<String> = ctx.pins.iter()
                .map(|p| format!("- [{}] {}", p.pin_type, p.content))
                .collect();
            session_ctx.push(format!("[Pinned constraints]\n{}", pins.join("\n")));
        }
        if !ctx.decisions.is_empty() {
            let decisions: Vec<String> = ctx.decisions.iter()
                .map(|d| format!("- [{}] {}", d.decision_type, d.topic))
                .collect();
            session_ctx.push(format!("[Decisions]\n{}", decisions.join("\n")));
        }

        if !session_ctx.is_empty() {
            context_parts.push(session_ctx.join("\n\n"));
        }

        Some(history)
    } else {
        None
    };

    // Auto-inject project context unless explicitly disabled
    if req.inject_context.unwrap_or(true) {
        if let Some(ctx) = context::get_project_context(db, semantic, project_id, project_name, project_type).await {
            if !ctx.trim().is_empty() {
                context_parts.push(ctx);
            }
        }
    }

    // Add manual context if provided
    if let Some(ctx) = &req.context {
        context_parts.push(ctx.clone());
    }

    // Build final message
    let message = if context_parts.is_empty() {
        req.message.clone()
    } else {
        format!("{}\n\n---\n\n{}", context_parts.join("\n\n"), req.message)
    };

    // Route based on provider (and tools if enabled)
    let enable_tools = req.enable_tools.unwrap_or(false);
    let mut result = match (req.provider.as_deref(), enable_tools) {
        (Some("gemini"), true) => call_gemini_with_tools(&message, db, semantic, project_id).await?,
        (Some("gemini"), false) => call_gemini(&message).await?,
        (Some("deepseek"), true) => call_deepseek_with_tools(&message, db, semantic, project_id).await?,
        (Some("deepseek"), false) => call_deepseek(&message).await?,
        (Some("opus"), true) => call_opus_with_tools(&message, db, semantic, project_id).await?,
        (Some("opus"), false) => call_opus(&message).await?,
        (Some("council"), _) => {
            // Council mode: spawn async deliberation, return immediately
            return spawn_council_deliberation(message, db.clone(), session_id).await;
        },
        (_, true) => call_gpt_with_tools(&message, db, semantic, project_id).await?,
        (_, false) => call_gpt(&message).await?,
    };

    // If we have a session, store the response and add session_id to result
    if has_session {
        // Store assistant response(s) in session
        if let Some(council) = result.get("council") {
            // Council mode - store each response
            if let Some(obj) = council.as_object() {
                for (provider, response) in obj {
                    if let Some(text) = response.as_str() {
                        add_message(db, &session_id, "assistant", text, Some(provider), None).await?;
                    }
                }
            }
            // Store synthesis if present
            if let Some(synthesis) = result.get("synthesis").and_then(|s| s.as_str()) {
                add_message(db, &session_id, "synthesis", synthesis, Some("deepseek-reasoner"), None).await?;
            }
        } else if let Some(response) = result.get("response").and_then(|r| r.as_str()) {
            // Single provider - store the response
            let provider = result.get("provider").and_then(|p| p.as_str());
            add_message(db, &session_id, "assistant", response, provider, None).await?;
        }

        // Add session_id to result
        result["session_id"] = serde_json::Value::String(session_id);
    }

    Ok(result)
}

// Tests removed - require database and API keys.
// Integration tests should be in tests/ directory.
