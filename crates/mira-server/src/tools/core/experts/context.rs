// crates/mira-server/src/tools/core/experts/context.rs
// Context helpers for expert prompts

use super::ToolContext;
use crate::utils::truncate;

/// Get MCP tools context for expert prompts
/// Lists available MCP servers and their tools (limited to avoid token bloat)
pub async fn get_mcp_tools_context<C: ToolContext>(ctx: &C) -> String {
    // Get available MCP tools from the context
    let mcp_tools = ctx.list_mcp_tools().await;

    if mcp_tools.is_empty() {
        return String::new();
    }

    let mut context = String::from(
        "\n\n## Available MCP Tools\n\nThese tools connect to external services. Call them with the prefixed name `mcp__{server}__{tool}` and appropriate arguments.\n\n",
    );

    // Limit to top 5 tools per server to save tokens
    for (server, tools) in mcp_tools {
        context.push_str(&format!("**{}:**\n", server));
        for tool in tools.iter().take(5) {
            context.push_str(&format!(
                "  - `mcp__{}__{}`: {}\n",
                server, tool.name, tool.description
            ));
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
pub async fn get_patterns_context<C: ToolContext>(ctx: &C, expert_role: &str) -> String {
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
        .run(move |conn| {
            get_relevant_corrections_sync(conn, None, correction_type_owned.as_deref(), 5)
        })
        .await
        .unwrap_or_default();

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

/// Build the user prompt from context and optional question
pub fn build_user_prompt(context: &str, question: Option<&str>) -> String {
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
pub fn format_expert_response(
    expert: super::ExpertRole,
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
    if let Some(reasoning) = &result.reasoning_content
        && !reasoning.is_empty()
    {
        output.push_str("<details>\n<summary>Reasoning Process</summary>\n\n");
        output.push_str(&truncate(reasoning, 1000));
        output.push_str("\n\n</details>\n\n");
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
