// src/tools/hotline.rs
// Hotline - Talk to other AI models for collaboration/second opinion
// Supports: GPT-5.2 (default), DeepSeek (chat), Gemini 3 Pro
// Council mode: GPT-5.2 + DeepSeek Reasoner + Gemini 3 Pro (no Opus - we're already on Opus)
// All providers are called directly via their APIs (no mira-chat dependency)

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::proactive;
use super::git_intel;
use mira_core::semantic::SemanticSearch;
use std::sync::Arc;
use super::types::{HotlineRequest, GetProactiveContextRequest, GetRecentCommitsRequest};

const DOTENV_PATH: &str = "/home/peter/Mira/.env";
const TIMEOUT_SECS: u64 = 120;

// API endpoints
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent";

// ============================================================================
// Environment helpers
// ============================================================================

fn get_env_var(name: &str) -> Option<String> {
    // First try env var
    if let Ok(val) = std::env::var(name) {
        return Some(val);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        let prefix = format!("{}=", name);
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix(&prefix) {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}

// ============================================================================
// OpenAI API (GPT 5.2)
// ============================================================================

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_completion_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Option<Vec<OpenAIChoice>>,
    error: Option<OpenAIError>,
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessageResponse,
}

#[derive(Deserialize)]
struct OpenAIMessageResponse {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIError {
    message: String,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

async fn call_gpt(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("OPENAI_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let client = Client::new();

    let request = OpenAIRequest {
        model: "gpt-5.2".to_string(),
        messages: vec![OpenAIMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        max_completion_tokens: 32000,
        reasoning_effort: Some("high".to_string()),
    };

    let response = client
        .post(OPENAI_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI API error: {} - {}", status, body);
    }

    let api_response: OpenAIResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("OpenAI error: {}", error.message);
    }

    let text = api_response
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "gpt-5.2",
    });

    if let Some(usage) = api_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        });
    }

    Ok(result)
}

// ============================================================================
// DeepSeek API (V3.2)
// ============================================================================

#[derive(Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct DeepSeekMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct DeepSeekResponse {
    choices: Option<Vec<DeepSeekChoice>>,
    error: Option<DeepSeekError>,
    usage: Option<DeepSeekUsage>,
}

#[derive(Deserialize)]
struct DeepSeekChoice {
    message: DeepSeekMessageResponse,
}

#[derive(Deserialize)]
struct DeepSeekMessageResponse {
    content: Option<String>,
}

#[derive(Deserialize)]
struct DeepSeekError {
    message: String,
}

#[derive(Deserialize)]
struct DeepSeekUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

async fn call_deepseek(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("DEEPSEEK_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

    let client = Client::new();

    let request = DeepSeekRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![DeepSeekMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        max_tokens: 8192,
    };

    let response = client
        .post(DEEPSEEK_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("DeepSeek API error: {} - {}", status, body);
    }

    let api_response: DeepSeekResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("DeepSeek error: {}", error.message);
    }

    let text = api_response
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "deepseek",
    });

    if let Some(usage) = api_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        });
    }

    Ok(result)
}

// ============================================================================
// DeepSeek Reasoner (for council - needs more thinking time)
// ============================================================================

async fn call_deepseek_reasoner(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("DEEPSEEK_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

    let client = Client::new();

    let request = DeepSeekRequest {
        model: "deepseek-reasoner".to_string(),
        messages: vec![DeepSeekMessage {
            role: "user".to_string(),
            content: message.to_string(),
        }],
        max_tokens: 8192, // Reasoner has same limit
    };

    let response = client
        .post(DEEPSEEK_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(180)) // Longer timeout for reasoning
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("DeepSeek Reasoner API error: {} - {}", status, body);
    }

    let api_response: DeepSeekResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("DeepSeek Reasoner error: {}", error.message);
    }

    let text = api_response
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "deepseek-reasoner",
    });

    if let Some(usage) = api_response.usage {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        });
    }

    Ok(result)
}

// ============================================================================
// Google API (Gemini 3 Pro)
// ============================================================================

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "thinkingConfig")]
    thinking_config: GeminiThinkingConfig,
}

#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingLevel")]
    thinking_level: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

async fn call_gemini(message: &str) -> Result<serde_json::Value> {
    let api_key = get_env_var("GEMINI_API_KEY")
        .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not set"))?;

    let client = Client::new();
    let url = format!("{}?key={}", GEMINI_API_URL, api_key);

    let request = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart {
                text: message.to_string(),
            }],
        }],
        generation_config: Some(GeminiGenerationConfig {
            thinking_config: GeminiThinkingConfig {
                thinking_level: "high".to_string(),  // Maximum reasoning depth
            },
        }),
    };

    let response = client
        .post(&url)
        .json(&request)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API error: {} - {}", status, body);
    }

    let api_response: GeminiResponse = response.json().await?;

    if let Some(error) = api_response.error {
        anyhow::bail!("Gemini error: {}", error.message);
    }

    let text = api_response
        .candidates
        .and_then(|c| c.into_iter().next())
        .map(|c| {
            c.content
                .parts
                .into_iter()
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let mut result = serde_json::json!({
        "response": text,
        "provider": "gemini",
    });

    if let Some(usage) = api_response.usage_metadata {
        result["tokens"] = serde_json::json!({
            "input": usage.prompt_token_count.unwrap_or(0),
            "output": usage.candidates_token_count.unwrap_or(0),
        });
    }

    Ok(result)
}

// ============================================================================
// Council - All models in parallel
// ============================================================================

async fn call_council(message: &str) -> Result<serde_json::Value> {
    // Run all three in parallel
    // Note: Using deepseek-reasoner for council (reasoning power)
    // Opus is omitted since we're already running on Opus in Claude Code
    let (gpt_result, deepseek_result, gemini_result) = tokio::join!(
        call_gpt(message),
        call_deepseek_reasoner(message),
        call_gemini(message)
    );

    // Format responses, handling errors gracefully
    let gpt = match gpt_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };
    let deepseek = match deepseek_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };
    let gemini = match gemini_result {
        Ok(r) => r["response"].as_str().unwrap_or("(error)").to_string(),
        Err(e) => format!("(error: {})", e),
    };

    Ok(serde_json::json!({
        "council": {
            "gpt-5.2": gpt,
            "deepseek-reasoner": deepseek,
            "gemini-3-pro": gemini,
        }
    }))
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
pub async fn call_mira(
    req: HotlineRequest,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> Result<serde_json::Value> {
    let mut context_parts = vec![];

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

    // Route based on provider
    match req.provider.as_deref() {
        Some("gemini") => call_gemini(&message).await,
        Some("deepseek") => call_deepseek(&message).await,
        Some("council") => call_council(&message).await,
        _ => call_gpt(&message).await, // default to GPT-5.2
    }
}

// Tests removed - require database and API keys.
// Integration tests should be in tests/ directory.
