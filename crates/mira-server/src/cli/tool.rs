// crates/mira-server/src/cli/tool.rs
// Direct tool execution from CLI

use super::serve::setup_server_context;
use anyhow::Result;
use mira::hooks::session::read_claude_session_id;
use mira::mcp::{
    SessionStartRequest, SetProjectRequest, RememberRequest, RecallRequest,
    ForgetRequest, GetSymbolsRequest, SemanticCodeSearchRequest,
    FindCallersRequest, FindCalleesRequest, CheckCapabilityRequest,
    GoalRequest, IndexRequest, SessionHistoryRequest,
    ConsultArchitectRequest, ConsultCodeReviewerRequest,
    ConsultPlanReviewerRequest, ConsultScopeAnalystRequest,
    ConsultSecurityRequest, ConsultExpertsRequest, ConfigureExpertRequest,
    ReplyToMiraRequest
};

/// Execute a tool directly from the command line
pub async fn run_tool(name: String, args: String) -> Result<()> {
    // Setup server context with restored project/session state
    let server = setup_server_context().await?;

    // Execute tool
    let res = match name.as_str() {
        "session_start" => {
            let req: SessionStartRequest = serde_json::from_str(&args)?;
            // Use provided session ID, or fall back to Claude's hook-generated ID
            let session_id = req.session_id.or_else(read_claude_session_id);
            mira::tools::session_start(&server, req.project_path, req.name, session_id).await
        }
        "set_project" => {
            let req: SetProjectRequest = serde_json::from_str(&args)?;
            mira::tools::set_project(&server, req.project_path, req.name).await
        }
        "get_project" => {
             mira::tools::get_project(&server).await
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
        "consult_architect" => {
            let req: ConsultArchitectRequest = serde_json::from_str(&args)?;
            mira::tools::consult_architect(&server, req.context, req.question).await
        }
        "consult_code_reviewer" => {
             let req: ConsultCodeReviewerRequest = serde_json::from_str(&args)?;
             mira::tools::consult_code_reviewer(&server, req.context, req.question).await
        }
        "consult_plan_reviewer" => {
             let req: ConsultPlanReviewerRequest = serde_json::from_str(&args)?;
             mira::tools::consult_plan_reviewer(&server, req.context, req.question).await
        }
        "consult_scope_analyst" => {
             let req: ConsultScopeAnalystRequest = serde_json::from_str(&args)?;
             mira::tools::consult_scope_analyst(&server, req.context, req.question).await
        }
        "consult_security" => {
             let req: ConsultSecurityRequest = serde_json::from_str(&args)?;
             mira::tools::consult_security(&server, req.context, req.question).await
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
             // Just print locally since we don't have a collaborative frontend connected
             Ok(format!("(Reply not sent - no frontend connected) Content: {}", req.content))
        }
        _ => Err(format!("Unknown tool: {}", name).into()),
    };

    match res {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}
