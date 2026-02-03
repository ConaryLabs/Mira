// crates/mira-server/src/cli/tool.rs
// Direct tool execution from CLI

use super::serve::setup_server_context;
use anyhow::Result;
use mira::hooks::session::read_claude_session_id;
use mira::mcp::requests::{
    AnalyzeDiffRequest, CodeRequest, DocumentationRequest, ExpertRequest, FindingRequest,
    GoalRequest, IndexRequest, MemoryRequest, ProjectRequest, ReplyToMiraRequest, SessionRequest,
    TasksRequest,
};

/// Execute a tool directly from the command line
pub async fn run_tool(name: String, args: String) -> Result<()> {
    // Setup server context with restored project/session state
    let server = setup_server_context().await?;

    // Execute tool
    let res: Result<String, String> = match name.as_str() {
        "project" => {
            let req: ProjectRequest = serde_json::from_str(&args)?;
            // For start action, use provided session ID or fall back to Claude's hook-generated ID
            let session_id = req.session_id.or_else(read_claude_session_id);
            mira::tools::project(&server, req.action, req.project_path, req.name, session_id)
                .await
                .map(|output| output.0.message)
        }
        "memory" => {
            let req: MemoryRequest = serde_json::from_str(&args)?;
            mira::tools::handle_memory(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "code" => {
            let req: CodeRequest = serde_json::from_str(&args)?;
            mira::tools::handle_code(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "goal" => {
            let req: GoalRequest = serde_json::from_str(&args)?;
            mira::tools::goal(&server, req)
            .await
            .map(|output| output.0.message)
        }
        "index" => {
            let req: IndexRequest = serde_json::from_str(&args)?;
            mira::tools::index(
                &server,
                req.action,
                req.path,
                req.skip_embed.unwrap_or(false),
            )
            .await
            .map(|output| output.0.message)
        }
        "session" => {
            let req: SessionRequest = serde_json::from_str(&args)?;
            mira::tools::handle_session(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "expert" => {
            let req: ExpertRequest = serde_json::from_str(&args)?;
            mira::tools::handle_expert(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "reply_to_mira" => {
            let req: ReplyToMiraRequest = serde_json::from_str(&args)?;
            mira::tools::reply_to_mira(
                &server,
                req.in_reply_to,
                req.content,
                req.complete.unwrap_or(true),
            )
            .await
            .map(|output| output.0.message)
        }
        "documentation" => {
            let req: DocumentationRequest = serde_json::from_str(&args)?;
            mira::tools::documentation(
                &server,
                req.action,
                req.task_id,
                req.reason,
                req.doc_type,
                req.priority,
                req.status,
            )
            .await
            .map(|output| output.0.message)
        }
        "finding" => {
            let req: FindingRequest = serde_json::from_str(&args)?;
            mira::tools::finding(
                &server,
                req.action,
                req.finding_id,
                req.finding_ids,
                req.status,
                req.feedback,
                req.file_path,
                req.expert_role,
                req.correction_type,
                req.limit,
            )
            .await
            .map(|output| output.0.message)
        }
        "analyze_diff" => {
            let req: AnalyzeDiffRequest = serde_json::from_str(&args)?;
            mira::tools::analyze_diff_tool(&server, req.from_ref, req.to_ref, req.include_impact)
                .await
                .map(|output| output.0.message)
        }
        "tasks" => {
            let req: TasksRequest = serde_json::from_str(&args)?;
            mira::tools::tasks::handle_tasks(&server, req)
                .await
                .map(|output| output.0.message)
        }
        _ => Err(format!("Unknown tool: {}", name)),
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
        "memory",
        "code",
        "goal",
        "index",
        "session",
        "expert",
        "reply_to_mira",
        "documentation",
        "finding",
        "analyze_diff",
        "tasks",
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
        let code_pool = Arc::new(DatabasePool::open_code_db_in_memory().await.unwrap());
        let server = MiraServer::new(pool, code_pool, None);

        let mcp_tools: std::collections::HashSet<String> =
            server.list_tool_names().into_iter().collect();

        let cli_tools: std::collections::HashSet<&str> =
            list_cli_tool_names().into_iter().collect();

        // Check for tools in MCP but missing from CLI
        let missing_from_cli: Vec<_> = mcp_tools
            .iter()
            .filter(|t| !cli_tools.contains(t.as_str()))
            .collect();

        // Check for tools in CLI but missing from MCP (shouldn't happen but good to check)
        let missing_from_mcp: Vec<_> = cli_tools
            .iter()
            .filter(|t| !mcp_tools.contains::<str>(t))
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
