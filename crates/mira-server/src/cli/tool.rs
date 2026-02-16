// crates/mira-server/src/cli/tool.rs
// Direct tool execution from CLI

use super::serve::setup_server_context;
use anyhow::Result;
use mira::hooks::session::read_claude_session_id;
use mira::mcp::requests::{
    CodeAction, CodeRequest, DocumentationRequest, GoalRequest, IndexRequest, MemoryRequest,
    ProjectRequest, RecipeRequest, SessionAction, SessionRequest, TeamRequest,
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
            if matches!(req.action, CodeAction::Diff) {
                mira::tools::analyze_diff_tool(
                    &server,
                    req.from_ref,
                    req.to_ref,
                    req.include_impact,
                )
                .await
                .map(|output| output.0.message)
            } else {
                mira::tools::handle_code(&server, req)
                    .await
                    .map(|output| output.0.message)
            }
        }
        "diff" => {
            #[derive(serde::Deserialize)]
            struct DiffArgs {
                from_ref: Option<String>,
                to_ref: Option<String>,
                include_impact: Option<bool>,
            }
            let req: DiffArgs = serde_json::from_str(&args)?;
            mira::tools::analyze_diff_tool(&server, req.from_ref, req.to_ref, req.include_impact)
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
            match req.action {
                SessionAction::TasksList | SessionAction::TasksGet | SessionAction::TasksCancel => {
                    mira::tools::tasks::handle_tasks(&server, req.action, req.task_id)
                        .await
                        .map(|output| output.0.message)
                }
                _ => mira::tools::handle_session(&server, req)
                    .await
                    .map(|output| output.0.message),
            }
        }
        "documentation" => {
            let req: DocumentationRequest = serde_json::from_str(&args)?;
            mira::tools::documentation(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "team" => {
            let req: TeamRequest = serde_json::from_str(&args)?;
            mira::tools::handle_team(&server, req)
                .await
                .map(|output| output.0.message)
        }
        "recipe" => {
            let req: RecipeRequest = serde_json::from_str(&args)?;
            mira::tools::handle_recipe(req)
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
        "diff",
        "goal",
        "index",
        "session",
        "documentation",
        "team",
        "recipe",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mira::db::pool::DatabasePool;
    use mira::mcp::MiraServer;
    use std::sync::Arc;

    /// Verifies CLI dispatcher is a superset of MCP tools.
    /// MCP exposes a slim surface; CLI supports all tools including MCP-removed ones.
    #[tokio::test]
    async fn cli_tools_superset_of_mcp_tools() {
        // Create a minimal server to get tool list
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let code_pool = Arc::new(DatabasePool::open_code_db_in_memory().await.unwrap());
        let server = MiraServer::new(pool, code_pool, None);

        let mcp_tools: std::collections::HashSet<String> =
            server.list_tool_names().into_iter().collect();

        let cli_tools: std::collections::HashSet<&str> =
            list_cli_tool_names().into_iter().collect();

        // Every MCP tool must have a CLI counterpart
        let missing_from_cli: Vec<_> = mcp_tools
            .iter()
            .filter(|t| !cli_tools.contains(t.as_str()))
            .collect();

        assert!(
            missing_from_cli.is_empty(),
            "CLI dispatcher is missing MCP tools: {:?}",
            missing_from_cli
        );

        // CLI may have extra tools not in MCP (e.g. documentation, team, recipe)
        // — that's expected after tool surface consolidation
    }
}
