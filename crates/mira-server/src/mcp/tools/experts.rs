// crates/mira-server/src/mcp/tools/experts.rs
// Expert consultation tools - delegates to DeepSeek Reasoner

use crate::mcp::MiraServer;
use crate::tools::core::experts;

/// Consult the Architect expert
pub async fn consult_architect(
    server: &MiraServer,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    experts::consult_architect(server, context, question).await
}

/// Consult the Plan Reviewer expert
pub async fn consult_plan_reviewer(
    server: &MiraServer,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    experts::consult_plan_reviewer(server, context, question).await
}

/// Consult the Scope Analyst expert
pub async fn consult_scope_analyst(
    server: &MiraServer,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    experts::consult_scope_analyst(server, context, question).await
}

/// Consult the Code Reviewer expert
pub async fn consult_code_reviewer(
    server: &MiraServer,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    experts::consult_code_reviewer(server, context, question).await
}

/// Consult the Security Analyst expert
pub async fn consult_security(
    server: &MiraServer,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    experts::consult_security(server, context, question).await
}
