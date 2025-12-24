// src/tools/hotline.rs
// Hotline - Talk to other AI models for collaboration/second opinion
// Uses the unified AdvisoryService for all provider calls
// Supports: GPT-5.2 (default), DeepSeek (chat), Gemini 3 Pro
// Council mode: GPT-5.2 + Gemini 3 Pro, synthesized by DeepSeek Reasoner
// (Opus excluded from council when running in Claude Code MCP context)

use anyhow::Result;
use sqlx::SqlitePool;

use super::proactive;
use super::git_intel;
use crate::advisory::{AdvisoryService, AdvisoryModel, tool_bridge};
use crate::core::SemanticSearch;
use std::sync::Arc;
use super::types::{HotlineRequest, GetProactiveContextRequest, GetRecentCommitsRequest};

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

async fn call_council(message: &str) -> Result<serde_json::Value> {
    let service = AdvisoryService::from_env()?;

    // Exclude Opus since we're already running on Opus in Claude Code MCP context
    // Council = GPT-5.2 + Gemini 3 Pro, synthesized by DeepSeek Reasoner
    let response = service.council(message, Some(AdvisoryModel::Opus45)).await?;

    Ok(response.to_json())
}

// ============================================================================
// Context Injection
// ============================================================================

/// Fetch and format project context for hotline calls
async fn get_project_context(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> Option<String> {
    // Fetch proactive context with generous limits
    let ctx_result = proactive::get_proactive_context(
        db,
        semantic,
        GetProactiveContextRequest {
            files: None,
            topics: None,
            error: None,
            task: None,
            limit_per_category: Some(5),
        },
        project_id,
    ).await;

    let ctx = match ctx_result {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Fetch recent commits
    let commits = git_intel::get_recent_commits(
        db,
        GetRecentCommitsRequest {
            limit: Some(5),
            file_path: None,
            author: None,
        },
    ).await.unwrap_or_default();

    Some(format_context_for_llm(&ctx, &commits, project_name, project_type))
}

/// Format context into a readable string for LLM consumption
fn format_context_for_llm(
    ctx: &serde_json::Value,
    commits: &[serde_json::Value],
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> String {
    let mut parts = vec![];

    // Project header
    if let (Some(name), Some(ptype)) = (project_name, project_type) {
        parts.push(format!("# Project: {} ({})\n", name, ptype));
    }

    // Corrections - full detail
    if let Some(corrections) = ctx["corrections"].as_array() {
        if !corrections.is_empty() {
            parts.push("## Corrections (learned mistakes to avoid)".to_string());
            for c in corrections {
                let wrong = c["what_was_wrong"].as_str().unwrap_or("");
                let right = c["what_is_right"].as_str().unwrap_or("");
                let rationale = c["rationale"].as_str().unwrap_or("");
                parts.push(format!("- **{}** -> {}", wrong, right));
                if !rationale.is_empty() {
                    parts.push(format!("  Rationale: {}", rationale));
                }
            }
            parts.push(String::new());
        }
    }

    // Active goals with progress
    if let Some(goals) = ctx["active_goals"].as_array() {
        if !goals.is_empty() {
            parts.push("## Active Goals".to_string());
            for g in goals {
                let title = g["title"].as_str().unwrap_or("");
                let status = g["status"].as_str().unwrap_or("");
                let progress = g["progress_percent"].as_i64().unwrap_or(0);
                let desc = g["description"].as_str().unwrap_or("");
                parts.push(format!("- **{}** [{}] {}%", title, status, progress));
                if !desc.is_empty() {
                    parts.push(format!("  {}", desc));
                }
            }
            parts.push(String::new());
        }
    }

    // Related decisions
    if let Some(decisions) = ctx["related_decisions"].as_array() {
        if !decisions.is_empty() {
            parts.push("## Past Decisions".to_string());
            for d in decisions {
                let content = d["content"].as_str().unwrap_or("");
                if !content.is_empty() {
                    parts.push(format!("- {}", content));
                }
            }
            parts.push(String::new());
        }
    }

    // Relevant memories
    if let Some(memories) = ctx["relevant_memories"].as_array() {
        if !memories.is_empty() {
            parts.push("## Relevant Context".to_string());
            for m in memories {
                let content = m["content"].as_str().unwrap_or("");
                if !content.is_empty() {
                    parts.push(format!("- {}", content));
                }
            }
            parts.push(String::new());
        }
    }

    // Rejected approaches
    if let Some(rejected) = ctx["rejected_approaches"].as_array() {
        if !rejected.is_empty() {
            parts.push("## Rejected Approaches (don't suggest these)".to_string());
            for r in rejected {
                let approach = r["approach"].as_str().unwrap_or("");
                let reason = r["rejection_reason"].as_str().unwrap_or("");
                if !approach.is_empty() {
                    parts.push(format!("- **{}**: {}", approach, reason));
                }
            }
            parts.push(String::new());
        }
    }

    // Recent commits
    if !commits.is_empty() {
        parts.push("## Recent Commits".to_string());
        for c in commits {
            let msg = c["message"].as_str().unwrap_or("");
            let author = c["author_name"].as_str().unwrap_or("");
            // Truncate long commit messages
            let msg_short = if msg.len() > 80 {
                format!("{}...", &msg[..77])
            } else {
                msg.to_string()
            };
            parts.push(format!("- {} ({})", msg_short, author));
        }
        parts.push(String::new());
    }

    // Similar fixes (error patterns that have been solved before)
    if let Some(fixes) = ctx["similar_fixes"].as_array() {
        if !fixes.is_empty() {
            parts.push("## Similar Past Fixes".to_string());
            for f in fixes {
                let pattern = f["error_pattern"].as_str().unwrap_or("");
                let fix = f["fix_description"].as_str().unwrap_or("");
                if !pattern.is_empty() && !fix.is_empty() {
                    parts.push(format!("- **{}**: {}", pattern, fix));
                }
            }
            parts.push(String::new());
        }
    }

    // Code context - related files and key symbols
    if let Some(code_ctx) = ctx.get("code_context") {
        // Related files (cochange patterns and imports)
        if let Some(related) = code_ctx["related_files"].as_array() {
            if !related.is_empty() {
                parts.push("## Related Files".to_string());
                for r in related.iter().take(5) {
                    let file = r["file"].as_str().unwrap_or("");
                    let relation = r["relation"].as_str().unwrap_or("");
                    let related_to = r["related_to"].as_str().unwrap_or("");
                    if !file.is_empty() {
                        parts.push(format!("- {} ({} of {})", file, relation, related_to));
                    }
                }
                parts.push(String::new());
            }
        }

        // Key symbols in the codebase
        if let Some(symbols) = code_ctx["key_symbols"].as_array() {
            if !symbols.is_empty() {
                parts.push("## Key Symbols".to_string());
                for s in symbols.iter().take(8) {
                    let name = s["name"].as_str().unwrap_or("");
                    let stype = s["type"].as_str().unwrap_or("");
                    let file = s["file"].as_str().unwrap_or("");
                    if !name.is_empty() {
                        parts.push(format!("- `{}` ({}) in {}", name, stype, file));
                    }
                }
                parts.push(String::new());
            }
        }

        // Codebase style guidance
        if let Some(style) = code_ctx.get("codebase_style") {
            if let Some(guidance) = style["guidance"].as_str() {
                parts.push("## Code Style".to_string());
                parts.push(format!("- {}", guidance));
                parts.push(String::new());
            }
        }

        // Improvement suggestions
        if let Some(improvements) = code_ctx["improvement_suggestions"].as_array() {
            if !improvements.is_empty() {
                parts.push("## Suggested Improvements".to_string());
                for i in improvements.iter().take(3) {
                    let symbol = i["symbol_name"].as_str().unwrap_or("");
                    let suggestion = i["suggestion"].as_str().unwrap_or("");
                    if !symbol.is_empty() && !suggestion.is_empty() {
                        parts.push(format!("- `{}`: {}", symbol, suggestion));
                    }
                }
                parts.push(String::new());
            }
        }
    }

    // Call graph relationships
    if let Some(call_graph) = ctx["call_graph"].as_array() {
        if !call_graph.is_empty() {
            parts.push("## Call Relationships".to_string());
            for cg in call_graph.iter().take(8) {
                let caller = cg["caller"].as_str().unwrap_or("");
                let callee = cg["callee"].as_str().unwrap_or("");
                if !caller.is_empty() && !callee.is_empty() {
                    parts.push(format!("- `{}` → `{}`", caller, callee));
                }
            }
            parts.push(String::new());
        }
    }

    // Index status (alert if stale)
    if let Some(index) = ctx.get("index_status") {
        if let Some(stale) = index["stale_files"].as_array() {
            if !stale.is_empty() {
                parts.push("## Index Status".to_string());
                parts.push(format!("⚠️ {} files may be out of date in the index", stale.len()));
                parts.push(String::new());
            }
        }
    }

    parts.join("\n")
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
        if let Some(ctx) = get_project_context(db, semantic, project_id, project_name, project_type).await {
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
        (Some("council"), _) => call_council(&message).await?,
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
