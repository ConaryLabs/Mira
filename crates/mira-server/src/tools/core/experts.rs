// crates/mira-server/src/tools/core/experts.rs
// Agentic expert sub-agents powered by DeepSeek Reasoner with tool access

use super::ToolContext;
use crate::db::Database;
use crate::indexer;
use crate::llm::{Message, Tool, ToolCall};
use crate::search::{embedding_to_bytes, find_callers, find_callees, hybrid_search};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

/// Maximum iterations for the agentic loop
const MAX_ITERATIONS: usize = 100;

/// Timeout for the entire expert consultation (including all tool calls)
const EXPERT_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes for multi-turn

/// Timeout for individual LLM calls
const LLM_CALL_TIMEOUT: Duration = Duration::from_secs(120);

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
    pub fn system_prompt(&self) -> String {
        let base = match self {
            ExpertRole::Architect => ARCHITECT_PROMPT,
            ExpertRole::PlanReviewer => PLAN_REVIEWER_PROMPT,
            ExpertRole::ScopeAnalyst => SCOPE_ANALYST_PROMPT,
            ExpertRole::CodeReviewer => CODE_REVIEWER_PROMPT,
            ExpertRole::Security => SECURITY_PROMPT,
        };
        format!("{}\n\n{}", base, TOOL_USAGE_PROMPT)
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
}

// Expert system prompts

const ARCHITECT_PROMPT: &str = r#"You are a senior software architect with deep expertise in system design, design patterns, and technical decision-making.

Your role is to:
- Analyze system designs and architectural decisions
- Identify potential scalability, maintainability, and performance issues
- Recommend patterns and approaches with clear tradeoffs
- Help debug complex architectural problems
- Suggest refactoring strategies when appropriate

When responding:
1. Start with your key recommendation or finding
2. Explain the reasoning behind your analysis
3. Present alternatives with tradeoffs when relevant
4. Be specific - reference concrete patterns, technologies, or approaches
5. If you see potential issues, prioritize them by impact

You are advisory only - your role is to analyze and recommend, not to implement."#;

const PLAN_REVIEWER_PROMPT: &str = r#"You are a meticulous technical lead who reviews implementation plans before coding begins.

Your role is to:
- Validate that plans are complete and well-thought-out
- Identify risks, gaps, and potential blockers
- Check for missing edge cases or error handling
- Assess whether the approach fits the codebase and constraints
- Provide a go/no-go assessment with specific concerns

When responding:
1. Give an overall assessment (ready to implement / needs work / major concerns)
2. List specific risks or gaps found
3. Suggest concrete improvements or clarifications needed
4. Highlight any dependencies or prerequisites that should be addressed first
5. Note what's done well to reinforce good planning

Be constructive but thorough - catching issues now saves significant rework later."#;

const SCOPE_ANALYST_PROMPT: &str = r#"You are an experienced analyst who specializes in finding what's missing, unclear, or risky in requirements and plans.

Your role is to:
- Detect ambiguity in requirements or specifications
- Identify unstated assumptions that could cause problems
- Find edge cases and boundary conditions
- Ask the questions that should be answered before implementation
- Highlight areas where "it depends" needs to be resolved

When responding:
1. List questions that need answers before proceeding
2. Identify assumptions being made (explicit and implicit)
3. Highlight edge cases or scenarios not addressed
4. Note any scope creep risks or unclear boundaries
5. Suggest what additional information would help

Your goal is to surface unknowns early - better to ask now than discover during implementation."#;

const CODE_REVIEWER_PROMPT: &str = r#"You are a thorough code reviewer focused on correctness, quality, and maintainability.

Your role is to:
- Find bugs, logic errors, and potential runtime issues
- Identify code quality concerns (complexity, duplication, naming)
- Check for proper error handling and edge cases
- Assess test coverage needs
- Suggest specific improvements

When responding:
1. List issues by severity (critical / major / minor / nit)
2. For each issue, explain WHY it's a problem
3. Provide specific suggestions for fixes
4. Highlight any patterns (good or bad) you notice
5. Note areas that need additional testing

Be specific - line numbers, function names, and concrete suggestions are more helpful than general advice."#;

const SECURITY_PROMPT: &str = r#"You are a security engineer who reviews code and designs for vulnerabilities.

Your role is to:
- Identify security vulnerabilities (injection, auth issues, data exposure, etc.)
- Assess attack vectors and their likelihood/impact
- Check for secure coding practices
- Review authentication, authorization, and data handling
- Recommend hardening measures

When responding:
1. List findings by severity (critical / high / medium / low)
2. For each finding:
   - Describe the vulnerability
   - Explain the potential impact
   - Provide remediation steps
3. Note any security best practices being followed
4. Suggest additional security measures if appropriate

Focus on actionable findings - theoretical risks should be clearly marked as such."#;

const TOOL_USAGE_PROMPT: &str = r#"You have access to tools to explore the codebase. Use them to gather context before providing your analysis.

IMPORTANT: Do not ask the user for more context. Instead, use the tools to find what you need:
- Use search_code to find relevant code by meaning (e.g., "authentication logic", "error handling")
- Use get_symbols to see the structure of a file (functions, structs, etc.)
- Use read_file to read specific file contents when you need details
- Use find_callers to see what calls a function
- Use find_callees to see what a function calls
- Use recall to retrieve past decisions and context

Explore proactively. If you're analyzing architecture, search for the main components. If reviewing security, look for authentication/authorization code. If reviewing a plan, verify the files and patterns mentioned exist.

When you have enough context, provide your analysis. Do not make assumptions about code you haven't seen - use the tools to verify."#;

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
                    output.push_str(&format!("### {}\n```\n{}\n```\n\n", r.file_path, r.content));
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
                for s in symbols.iter().take(20) {
                    let lines = if s.start_line == s.end_line {
                        format!("line {}", s.start_line)
                    } else {
                        format!("lines {}-{}", s.start_line, s.end_line)
                    };
                    output.push_str(&format!("  {} ({}) {}\n", s.name, s.symbol_type, lines));
                }
                if symbols.len() > 20 {
                    output.push_str(&format!("  ... and {} more\n", symbols.len() - 20));
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
            let end = end_line.unwrap_or(lines.len()).min(lines.len());

            if start >= lines.len() {
                return format!("Start line {} exceeds file length ({})", start + 1, lines.len());
            }

            let selected: Vec<String> = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{:4} | {}", start + i + 1, line))
                .collect();

            format!("{}:\n{}", file_path, selected.join("\n"))
        }
        Err(e) => format!("Failed to read {}: {}", file_path, e),
    }
}

async fn execute_find_callers<C: ToolContext>(ctx: &C, function_name: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;

    let callers = find_callers(ctx.db(), project_id, function_name, limit);
    if callers.is_empty() {
        format!("No callers found for `{}`", function_name)
    } else {
        let mut output = format!("Functions that call `{}`:\n", function_name);
        for caller in callers {
            output.push_str(&format!("  {} in {} ({}x)\n", caller.symbol_name, caller.file_path, caller.call_count));
        }
        output
    }
}

async fn execute_find_callees<C: ToolContext>(ctx: &C, function_name: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;

    let callees = find_callees(ctx.db(), project_id, function_name, limit);
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

            // Run vector search on blocking thread pool
            let db_clone = ctx.db().clone();
            let results: Result<Vec<(i64, String, f32)>, String> =
                Database::run_blocking(db_clone, move |conn| {
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
                        .map_err(|e| e.to_string())?;

                    let results: Vec<(i64, String, f32)> = stmt
                        .query_map(params![embedding_bytes, project_id, limit as i64], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .map_err(|e| e.to_string())?
                        .filter_map(|r| r.ok())
                        .collect();

                    Ok(results)
                })
                .await;

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

    // Fallback to keyword search
    match ctx.db().search_memories(project_id, query, limit) {
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
    let deepseek = ctx.deepseek()
        .ok_or("DeepSeek not configured")?;

    let system_prompt = expert.system_prompt();
    let user_prompt = build_user_prompt(&context, question.as_deref());
    let tools = get_expert_tools();

    let mut messages = vec![
        Message::system(system_prompt),
        Message::user(user_prompt),
    ];

    let mut total_tool_calls = 0;
    let mut iterations = 0;

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

            // Call DeepSeek with tools
            let result = timeout(
                LLM_CALL_TIMEOUT,
                deepseek.chat(messages.clone(), Some(tools.clone()))
            )
            .await
            .map_err(|_| format!("LLM call timed out after {}s", LLM_CALL_TIMEOUT.as_secs()))?
            .map_err(|e| format!("Expert consultation failed: {}", e))?;

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

                    // Execute each tool and add results
                    for tc in tool_calls {
                        total_tool_calls += 1;
                        let tool_result = execute_tool(ctx, tc).await;
                        messages.push(Message::tool_result(&tc.id, tool_result));
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
