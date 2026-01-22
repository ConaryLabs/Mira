// crates/mira-server/src/tools/core/experts.rs
// Agentic expert sub-agents powered by LLM providers with tool access

use super::ToolContext;
use crate::db::Database;
use crate::indexer;
use crate::llm::{LlmClient, Message, PromptBuilder, Tool, ToolCall};
use crate::search::{embedding_to_bytes, find_callers, find_callees, hybrid_search};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Maximum iterations for the agentic loop
const MAX_ITERATIONS: usize = 100;

/// Timeout for the entire expert consultation (including all tool calls)
const EXPERT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes for multi-turn with reasoning models

/// Timeout for individual LLM calls (6 minutes for reasoning models like DeepSeek)
const LLM_CALL_TIMEOUT: Duration = Duration::from_secs(360);

/// Maximum concurrent expert consultations (prevents rate limit exhaustion)
const MAX_CONCURRENT_EXPERTS: usize = 3;

/// Timeout for parallel expert consultation (longer than single expert to allow queuing)
const PARALLEL_EXPERT_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes for reasoning models

/// Expert roles available for consultation
#[derive(Debug, Clone, Copy)]
pub enum ExpertRole {
    Architect,
    PlanReviewer,
    ScopeAnalyst,
    CodeReviewer,
    Security,
}

impl ExpertRole {
    /// Get the system prompt for this expert role
    /// Checks database for custom prompt first, falls back to default
    pub fn system_prompt(&self, db: &Database) -> String {
        let role_key = self.db_key();

        // Get role instructions (custom or default)
        let role_instructions = if let Ok(Some(custom_prompt)) = db.get_custom_prompt(role_key) {
            custom_prompt
        } else {
            match self {
                ExpertRole::Architect => ARCHITECT_PROMPT,
                ExpertRole::PlanReviewer => PLAN_REVIEWER_PROMPT,
                ExpertRole::ScopeAnalyst => SCOPE_ANALYST_PROMPT,
                ExpertRole::CodeReviewer => CODE_REVIEWER_PROMPT,
                ExpertRole::Security => SECURITY_PROMPT,
            }.to_string()
        };

        // Build standardized prompt with static prefix and tool guidance
        PromptBuilder::new(role_instructions)
            .with_tool_guidance()
            .build_system_prompt()
    }

    /// Database key for this expert role
    pub fn db_key(&self) -> &'static str {
        match self {
            ExpertRole::Architect => "architect",
            ExpertRole::PlanReviewer => "plan_reviewer",
            ExpertRole::ScopeAnalyst => "scope_analyst",
            ExpertRole::CodeReviewer => "code_reviewer",
            ExpertRole::Security => "security",
        }
    }

    /// Display name for this expert
    pub fn name(&self) -> &'static str {
        match self {
            ExpertRole::Architect => "Architect",
            ExpertRole::PlanReviewer => "Plan Reviewer",
            ExpertRole::ScopeAnalyst => "Scope Analyst",
            ExpertRole::CodeReviewer => "Code Reviewer",
            ExpertRole::Security => "Security Analyst",
        }
    }

    /// Get role from database key
    pub fn from_db_key(key: &str) -> Option<Self> {
        match key {
            "architect" => Some(ExpertRole::Architect),
            "plan_reviewer" => Some(ExpertRole::PlanReviewer),
            "scope_analyst" => Some(ExpertRole::ScopeAnalyst),
            "code_reviewer" => Some(ExpertRole::CodeReviewer),
            "security" => Some(ExpertRole::Security),
            _ => None,
        }
    }

    /// List all available roles
    pub fn all() -> &'static [ExpertRole] {
        &[
            ExpertRole::Architect,
            ExpertRole::PlanReviewer,
            ExpertRole::ScopeAnalyst,
            ExpertRole::CodeReviewer,
            ExpertRole::Security,
        ]
    }
}

// Expert system prompts

const ARCHITECT_PROMPT: &str = r#"You are a software architect specializing in system design and technical decisions.

Your role:
- Analyze designs and architectural decisions
- Identify scalability, maintainability, performance issues
- Recommend patterns with clear tradeoffs
- Debug complex architectural problems
- Suggest refactoring strategies

When responding:
1. Start with key recommendation
2. Explain reasoning
3. Present alternatives with tradeoffs
4. Be specific - reference patterns or technologies
5. Prioritize issues by impact

You are advisory - analyze and recommend, not implement."#;

const PLAN_REVIEWER_PROMPT: &str = r#"You are a technical lead reviewing implementation plans before coding.

Your role:
- Validate plan completeness
- Identify risks, gaps, blockers
- Check for missing edge cases or error handling
- Assess fit with codebase and constraints
- Provide go/no-go assessment with specific concerns

When responding:
1. Give overall assessment (ready/needs work/major concerns)
2. List specific risks or gaps
3. Suggest improvements or clarifications needed
4. Highlight dependencies or prerequisites
5. Note what's done well

Be constructive but thorough."#;

const SCOPE_ANALYST_PROMPT: &str = r#"You are an analyst finding missing, unclear, or risky requirements and plans.

Your role:
- Detect ambiguity in requirements
- Identify unstated assumptions
- Find edge cases and boundary conditions
- Ask questions needed before implementation
- Highlight areas where "it depends" needs resolution

When responding:
1. List questions needing answers
2. Identify assumptions (explicit and implicit)
3. Highlight edge cases not addressed
4. Note scope creep risks or unclear boundaries
5. Suggest additional information needed

Surface unknowns early."#;

const CODE_REVIEWER_PROMPT: &str = r#"You are a code reviewer focused on correctness, quality, and maintainability.

Your role:
- Find bugs, logic errors, runtime issues
- Identify code quality concerns (complexity, duplication, naming)
- Check error handling and edge cases
- Assess test coverage needs
- Suggest specific improvements

When responding:
1. List issues by severity (critical/major/minor/nit)
2. For each issue, explain why it's a problem
3. Provide specific fix suggestions
4. Highlight patterns (good or bad)
5. Note areas needing additional testing

Be specific - reference line numbers, function names, concrete suggestions."#;

const SECURITY_PROMPT: &str = r#"You are a security engineer reviewing code and designs for vulnerabilities.

Your role:
- Identify security vulnerabilities (injection, auth, data exposure, etc.)
- Assess attack vectors and likelihood/impact
- Check secure coding practices
- Review authentication, authorization, data handling
- Recommend hardening measures

When responding:
1. List findings by severity (critical/high/medium/low)
2. For each finding:
   - Describe vulnerability
   - Explain potential impact
   - Provide remediation steps
3. Note security best practices being followed
4. Suggest additional security measures if needed

Focus on actionable findings."#;


/// Define the tools available to experts
fn get_expert_tools() -> Vec<Tool> {
    vec![
        Tool::function(
            "search_code",
            "Search for code by meaning. Use this to find relevant code snippets, functions, or patterns.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of what you're looking for (e.g., 'authentication middleware', 'error handling in API routes')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "get_symbols",
            "Get the structure of a file - lists all functions, structs, classes, etc.",
            json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file (relative to project root)"
                    }
                },
                "required": ["file_path"]
            }),
        ),
        Tool::function(
            "read_file",
            "Read the contents of a specific file or a range of lines.",
            json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file (relative to project root)"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Starting line number (1-indexed, optional)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Ending line number (inclusive, optional)"
                    }
                },
                "required": ["file_path"]
            }),
        ),
        Tool::function(
            "find_callers",
            "Find all functions that call a given function.",
            json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callers for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10)",
                        "default": 10
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "find_callees",
            "Find all functions that a given function calls.",
            json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callees for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10)",
                        "default": 10
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "recall",
            "Recall past decisions, context, or preferences stored in memory.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to search for in memory (e.g., 'authentication approach', 'database schema decisions')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
    ]
}

/// Execute a tool call and return the result
async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_call: &ToolCall,
) -> String {
    let args: Value = serde_json::from_str(&tool_call.function.arguments)
        .unwrap_or(json!({}));

    let result = match tool_call.function.name.as_str() {
        "search_code" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            execute_search_code(ctx, query, limit).await
        }
        "get_symbols" => {
            let file_path = args["file_path"].as_str().unwrap_or("");
            execute_get_symbols(ctx, file_path).await
        }
        "read_file" => {
            let file_path = args["file_path"].as_str().unwrap_or("");
            let start_line = args["start_line"].as_u64().map(|n| n as usize);
            let end_line = args["end_line"].as_u64().map(|n| n as usize);
            execute_read_file(ctx, file_path, start_line, end_line).await
        }
        "find_callers" => {
            let function_name = args["function_name"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            execute_find_callers(ctx, function_name, limit).await
        }
        "find_callees" => {
            let function_name = args["function_name"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            execute_find_callees(ctx, function_name, limit).await
        }
        "recall" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            execute_recall(ctx, query, limit).await
        }
        _ => format!("Unknown tool: {}", tool_call.function.name),
    };

    result
}

async fn execute_search_code<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());

    match hybrid_search(
        ctx.db(),
        ctx.embeddings(),
        query,
        project_id,
        project_path.as_deref(),
        limit,
    ).await {
        Ok(result) => {
            if result.results.is_empty() {
                "No code matches found.".to_string()
            } else {
                let mut output = format!("Found {} results:\n\n", result.results.len());
                for r in result.results {
                    // Truncate content if too long
                    let content_preview = if r.content.len() > 2000 {
                        format!("{}\n... (truncated)", &r.content[..2000])
                    } else {
                        r.content
                    };
                    output.push_str(&format!("### {}\n```\n{}\n```\n\n", r.file_path, content_preview));
                }
                output
            }
        }
        Err(e) => format!("Search failed: {}", e),
    }
}

async fn execute_get_symbols<C: ToolContext>(_ctx: &C, file_path: &str) -> String {
    let project = _ctx.get_project().await;

    // Build full path
    let full_path = if let Some(ref proj) = project {
        if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("{}/{}", proj.path, file_path)
        }
    } else {
        file_path.to_string()
    };

    let path = Path::new(&full_path);
    if !path.exists() {
        return format!("File not found: {}", file_path);
    }

    match indexer::extract_symbols(path) {
        Ok(symbols) => {
            if symbols.is_empty() {
                format!("No symbols found in {}", file_path)
            } else {
                let mut output = format!("{} symbols in {}:\n", symbols.len(), file_path);
                for s in symbols.iter().take(50) { // Increased limit slightly, but capped
                    let lines = if s.start_line == s.end_line {
                        format!("line {}", s.start_line)
                    } else {
                        format!("lines {}-{}", s.start_line, s.end_line)
                    };
                    output.push_str(&format!("  {} ({}) {}\n", s.name, s.symbol_type, lines));
                }
                if symbols.len() > 50 {
                    output.push_str(&format!("  ... and {} more\n", symbols.len() - 50));
                }
                output
            }
        }
        Err(e) => format!("Failed to get symbols: {}", e),
    }
}

async fn execute_read_file<C: ToolContext>(
    ctx: &C,
    file_path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> String {
    let project = ctx.get_project().await;

    // Build full path
    let full_path = if let Some(ref proj) = project {
        if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("{}/{}", proj.path, file_path)
        }
    } else {
        file_path.to_string()
    };

    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = start_line.unwrap_or(1).saturating_sub(1);
            let mut end = end_line.unwrap_or(lines.len()).min(lines.len());
            
            // Cap output at 2000 lines max
            let max_lines = 2000;
            let mut truncated = false;
            
            if end - start > max_lines {
                end = start + max_lines;
                truncated = true;
            }

            if start >= lines.len() {
                return format!("Start line {} exceeds file length ({})", start + 1, lines.len());
            }

            let selected: Vec<String> = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{:4} | {}", start + i + 1, line))
                .collect();

            let mut output = format!("{}:\n{}", file_path, selected.join("\n"));
            if truncated {
                output.push_str("\n... (truncated, use start_line/end_line to read more)");
            }
            output
        }
        Err(e) => format!("Failed to read {}: {}", file_path, e),
    }
}

async fn execute_find_callers<C: ToolContext>(ctx: &C, function_name: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;
    let fn_name = function_name.to_string();

    let callers = ctx
        .pool()
        .interact(move |conn| Ok(find_callers(conn, project_id, &fn_name, limit)))
        .await
        .unwrap_or_default();

    if callers.is_empty() {
        format!("No callers found for `{}`", function_name)
    } else {
        let mut output = format!("Functions that call `{}`:\n", function_name);
        for caller in callers {
            output.push_str(&format!(
                "  {} in {} ({}x)\n",
                caller.symbol_name, caller.file_path, caller.call_count
            ));
        }
        output
    }
}

async fn execute_find_callees<C: ToolContext>(ctx: &C, function_name: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;
    let fn_name = function_name.to_string();

    let callees = ctx
        .pool()
        .interact(move |conn| Ok(find_callees(conn, project_id, &fn_name, limit)))
        .await
        .unwrap_or_default();

    if callees.is_empty() {
        format!("No callees found for `{}`", function_name)
    } else {
        let mut output = format!("Functions that `{}` calls:\n", function_name);
        for callee in callees {
            output.push_str(&format!("  {} ({}x)\n", callee.symbol_name, callee.call_count));
        }
        output
    }
}

async fn execute_recall<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;

    // Try semantic recall if embeddings available
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);

            // Run vector search via connection pool
            let results: Result<Vec<(i64, String, f32)>, String> = ctx
                .pool()
                .interact(move |conn| {
                    use rusqlite::params;
                    let mut stmt = conn
                        .prepare(
                            "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance
                             FROM vec_memory v
                             JOIN memory_facts f ON v.fact_id = f.id
                             WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
                             ORDER BY distance
                             LIMIT ?3",
                        )
                        .map_err(|e| anyhow::anyhow!(e))?;

                    let results: Vec<(i64, String, f32)> = stmt
                        .query_map(params![embedding_bytes, project_id, limit as i64], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .map_err(|e| anyhow::anyhow!(e))?
                        .filter_map(|r| r.ok())
                        .collect();

                    Ok(results)
                })
                .await
                .map_err(|e| e.to_string());

            if let Ok(results) = results {
                if !results.is_empty() {
                    let mut output = format!("Found {} relevant memories:\n\n", results.len());
                    for (id, content, distance) in results {
                        let score = 1.0 - distance;
                        let preview = if content.len() > 150 {
                            format!("{}...", &content[..150])
                        } else {
                            content
                        };
                        output.push_str(&format!("[{}] (score: {:.2}) {}\n", id, score, preview));
                    }
                    return output;
                }
            }
        }
    }

    // Fallback to keyword search via connection pool
    let query_owned = query.to_string();
    let result = ctx
        .pool()
        .interact(move |conn| {
            search_memories_sync(conn, project_id, &query_owned, limit)
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string());

    match result {
        Ok(memories) => {
            if memories.is_empty() {
                "No relevant memories found.".to_string()
            } else {
                let mut output = format!("Found {} memories:\n\n", memories.len());
                for mem in memories {
                    let preview = if mem.content.len() > 150 {
                        format!("{}...", &mem.content[..150])
                    } else {
                        mem.content
                    };
                    output.push_str(&format!("[{}] {}\n", mem.id, preview));
                }
                output
            }
        }
        Err(e) => format!("Recall failed: {}", e),
    }
}

/// Sync helper: search memories by text (for use inside run_blocking)
fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    limit: usize,
) -> Result<Vec<mira_types::MemoryFact>, String> {
    use rusqlite::params;

    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{}%", escaped);

    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ? ESCAPE '\\'
             ORDER BY updated_at DESC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id, pattern, limit as i64], |row| {
            Ok(mira_types::MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Build the user prompt from context and optional question
fn build_user_prompt(context: &str, question: Option<&str>) -> String {
    let context_section = if context.is_empty() {
        String::new()
    } else {
        format!("Initial context provided:\n```\n{}\n```\n\n", context)
    };

    match question {
        Some(q) => format!(
            "{}Task: {}\n\nUse the available tools to explore the codebase and gather the information you need, then provide your analysis.",
            context_section, q
        ),
        None => format!(
            "{}Please analyze the codebase. Use the available tools to explore and gather context, then provide your analysis.",
            context_section
        ),
    }
}

/// Format the expert response including reasoning and tool usage summary
fn format_expert_response(
    expert: ExpertRole,
    result: crate::llm::ChatResult,
    tool_calls_made: usize,
    iterations: usize,
) -> String {
    let mut output = String::new();

    // Add expert header
    output.push_str(&format!("## {} Analysis\n\n", expert.name()));

    // Add exploration summary
    if tool_calls_made > 0 {
        output.push_str(&format!(
            "*Explored codebase: {} tool calls across {} iterations*\n\n",
            tool_calls_made, iterations
        ));
    }

    // Add reasoning summary if available (truncated for readability)
    if let Some(reasoning) = &result.reasoning_content {
        if !reasoning.is_empty() {
            let reasoning_preview = if reasoning.len() > 1000 {
                format!("{}...", &reasoning[..1000])
            } else {
                reasoning.clone()
            };
            output.push_str("<details>\n<summary>Reasoning Process</summary>\n\n");
            output.push_str(&reasoning_preview);
            output.push_str("\n\n</details>\n\n");
        }
    }

    // Add main content
    if let Some(content) = result.content {
        output.push_str(&content);
    } else {
        output.push_str("No analysis generated.");
    }

    // Add token usage info
    if let Some(usage) = result.usage {
        output.push_str(&format!(
            "\n\n---\n*Tokens: {} prompt, {} completion*",
            usage.prompt_tokens, usage.completion_tokens
        ));
    }

    output
}

/// Core function to consult an expert with agentic tool access
pub async fn consult_expert<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    // Get the appropriate LLM client for this expert role
    let client: Arc<dyn LlmClient> = ctx.llm_factory()
        .client_for_role(expert.db_key(), ctx.db())
        .map_err(|e| e.to_string())?;

    let provider = client.provider_type();
    tracing::info!(expert = expert.db_key(), provider = %provider, "Expert consultation starting");

    let system_prompt = expert.system_prompt(ctx.db());
    let user_prompt = build_user_prompt(&context, question.as_deref());
    let tools = get_expert_tools();

    let mut messages = vec![
        Message::system(system_prompt),
        Message::user(user_prompt),
    ];

    let mut total_tool_calls = 0;
    let mut iterations = 0;
    // Track previous response ID for stateful providers (like OpenAI)
    // This preserves reasoning context across tool-calling turns
    let mut previous_response_id: Option<String> = None;

    // Agentic loop with overall timeout
    let result = timeout(EXPERT_TIMEOUT, async {
        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return Err(format!(
                    "Expert exceeded maximum iterations ({}). Partial analysis may be available.",
                    MAX_ITERATIONS
                ));
            }

            // For stateful providers (OpenAI Responses API), only send new messages after
            // the first call. The previous_response_id preserves context server-side.
            // For non-stateful providers (DeepSeek, Gemini), always send full history.
            let messages_to_send = if previous_response_id.is_some() && client.supports_stateful() {
                // Only send tool messages (results from current iteration)
                // These are at the end of the messages vec after the last assistant message
                messages
                    .iter()
                    .rev()
                    .take_while(|m| m.role == "tool")
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            } else {
                // First call OR non-stateful provider - send all messages
                messages.clone()
            };

            // Call LLM with tools using stateful API to preserve reasoning context
            let result = timeout(
                LLM_CALL_TIMEOUT,
                client.chat_stateful(
                    messages_to_send,
                    Some(tools.clone()),
                    previous_response_id.as_deref(),
                )
            )
            .await
            .map_err(|_| format!("LLM call timed out after {}s", LLM_CALL_TIMEOUT.as_secs()))?
            .map_err(|e| format!("Expert consultation failed: {}", e))?;

            // Store response ID for next iteration (enables reasoning context preservation)
            previous_response_id = Some(result.request_id.clone());

            // Check if the model wants to call tools
            if let Some(ref tool_calls) = result.tool_calls {
                if !tool_calls.is_empty() {
                    // Add assistant message with tool calls
                    let mut assistant_msg = Message::assistant(
                        result.content.clone(),
                        result.reasoning_content.clone(),
                    );
                    assistant_msg.tool_calls = Some(tool_calls.clone());
                    messages.push(assistant_msg);

                    // Execute tools in parallel for better performance
                    let tool_futures = tool_calls.iter().map(|tc| {
                        let ctx = ctx;  // ctx is already &C, just copy the reference
                        let tc = tc.clone();
                        async move {
                            let result = execute_tool(ctx, &tc).await;
                            (tc.id.clone(), result)
                        }
                    });

                    let tool_results = futures::future::join_all(tool_futures).await;
                    
                    for (id, result) in tool_results {
                        total_tool_calls += 1;
                        messages.push(Message::tool_result(&id, result));
                    }

                    // Continue the loop to get the next response
                    continue;
                }
            }

            // No tool calls - we have the final response
            return Ok((result, total_tool_calls, iterations));
        }
    })
    .await
    .map_err(|_| format!(
        "{} consultation timed out after {}s",
        expert.name(),
        EXPERT_TIMEOUT.as_secs()
    ))??;

    let (final_result, tool_calls, iters) = result;
    Ok(format_expert_response(expert, final_result, tool_calls, iters))
}

// Convenience functions for each expert role

pub async fn consult_architect<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::Architect, context, question).await
}

pub async fn consult_plan_reviewer<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::PlanReviewer, context, question).await
}

pub async fn consult_scope_analyst<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::ScopeAnalyst, context, question).await
}

pub async fn consult_code_reviewer<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::CodeReviewer, context, question).await
}

pub async fn consult_security<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::Security, context, question).await
}

/// Consult multiple experts in parallel
/// Takes a list of role names and runs all consultations concurrently
pub async fn consult_experts<C: ToolContext + Clone + 'static>(
    ctx: &C,
    roles: Vec<String>,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    use futures::stream::{self, StreamExt};

    if roles.is_empty() {
        return Err("No expert roles specified".to_string());
    }

    // Parse and validate all roles first
    let parsed_roles: Result<Vec<ExpertRole>, String> = roles
        .iter()
        .map(|r| {
            ExpertRole::from_db_key(r)
                .ok_or_else(|| format!("Unknown expert role: '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security", r))
        })
        .collect();

    let expert_roles = parsed_roles?;

    // Use Arc for efficient sharing across concurrent tasks (avoids cloning large context)
    let context: Arc<str> = Arc::from(context);
    let question: Option<Arc<str>> = question.map(|q| Arc::from(q));

    // Run consultations with bounded concurrency and overall timeout
    let consultation_future = stream::iter(expert_roles)
        .map(|role| {
            let ctx = ctx.clone();
            let context = Arc::clone(&context);
            let question = question.clone();
            async move {
                let result =
                    consult_expert(&ctx, role, context.to_string(), question.map(|q| q.to_string()))
                        .await;
                (role, result)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_EXPERTS)
        .collect::<Vec<_>>();

    let results = match timeout(PARALLEL_EXPERT_TIMEOUT, consultation_future).await {
        Ok(results) => results,
        Err(_) => {
            return Err(format!(
                "Parallel expert consultation timed out after {} seconds",
                PARALLEL_EXPERT_TIMEOUT.as_secs()
            ));
        }
    };

    // Format combined results
    let mut output = String::new();
    let mut successes = 0;
    let mut failures = 0;

    for (role, result) in results {
        match result {
            Ok(response) => {
                output.push_str(&response);
                output.push_str("\n\n---\n\n");
                successes += 1;
            }
            Err(e) => {
                output.push_str(&format!("## {} (Failed)\n\nError: {}\n\n---\n\n", role.name(), e));
                failures += 1;
            }
        }
    }

    // Add summary
    if failures > 0 {
        output.push_str(&format!(
            "*Consulted {} experts: {} succeeded, {} failed*",
            successes + failures,
            successes,
            failures
        ));
    } else {
        output.push_str(&format!("*Consulted {} experts in parallel*", successes));
    }

    Ok(output)
}

/// Configure expert system prompts and LLM providers (set, get, delete, list, providers)
pub async fn configure_expert<C: ToolContext>(
    ctx: &C,
    action: String,
    role: Option<String>,
    prompt: Option<String>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    use crate::llm::Provider;

    match action.as_str() {
        "set" => {
            let role_key = role.as_deref()
                .ok_or("Role is required for 'set' action")?;

            // Validate role
            if ExpertRole::from_db_key(role_key).is_none() {
                return Err(format!(
                    "Invalid role '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security",
                    role_key
                ));
            }

            // Parse provider if provided
            let parsed_provider = if let Some(ref p) = provider {
                Some(Provider::from_str(p).ok_or_else(|| {
                    format!("Invalid provider '{}'. Valid providers: deepseek, openai, gemini", p)
                })?)
            } else {
                None
            };

            // At least one of prompt, provider, or model should be set
            if prompt.is_none() && parsed_provider.is_none() && model.is_none() {
                return Err("At least one of prompt, provider, or model is required for 'set' action".to_string());
            }

            ctx.db().set_expert_config(
                role_key,
                prompt.as_deref(),
                parsed_provider,
                model.as_deref(),
            ).map_err(|e| e.to_string())?;

            let mut msg = format!("Configuration updated for '{}' expert:", role_key);
            if prompt.is_some() {
                msg.push_str(" prompt set");
            }
            if let Some(ref p) = parsed_provider {
                msg.push_str(&format!(" provider={}", p));
            }
            if let Some(ref m) = model {
                msg.push_str(&format!(" model={}", m));
            }
            Ok(msg)
        }

        "get" => {
            let role_key = role.as_deref()
                .ok_or("Role is required for 'get' action")?;

            // Validate role
            let expert = ExpertRole::from_db_key(role_key)
                .ok_or_else(|| format!(
                    "Invalid role '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security",
                    role_key
                ))?;

            match ctx.db().get_expert_config(role_key) {
                Ok(config) => {
                    let mut output = format!("Configuration for '{}' ({}):\n", role_key, expert.name());
                    output.push_str(&format!("  Provider: {}\n", config.provider));
                    if let Some(ref m) = config.model {
                        output.push_str(&format!("  Model: {}\n", m));
                    } else {
                        output.push_str(&format!("  Model: {} (default)\n", config.provider.default_model()));
                    }
                    if let Some(ref p) = config.prompt {
                        let preview = if p.len() > 200 {
                            format!("{}...", &p[..200])
                        } else {
                            p.clone()
                        };
                        output.push_str(&format!("  Custom prompt: {}\n", preview));
                    } else {
                        output.push_str("  Prompt: (default)\n");
                    }
                    Ok(output)
                }
                Err(e) => Err(e.to_string()),
            }
        }

        "delete" => {
            let role_key = role.as_deref()
                .ok_or("Role is required for 'delete' action")?;

            // Validate role
            if ExpertRole::from_db_key(role_key).is_none() {
                return Err(format!(
                    "Invalid role '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security",
                    role_key
                ));
            }

            match ctx.db().delete_custom_prompt(role_key) {
                Ok(true) => Ok(format!("Configuration deleted for '{}'. Reverted to defaults.", role_key)),
                Ok(false) => Ok(format!("No custom configuration was set for '{}'.", role_key)),
                Err(e) => Err(e.to_string()),
            }
        }

        "list" => {
            match ctx.db().list_custom_prompts() {
                Ok(configs) => {
                    if configs.is_empty() {
                        Ok("No custom configurations. All experts use default settings.".to_string())
                    } else {
                        let mut output = format!("{} expert configurations:\n\n", configs.len());
                        for (role_key, prompt_text, provider_str, model_opt) in configs {
                            let prompt_preview = if prompt_text.len() > 50 {
                                format!("{}...", &prompt_text[..50])
                            } else if prompt_text.is_empty() {
                                "(default)".to_string()
                            } else {
                                prompt_text
                            };
                            let model_str = model_opt.as_deref().unwrap_or("default");
                            output.push_str(&format!(
                                "  {}: provider={}, model={}, prompt={}\n",
                                role_key, provider_str, model_str, prompt_preview
                            ));
                        }
                        Ok(output)
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }

        "providers" => {
            // List available LLM providers
            let factory = ctx.llm_factory();
            let available = factory.available_providers();

            if available.is_empty() {
                Ok("No LLM providers available. Set DEEPSEEK_API_KEY, OPENAI_API_KEY, or GEMINI_API_KEY.".to_string())
            } else {
                let mut output = format!("{} LLM providers available:\n\n", available.len());
                for p in &available {
                    let is_default = factory.default_provider() == Some(*p);
                    let default_marker = if is_default { " (default)" } else { "" };
                    output.push_str(&format!(
                        "  {}: model={}{}\n",
                        p, p.default_model(), default_marker
                    ));
                }
                output.push_str("\nSet DEFAULT_LLM_PROVIDER env var to change the global default.");
                Ok(output)
            }
        }

        _ => Err(format!(
            "Invalid action '{}'. Valid actions: set, get, delete, list, providers",
            action
        )),
    }
}
