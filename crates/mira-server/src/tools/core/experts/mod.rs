// crates/mira-server/src/tools/core/experts/mod.rs
// Agentic expert sub-agents powered by LLM providers with tool access

use std::time::Duration;

use crate::db::{ExpertConfig, get_expert_config_sync, list_custom_prompts_sync};
use crate::mcp::requests::ExpertConfigAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    ConfigureData, ConsultData, ExpertConfigEntry, ExpertData, ExpertOpinion, ExpertOutput,
};

mod config;
mod context;
mod council;
mod execution;
pub(crate) mod findings;
mod plan;
mod prompts;
mod role;
pub(crate) mod strategy;
mod tools;

// Re-export ToolContext from parent for use by submodules
pub(crate) use super::ToolContext;

// Constants used across the module
/// Maximum iterations for the agentic loop
pub(crate) const MAX_ITERATIONS: usize = 100;

/// Timeout for the entire expert consultation (including all tool calls)
pub(crate) const EXPERT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes for multi-turn with reasoning models

/// Timeout for individual LLM calls (6 minutes for reasoning models like DeepSeek)
pub(crate) const LLM_CALL_TIMEOUT: Duration = Duration::from_secs(360);

/// Maximum concurrent expert consultations (prevents rate limit exhaustion)
pub(crate) const MAX_CONCURRENT_EXPERTS: usize = 3;

/// Timeout for parallel expert consultation (longer than single expert to allow queuing)
pub(crate) const PARALLEL_EXPERT_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes for reasoning models

// Public API exports
pub use config::configure_expert;
pub use execution::{consult_expert, consult_experts};
pub use findings::ParsedFinding;
pub use role::ExpertRole;

/// Unified expert tool dispatcher
pub async fn handle_expert<C: ToolContext + Clone + 'static>(
    ctx: &C,
    req: crate::mcp::requests::ExpertRequest,
) -> Result<Json<ExpertOutput>, String> {
    use crate::mcp::requests::ExpertAction;
    match req.action {
        ExpertAction::Consult => {
            let roles = req.roles.ok_or("roles is required for action 'consult'")?;
            let context = req
                .context
                .ok_or("context is required for action 'consult'")?;
            let message = consult_experts(ctx, roles, context, req.question, req.mode).await?;
            let opinions = parse_opinions(&message);
            Ok(Json(ExpertOutput {
                action: "consult".into(),
                message,
                data: Some(ExpertData::Consult(ConsultData { opinions })),
            }))
        }
        ExpertAction::Configure => {
            let config_action = req
                .config_action
                .ok_or("config_action is required for action 'configure'")?;
            let role = req.role.clone();
            let message = configure_expert(
                ctx,
                config_action,
                req.role,
                req.prompt,
                req.provider,
                req.model,
            )
            .await?;
            let data = build_config_data(ctx, config_action, role).await?;
            Ok(Json(ExpertOutput {
                action: "configure".into(),
                message,
                data,
            }))
        }
    }
}

fn parse_opinions(message: &str) -> Vec<ExpertOpinion> {
    let parts: Vec<&str> = message.split("\n\n---\n\n").collect();
    let mut opinions = Vec::new();

    for part in parts {
        let mut lines = part.lines();
        let header = lines.next().unwrap_or("");
        let role = header
            .strip_prefix("## ")
            .and_then(|s| s.strip_suffix(" Analysis"))
            .unwrap_or("expert")
            .to_string();
        opinions.push(ExpertOpinion {
            role,
            content: part.to_string(),
        });
    }

    opinions
}

async fn build_config_data<C: ToolContext>(
    ctx: &C,
    action: ExpertConfigAction,
    role: Option<String>,
) -> Result<Option<ExpertData>, String> {
    match action {
        ExpertConfigAction::Providers => Ok(None),
        ExpertConfigAction::List => {
            let configs = ctx.pool().run(list_custom_prompts_sync).await?;
            let entries = configs
                .into_iter()
                .map(
                    |(role_key, prompt_text, provider_str, model_opt)| ExpertConfigEntry {
                        role: role_key,
                        provider: Some(provider_str),
                        model: model_opt,
                        has_custom_prompt: Some(!prompt_text.is_empty()),
                    },
                )
                .collect();
            Ok(Some(ExpertData::Configure(ConfigureData {
                configs: entries,
            })))
        }
        ExpertConfigAction::Get | ExpertConfigAction::Set | ExpertConfigAction::Delete => {
            let role_key = role.ok_or("role is required for config output")?;
            let role_key_clone = role_key.clone();
            let config: ExpertConfig = ctx
                .pool()
                .run(move |conn| get_expert_config_sync(conn, &role_key_clone))
                .await?;
            let entry = ExpertConfigEntry {
                role: role_key,
                provider: Some(config.provider.to_string()),
                model: config.model.clone(),
                has_custom_prompt: Some(
                    config
                        .prompt
                        .as_ref()
                        .map(|p| !p.is_empty())
                        .unwrap_or(false),
                ),
            };
            Ok(Some(ExpertData::Configure(ConfigureData {
                configs: vec![entry],
            })))
        }
    }
}
