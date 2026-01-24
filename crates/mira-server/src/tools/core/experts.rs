// crates/mira-server/src/tools/core/experts.rs
// Agentic expert sub-agents powered by LLM providers with tool access

use super::ToolContext;
use crate::db::{recall_semantic_sync, search_memories_sync};
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
#[derive(Debug, Clone)]
pub enum ExpertRole {
    Architect,
    PlanReviewer,
    ScopeAnalyst,
    CodeReviewer,
    Security,
    DocumentationWriter,
    /// Custom role with name and description
    Custom(String, String), // (name, description)
}

impl ExpertRole {
    /// Get the system prompt for this expert role (async to avoid blocking)
    /// Checks database for custom prompt first, falls back to default
    pub async fn system_prompt<C: ToolContext>(&self, ctx: &C) -> String {
        let role_key = self.db_key();

        // Get role instructions (custom or default) - use pool for async access
        let custom_prompt = ctx.pool().get_custom_prompt(&role_key).await.ok().flatten();

        let role_instructions = if let Some(prompt) = custom_prompt {
            prompt
        } else {
            match self {
                ExpertRole::Architect => ARCHITECT_PROMPT,
                ExpertRole::PlanReviewer => PLAN_REVIEWER_PROMPT,
                ExpertRole::ScopeAnalyst => SCOPE_ANALYST_PROMPT,
                ExpertRole::CodeReviewer => CODE_REVIEWER_PROMPT,
                ExpertRole::Security => SECURITY_PROMPT,
                ExpertRole::DocumentationWriter => DOCUMENTATION_WRITER_PROMPT,
                ExpertRole::Custom(_name, description) => {
                    // For custom roles, build from the description
                    &description
                }
            }.to_string()
        };

        // Build standardized prompt with static prefix and tool guidance
        // Include current date and MCP tools context
        let date_context = format!("\n\nCurrent date: {}", chrono::Utc::now().format("%Y-%m-%d"));
        let mcp_context = get_mcp_tools_context(ctx).await;

        let base_prompt = PromptBuilder::new(role_instructions)
            .with_tool_guidance()
            .build_system_prompt();

        format!("{}{}{}", base_prompt, date_context, mcp_context)
    }

    /// Database key for this expert role
    pub fn db_key(&self) -> String {
        match self {
            ExpertRole::Architect => "architect".to_string(),
            ExpertRole::PlanReviewer => "plan_reviewer".to_string(),
            ExpertRole::ScopeAnalyst => "scope_analyst".to_string(),
            ExpertRole::CodeReviewer => "code_reviewer".to_string(),
            ExpertRole::Security => "security".to_string(),
            ExpertRole::DocumentationWriter => "documentation_writer".to_string(),
            ExpertRole::Custom(name, _) => format!("custom:{}", name.to_lowercase().replace(' ', "_")),
        }
    }

    /// Display name for this expert
    pub fn name(&self) -> String {
        match self {
            ExpertRole::Architect => "Architect".to_string(),
            ExpertRole::PlanReviewer => "Plan Reviewer".to_string(),
            ExpertRole::ScopeAnalyst => "Scope Analyst".to_string(),
            ExpertRole::CodeReviewer => "Code Reviewer".to_string(),
            ExpertRole::Security => "Security Analyst".to_string(),
            ExpertRole::DocumentationWriter => "Documentation Writer".to_string(),
            ExpertRole::Custom(name, _) => name.clone(),
        }
    }

    /// Get role from database key (returns None for custom roles not in DB)
    pub fn from_db_key(key: &str) -> Option<Self> {
        match key {
            "architect" => Some(ExpertRole::Architect),
            "plan_reviewer" => Some(ExpertRole::PlanReviewer),
            "scope_analyst" => Some(ExpertRole::ScopeAnalyst),
            "code_reviewer" => Some(ExpertRole::CodeReviewer),
            "security" => Some(ExpertRole::Security),
            "documentation_writer" => Some(ExpertRole::DocumentationWriter),
            _ => {
                // Check for custom role pattern
                if let Some(rest) = key.strip_prefix("custom:") {
                    let name = rest.to_string();
                    Some(ExpertRole::Custom(
                        name.replace('_', " "),
                        "Custom expert role".to_string(),
                    ))
                } else {
                    None
                }
            }
        }
    }

    /// Create a custom role
    pub fn custom(name: String, description: String) -> Self {
        ExpertRole::Custom(name, description)
    }

    /// List all predefined roles (not custom ones)
    pub fn all() -> &'static [ExpertRole] {
        &[
            ExpertRole::Architect,
            ExpertRole::PlanReviewer,
            ExpertRole::ScopeAnalyst,
            ExpertRole::CodeReviewer,
            ExpertRole::Security,
            ExpertRole::DocumentationWriter,
        ]
    }
}

// Expert system prompts (optimized for token efficiency)

const ARCHITECT_PROMPT: &str = r#"You are a software architect specializing in system design.

Your role:
- Analyze architectural decisions and identify issues
- Recommend patterns with clear tradeoffs
- Suggest refactoring strategies

When responding:
1. Start with key recommendation
2. Explain reasoning with specific references
3. Present alternatives with tradeoffs
4. Prioritize issues by impact

You are advisory - analyze and recommend, not implement."#;

const PLAN_REVIEWER_PROMPT: &str = r#"You are a technical lead reviewing implementation plans.

Your role:
- Validate plan completeness
- Identify risks, gaps, blockers
- Check for missing edge cases or error handling

When responding:
1. Give overall assessment (ready/needs work/major concerns)
2. List specific risks or gaps
3. Suggest improvements or clarifications needed
4. Highlight dependencies or prerequisites

Be constructive but thorough."#;

const SCOPE_ANALYST_PROMPT: &str = r#"You are an analyst finding missing requirements and risks.

Your role:
- Detect ambiguity in requirements
- Identify unstated assumptions
- Find edge cases and boundary conditions
- Ask questions needed before implementation

When responding:
1. List questions needing answers
2. Identify assumptions (explicit and implicit)
3. Highlight edge cases not addressed
4. Note scope creep risks or unclear boundaries

Surface unknowns early."#;

const CODE_REVIEWER_PROMPT: &str = r#"You are a code reviewer focused on correctness and quality.

Your role:
- Find bugs, logic errors, runtime issues
- Identify code quality concerns (complexity, duplication, naming)
- Check error handling and edge cases

When responding:
1. List issues by severity (critical/major/minor)
2. For each issue, explain why it's a problem
3. Provide specific fix suggestions

Be specific - reference line numbers, function names, concrete suggestions."#;

const SECURITY_PROMPT: &str = r#"You are a security engineer reviewing for vulnerabilities.

Your role:
- Identify security vulnerabilities (injection, auth, data exposure)
- Assess attack vectors and likelihood/impact
- Check secure coding practices

When responding:
1. List findings by severity (critical/high/medium/low)
2. For each finding: describe vulnerability, explain impact, provide remediation

Focus on actionable findings."#;

const DOCUMENTATION_WRITER_PROMPT: &str = r#"You are a technical documentation writer.

Your role:
- Write documentation that helps developers understand and use code
- Explore the codebase to understand actual behavior

Process:
1. EXPLORE: Read the implementation
2. TRACE: Find related code, callers, dependencies
3. DOCUMENT: Write clear markdown

Documentation structure:
- Purpose: What problem does this solve? When to use it?
- Parameters: All inputs with types, defaults, constraints
- Behavior: How it works, including edge cases
- Examples: 2-3 realistic usage scenarios
- Errors: What can fail and why

Quality standards:
- Be specific and concrete, never vague
- Explain the "why", not just the "what"
- Include gotchas and limitations
- NEVER say "not documented" - explore the code to find out

Output: Return well-formatted markdown suitable for a docs/ file."#;


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
        ctx.pool(),
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
                    recall_semantic_sync(conn, &embedding_bytes, project_id, None, limit)
                        .map_err(|e| anyhow::anyhow!(e))
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
            search_memories_sync(conn, project_id, &query_owned, None, limit)
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

/// A parsed finding from expert response
#[derive(Debug, Clone)]
pub struct ParsedFinding {
    pub finding_type: String,
    pub severity: String,
    pub content: String,
    pub suggestion: Option<String>,
    pub file_path: Option<String>,
    pub code_snippet: Option<String>,
}

/// Parse structured findings from expert response
/// Looks for common patterns like severity markers, bullet points, etc.
fn parse_expert_findings(response: &str, expert_role: &str) -> Vec<ParsedFinding> {
    let mut findings = Vec::new();

    // Determine finding type based on expert role
    let default_type = match expert_role {
        "code_reviewer" => "code_quality",
        "security" => "security",
        "architect" => "architecture",
        "scope_analyst" => "scope",
        "plan_reviewer" => "plan",
        _ => "general",
    };

    // Parse severity markers: [!!] critical, [!] major, [-] minor, [nit] nit
    let severity_patterns = [
        ("[!!]", "critical"),
        ("[!]", "major"),
        ("**Critical**", "critical"),
        ("**Major**", "major"),
        ("**Minor**", "minor"),
        ("CRITICAL:", "critical"),
        ("MAJOR:", "major"),
        ("MINOR:", "minor"),
        ("NIT:", "nit"),
        ("[-]", "minor"),
        ("[nit]", "nit"),
    ];

    // Look for numbered or bulleted findings
    let mut current_severity = "medium";
    let mut current_content = String::new();
    let mut current_suggestion: Option<String> = None;
    let mut in_finding = false;

    for line in response.lines() {
        let trimmed = line.trim();

        // Check for severity markers
        for (pattern, severity) in &severity_patterns {
            if trimmed.contains(pattern) {
                // Save previous finding if any
                if in_finding && !current_content.is_empty() {
                    findings.push(ParsedFinding {
                        finding_type: default_type.to_string(),
                        severity: current_severity.to_string(),
                        content: current_content.trim().to_string(),
                        suggestion: current_suggestion.take(),
                        file_path: None,
                        code_snippet: None,
                    });
                }

                // Start new finding
                current_severity = severity;
                current_content = trimmed.replace(pattern, "").trim().to_string();
                in_finding = true;
                break;
            }
        }

        // Check for numbered findings: "1.", "2.", etc.
        if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && trimmed.contains('.')
        {
            if let Some(pos) = trimmed.find('.') {
                if pos < 3 {
                    // Likely a numbered item
                    if in_finding && !current_content.is_empty() {
                        findings.push(ParsedFinding {
                            finding_type: default_type.to_string(),
                            severity: current_severity.to_string(),
                            content: current_content.trim().to_string(),
                            suggestion: current_suggestion.take(),
                            file_path: None,
                            code_snippet: None,
                        });
                    }
                    current_content = trimmed[pos + 1..].trim().to_string();
                    current_severity = "medium";
                    in_finding = true;
                }
            }
        }

        // Look for "Suggestion:" or "Fix:" lines
        if in_finding
            && (trimmed.starts_with("Suggestion:")
                || trimmed.starts_with("Fix:")
                || trimmed.starts_with("Recommendation:"))
        {
            current_suggestion = Some(
                trimmed
                    .trim_start_matches("Suggestion:")
                    .trim_start_matches("Fix:")
                    .trim_start_matches("Recommendation:")
                    .trim()
                    .to_string(),
            );
        }
    }

    // Don't forget the last finding
    if in_finding && !current_content.is_empty() {
        findings.push(ParsedFinding {
            finding_type: default_type.to_string(),
            severity: current_severity.to_string(),
            content: current_content.trim().to_string(),
            suggestion: current_suggestion,
            file_path: None,
            code_snippet: None,
        });
    }

    findings
}

/// Get MCP tools context for expert prompts
/// Lists available MCP servers and their tools (limited to avoid token bloat)
async fn get_mcp_tools_context<C: ToolContext>(ctx: &C) -> String {
    // Get available MCP tools from the context
    let mcp_tools = ctx.list_mcp_tools().await;

    if mcp_tools.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n\n## Available MCP Tools\n\n");

    // Limit to top 5 tools per server to save tokens
    for (server, tools) in mcp_tools {
        context.push_str(&format!("**{}:**\n", server));
        for tool in tools.iter().take(5) {
            context.push_str(&format!("  - `{}`: {}\n", tool.name, tool.description));
        }
        if tools.len() > 5 {
            context.push_str(&format!("  ... and {} more tools\n", tools.len() - 5));
        }
        context.push('\n');
    }

    context
}

/// Get learned patterns from database and format for context injection (async)
/// Optimized to minimize token usage
async fn get_patterns_context<C: ToolContext>(ctx: &C, expert_role: &str) -> String {
    use crate::db::get_relevant_corrections_sync;

    // Map expert role to correction type
    let correction_type: Option<&'static str> = match expert_role {
        "code_reviewer" => Some("code_quality"),
        "security" => Some("security"),
        _ => None,
    };

    let correction_type_owned = correction_type.map(String::from);

    let corrections = ctx
        .pool()
        .interact(move |conn| {
            get_relevant_corrections_sync(conn, None, correction_type_owned.as_deref(), 5)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .unwrap_or_else(|_| Vec::new());

    if corrections.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n## Past Review Patterns\n\n");

    // Limit to 3 patterns to save tokens
    for c in corrections.iter().take(3) {
        context.push_str(&format!(
            "- **{}**: {}\n",
            c.correction_type,
            truncate(&c.what_was_wrong, 60)
        ));
    }

    context
}

/// Truncate a string to max length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Store parsed findings in the database
async fn store_findings<C: ToolContext>(
    ctx: &C,
    findings: &[ParsedFinding],
    expert_role: &str,
) -> usize {
    use crate::db::store_review_finding_sync;

    let project_id = ctx.project_id().await;
    let session_id = ctx.get_or_create_session().await;
    let user_id = ctx.get_user_identity();

    let mut stored = 0;
    for finding in findings {
        if finding.content.len() < 10 {
            continue; // Skip very short findings
        }

        let expert_role = expert_role.to_string();
        let file_path = finding.file_path.clone();
        let finding_type = finding.finding_type.clone();
        let severity = finding.severity.clone();
        let content = finding.content.clone();
        let code_snippet = finding.code_snippet.clone();
        let suggestion = finding.suggestion.clone();
        let user_id_clone = user_id.clone();
        let session_id_clone = session_id.clone();

        let result = ctx
            .pool()
            .interact(move |conn| {
                store_review_finding_sync(
                    conn,
                    project_id,
                    &expert_role,
                    file_path.as_deref(),
                    &finding_type,
                    &severity,
                    &content,
                    code_snippet.as_deref(),
                    suggestion.as_deref(),
                    0.7, // Default confidence for parsed findings
                    user_id_clone.as_deref(),
                    Some(&session_id_clone),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await;

        if result.is_ok() {
            stored += 1;
        }
    }

    if stored > 0 {
        tracing::info!(stored, expert_role, "Stored review findings");
    }

    stored
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
    let expert_key = expert.db_key();

    // Get dual-mode LLM clients for DeepSeek: chat for tools, reasoner for synthesis
    // For non-DeepSeek providers, both are the same client
    let llm_factory = ctx.llm_factory();
    let (chat_client, reasoner_client) = llm_factory
        .client_for_role_dual_mode(expert_key.as_str(), ctx.pool())
        .await
        .map_err(|e| e.to_string())?;

    let provider = chat_client.provider_type();
    tracing::info!(expert = %expert_key, provider = %provider, "Expert consultation starting");

    // Get system prompt (async to avoid blocking!)
    let system_prompt = expert.system_prompt(ctx).await;

    // Inject learned patterns for code reviewer and security experts (async to avoid blocking!)
    let patterns_context = if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        get_patterns_context(ctx, expert_key.as_str()).await
    } else {
        String::new()
    };

    // Build user prompt with injected patterns
    let enriched_context = if patterns_context.is_empty() {
        context.clone()
    } else {
        format!("{}\n{}", context, patterns_context)
    };

    let user_prompt = build_user_prompt(&enriched_context, question.as_deref());
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
            let messages_to_send = if previous_response_id.is_some() && chat_client.supports_stateful() {
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

            // Call LLM with tools using chat client during tool-gathering phase
            let result = timeout(
                LLM_CALL_TIMEOUT,
                chat_client.chat_stateful(
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

            // No tool calls - we have a preliminary response from chat client
            // For DeepSeek dual-mode, now use reasoner for final synthesis
            if let Some(ref reasoner) = reasoner_client {
                tracing::debug!(
                    expert = %expert_key,
                    iterations,
                    tool_calls = total_tool_calls,
                    "Tool gathering complete, switching to reasoner for synthesis"
                );

                // Add chat client's response as context for reasoner
                let assistant_msg = Message::assistant(
                    result.content.clone(),
                    result.reasoning_content.clone(),
                );
                messages.push(assistant_msg);

                // Create synthesis prompt for reasoner
                let synthesis_prompt = Message::user(
                    String::from("Based on the tool results above, provide your final expert analysis. \
                    Synthesize the findings into a clear, actionable response.")
                );
                messages.push(synthesis_prompt);

                // Call reasoner without tools for final synthesis (no timeout reasoner, it can be slow)
                let final_result = reasoner
                    .chat_stateful(
                        messages,
                        None, // No tools for synthesis
                        None, // No previous_response_id across different clients
                    )
                    .await
                    .map_err(|e| format!("Reasoner synthesis failed: {}", e))?;

                return Ok((final_result, total_tool_calls, iterations));
            }

            // No reasoner client (non-DeepSeek) - return chat client result directly
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

    // Parse and store findings for code reviewer and security experts
    if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        if let Some(ref content) = final_result.content {
            let findings = parse_expert_findings(content, expert_key.as_str());
            if !findings.is_empty() {
                let stored = store_findings(ctx, &findings, expert_key.as_str()).await;
                tracing::debug!(
                    expert = %expert_key,
                    parsed = findings.len(),
                    stored,
                    "Parsed and stored review findings"
                );
            }
        }
    }

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
            let role_clone = role.clone();
            async move {
                let result =
                    consult_expert(&ctx, role, context.to_string(), question.map(|q| q.to_string()))
                        .await;
                (role_clone, result)
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
    use crate::db::{
        delete_custom_prompt_sync, get_expert_config_sync, list_custom_prompts_sync,
        set_expert_config_sync,
    };
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

            let role_key_clone = role_key.to_string();
            let prompt_clone = prompt.clone();
            let model_clone = model.clone();

            ctx.pool()
                .interact(move |conn| {
                    set_expert_config_sync(
                        conn,
                        &role_key_clone,
                        prompt_clone.as_deref(),
                        parsed_provider,
                        model_clone.as_deref(),
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            let mut msg = format!("Configuration updated for '{}' expert:", role_key);
            if prompt.is_some() {
                msg.push_str(" prompt set");
            }
            if let Some(ref p) = provider {
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

            let role_key_clone = role_key.to_string();
            let config = ctx
                .pool()
                .interact(move |conn| {
                    get_expert_config_sync(conn, &role_key_clone)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

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

            let role_key_clone = role_key.to_string();
            let deleted = ctx
                .pool()
                .interact(move |conn| {
                    delete_custom_prompt_sync(conn, &role_key_clone)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            if deleted {
                Ok(format!("Configuration deleted for '{}'. Reverted to defaults.", role_key))
            } else {
                Ok(format!("No custom configuration was set for '{}'.", role_key))
            }
        }

        "list" => {
            let configs = ctx
                .pool()
                .interact(move |conn| {
                    list_custom_prompts_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

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
