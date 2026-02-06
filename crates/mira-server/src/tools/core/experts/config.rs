// crates/mira-server/src/tools/core/experts/config.rs
// Expert configuration management

use super::ToolContext;
use super::role::ExpertRole;
use crate::mcp::requests::ExpertConfigAction;
use crate::utils::truncate;

/// Validate that a role key is a known expert role or special system role.
fn validate_role(role_key: &str) -> Result<(), String> {
    let is_valid = ExpertRole::from_db_key(role_key).is_some() || role_key == "background";
    if !is_valid {
        return Err(format!(
            "Invalid role '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security, background",
            role_key
        ));
    }
    Ok(())
}

/// Configure expert system prompts and LLM providers (set, get, delete, list, providers)
pub async fn configure_expert<C: ToolContext>(
    ctx: &C,
    action: ExpertConfigAction,
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

    match action {
        ExpertConfigAction::Set => {
            let role_key = role.as_deref().ok_or("Role is required for 'set' action")?;
            validate_role(role_key)?;

            // Parse provider if provided
            let parsed_provider = if let Some(ref p) = provider {
                Some(Provider::from_str(p).ok_or_else(|| {
                    format!(
                        "Invalid provider '{}'. Valid providers: deepseek, zhipu, ollama",
                        p
                    )
                })?)
            } else {
                None
            };

            // At least one of prompt, provider, or model should be set
            if prompt.is_none() && parsed_provider.is_none() && model.is_none() {
                return Err(
                    "At least one of prompt, provider, or model is required for 'set' action"
                        .to_string(),
                );
            }

            let role_key_clone = role_key.to_string();
            let prompt_clone = prompt.clone();
            let model_clone = model.clone();

            ctx.pool()
                .run(move |conn| {
                    set_expert_config_sync(
                        conn,
                        &role_key_clone,
                        prompt_clone.as_deref(),
                        parsed_provider,
                        model_clone.as_deref(),
                    )
                })
                .await?;

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

        ExpertConfigAction::Get => {
            let role_key = role.as_deref().ok_or("Role is required for 'get' action")?;
            validate_role(role_key)?;
            let expert = ExpertRole::from_db_key(role_key);

            let role_key_clone = role_key.to_string();
            let config = ctx
                .pool()
                .run(move |conn| get_expert_config_sync(conn, &role_key_clone))
                .await?;

            let role_name = expert
                .map(|e| e.name())
                .unwrap_or_else(|| "Background Worker".to_string());
            let mut output = format!("Configuration for '{}' ({}):\n", role_key, role_name);
            output.push_str(&format!("  Provider: {}\n", config.provider));
            if let Some(ref m) = config.model {
                output.push_str(&format!("  Model: {}\n", m));
            } else {
                output.push_str(&format!(
                    "  Model: {} (default)\n",
                    config.provider.default_model()
                ));
            }
            if let Some(ref p) = config.prompt {
                output.push_str(&format!("  Custom prompt: {}\n", truncate(p, 200)));
            } else {
                output.push_str("  Prompt: (default)\n");
            }
            Ok(output)
        }

        ExpertConfigAction::Delete => {
            let role_key = role
                .as_deref()
                .ok_or("Role is required for 'delete' action")?;
            validate_role(role_key)?;

            let role_key_clone = role_key.to_string();
            let deleted = ctx
                .pool()
                .run(move |conn| delete_custom_prompt_sync(conn, &role_key_clone))
                .await?;

            if deleted {
                Ok(format!(
                    "Configuration deleted for '{}'. Reverted to defaults.",
                    role_key
                ))
            } else {
                Ok(format!(
                    "No custom configuration was set for '{}'.",
                    role_key
                ))
            }
        }

        ExpertConfigAction::List => {
            let configs = ctx.pool().run(list_custom_prompts_sync).await?;

            if configs.is_empty() {
                Ok("No custom configurations. All experts use default settings.".to_string())
            } else {
                let mut output = format!("{} expert configurations:\n\n", configs.len());
                for (role_key, prompt_text, provider_str, model_opt) in configs {
                    let prompt_preview = if prompt_text.is_empty() {
                        "(default)".to_string()
                    } else {
                        truncate(&prompt_text, 50)
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

        ExpertConfigAction::Providers => {
            // List available LLM providers
            let factory = ctx.llm_factory();
            let available = factory.available_providers();

            if available.is_empty() {
                Ok(
                    "No LLM providers available. Set DEEPSEEK_API_KEY, ZHIPU_API_KEY, or OLLAMA_HOST."
                        .to_string(),
                )
            } else {
                let mut output = format!("{} LLM providers available:\n\n", available.len());
                for p in &available {
                    let is_default = factory.default_provider() == Some(*p);
                    let default_marker = if is_default { " (default)" } else { "" };
                    output.push_str(&format!(
                        "  {}: model={}{}\n",
                        p,
                        p.default_model(),
                        default_marker
                    ));
                }
                output.push_str("\nSet DEFAULT_LLM_PROVIDER env var to change the global default.");
                Ok(output)
            }
        }
    }
}
