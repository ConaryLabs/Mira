// crates/mira-server/src/cli/tool.rs
// Direct tool execution from CLI

use super::serve::setup_server_context;
use anyhow::Result;
use mira::hooks::session::read_claude_session_id;
use mira::mcp::requests::{
    AnalyzeDiffRequest, CheckCapabilityRequest, ConfigureExpertRequest,
    ConsultExpertsRequest, CrossProjectRequest, DocumentationRequest,
    FindCalleesRequest, FindCallersRequest, FindingRequest, ForgetRequest,
    GetSymbolsRequest, GoalRequest, IndexRequest, ProjectRequest, RecallRequest,
    RememberRequest, ReplyToMiraRequest, SemanticCodeSearchRequest,
    SessionHistoryRequest, TeamRequest, UsageRequest,
};

/// Execute a tool directly from the command line
pub async fn run_tool(name: String, args: String) -> Result<()> {
    // Setup server context with restored project/session state
    let server = setup_server_context().await?;

    // Execute tool
    let res = match name.as_str() {
        "project" => {
            let req: ProjectRequest = serde_json::from_str(&args)?;
            // For start action, use provided session ID or fall back to Claude's hook-generated ID
            let session_id = req.session_id.or_else(read_claude_session_id);
            mira::tools::project(&server, req.action, req.project_path, req.name, session_id).await
        }
        "remember" => {
             let req: RememberRequest = serde_json::from_str(&args)?;
             mira::tools::remember(&server, req.content, req.key, req.fact_type, req.category, req.confidence, req.scope).await
        }
        "recall" => {
            let req: RecallRequest = serde_json::from_str(&args)?;
            mira::tools::recall(&server, req.query, req.limit, req.category, req.fact_type).await
        }
        "forget" => {
            let req: ForgetRequest = serde_json::from_str(&args)?;
            mira::tools::forget(&server, req.id).await
        }
        "get_symbols" => {
            let req: GetSymbolsRequest = serde_json::from_str(&args)?;
            mira::tools::get_symbols(req.file_path, req.symbol_type)
        }
        "search_code" => {
            let req: SemanticCodeSearchRequest = serde_json::from_str(&args)?;
            mira::tools::search_code(&server, req.query, req.language, req.limit).await
        }
        "find_callers" => {
            let req: FindCallersRequest = serde_json::from_str(&args)?;
            mira::tools::find_function_callers(&server, req.function_name, req.limit).await
        }
        "find_callees" => {
            let req: FindCalleesRequest = serde_json::from_str(&args)?;
            mira::tools::find_function_callees(&server, req.function_name, req.limit).await
        }
        "check_capability" => {
            let req: CheckCapabilityRequest = serde_json::from_str(&args)?;
            mira::tools::check_capability(&server, req.description).await
        }
        "goal" => {
             let req: GoalRequest = serde_json::from_str(&args)?;
             mira::tools::goal(&server, req.action, req.goal_id, req.title, req.description, req.status, req.priority, req.progress_percent, req.include_finished, req.limit, req.goals, req.milestone_title, req.milestone_id, req.weight).await
        }
        "index" => {
             let req: IndexRequest = serde_json::from_str(&args)?;
             mira::tools::index(&server, req.action, req.path, req.skip_embed.unwrap_or(false)).await
        }
        "summarize_codebase" => {
            mira::tools::summarize_codebase(&server).await
        }
        "get_session_recap" => {
            mira::tools::get_session_recap(&server).await
        }
        "session_history" => {
            let req: SessionHistoryRequest = serde_json::from_str(&args)?;
            mira::tools::session_history(&server, req.action, req.session_id, req.limit).await
        }
        "consult_experts" => {
             let req: ConsultExpertsRequest = serde_json::from_str(&args)?;
             mira::tools::consult_experts(&server, req.roles, req.context, req.question).await
        }
        "configure_expert" => {
             let req: ConfigureExpertRequest = serde_json::from_str(&args)?;
             mira::tools::configure_expert(&server, req.action, req.role, req.prompt, req.provider, req.model).await
        }
        "reply_to_mira" => {
             let req: ReplyToMiraRequest = serde_json::from_str(&args)?;
             mira::tools::reply_to_mira(&server, req.in_reply_to, req.content, req.complete.unwrap_or(true)).await
        }
        "cross_project" => {
            let req: CrossProjectRequest = serde_json::from_str(&args)?;
            mira::tools::cross_project(&server, req.action, req.export, req.import, req.min_confidence, req.epsilon).await
        }
        "export_claude_local" => {
            mira::tools::export_claude_local(&server).await
        }
        "documentation" => {
            let req: DocumentationRequest = serde_json::from_str(&args)?;
            mira::tools::documentation(&server, req.action, req.task_id, req.reason, req.doc_type, req.priority, req.status).await
        }
        "team" => {
            let req: TeamRequest = serde_json::from_str(&args)?;
            mira::tools::team(&server, req.action, req.team_id, req.name, req.description, req.user_identity, req.role).await
        }
        "finding" => {
            let req: FindingRequest = serde_json::from_str(&args)?;
            mira::tools::finding(&server, req.action, req.finding_id, req.finding_ids, req.status, req.feedback, req.file_path, req.expert_role, req.correction_type, req.limit).await
        }
        "analyze_diff" => {
            let req: AnalyzeDiffRequest = serde_json::from_str(&args)?;
            mira::tools::analyze_diff_tool(&server, req.from_ref, req.to_ref, req.include_impact).await
        }
        "usage" => {
            let req: UsageRequest = serde_json::from_str(&args)?;
            mira::tools::usage(&server, req.action, req.group_by, req.since_days, req.limit).await
        }
        _ => Err(format!("Unknown tool: {}", name).into()),
    };

    match res {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

/// Returns the list of tool names supported by the CLI dispatcher.
/// Used for verification against MCP router.
#[cfg(test)]
fn list_cli_tool_names() -> Vec<&'static str> {
    vec![
        "project",
        "remember",
        "recall",
        "forget",
        "get_symbols",
        "search_code",
        "find_callers",
        "find_callees",
        "check_capability",
        "goal",
        "index",
        "summarize_codebase",
        "get_session_recap",
        "session_history",
        "consult_experts",
        "configure_expert",
        "reply_to_mira",
        "cross_project",
        "export_claude_local",
        "documentation",
        "team",
        "finding",
        "analyze_diff",
        "usage",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mira::db::pool::DatabasePool;
    use mira::mcp::MiraServer;
    use std::sync::Arc;

    /// Verifies CLI dispatcher supports all MCP tools.
    /// This test catches drift between the two implementations.
    #[tokio::test]
    async fn cli_tools_match_mcp_tools() {
        // Create a minimal server to get tool list
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let server = MiraServer::new(pool, None);

        let mcp_tools: std::collections::HashSet<String> = server
            .list_tool_names()
            .into_iter()
            .collect();

        let cli_tools: std::collections::HashSet<&str> = list_cli_tool_names()
            .into_iter()
            .collect();

        // Check for tools in MCP but missing from CLI
        let missing_from_cli: Vec<_> = mcp_tools
            .iter()
            .filter(|t| !cli_tools.contains(t.as_str()))
            .collect();

        // Check for tools in CLI but missing from MCP (shouldn't happen but good to check)
        let missing_from_mcp: Vec<_> = cli_tools
            .iter()
            .filter(|t| !mcp_tools.contains(&t.to_string()))
            .collect();

        assert!(
            missing_from_cli.is_empty(),
            "CLI dispatcher is missing MCP tools: {:?}",
            missing_from_cli
        );

        assert!(
            missing_from_mcp.is_empty(),
            "CLI has tools not in MCP (should not happen): {:?}",
            missing_from_mcp
        );
    }
}
