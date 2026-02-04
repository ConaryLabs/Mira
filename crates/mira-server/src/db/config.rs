// crates/mira-server/src/db/config.rs
// Configuration storage (custom system prompts, LLM provider config, etc.)

use crate::llm::Provider;
use rusqlite::{Connection, params};

/// (role, prompt, provider, model)
type PromptConfigRow = (String, String, String, Option<String>);

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Get full expert configuration for a role (sync version for pool.interact)
pub fn get_expert_config_sync(conn: &Connection, role: &str) -> rusqlite::Result<ExpertConfig> {
    let result = conn.query_row(
        "SELECT prompt, provider, model FROM system_prompts WHERE role = ?",
        params![role],
        |row| {
            let prompt: Option<String> = row.get(0)?;
            let provider_str: Option<String> = row.get(1)?;
            let model: Option<String> = row.get(2)?;
            Ok((prompt, provider_str, model))
        },
    );

    match result {
        Ok((prompt, provider_str, model)) => {
            let provider = provider_str
                .as_deref()
                .and_then(Provider::from_str)
                .unwrap_or(Provider::DeepSeek);
            Ok(ExpertConfig {
                prompt,
                provider,
                model,
            })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ExpertConfig::default()),
        Err(e) => Err(e),
    }
}

/// Set full expert configuration (sync version for pool.interact)
pub fn set_expert_config_sync(
    conn: &Connection,
    role: &str,
    prompt: Option<&str>,
    provider: Option<Provider>,
    model: Option<&str>,
) -> rusqlite::Result<()> {
    // Check if row exists
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM system_prompts WHERE role = ?",
            params![role],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if exists {
        // Update only provided fields
        if let Some(p) = prompt {
            conn.execute(
                "UPDATE system_prompts SET prompt = ?, updated_at = CURRENT_TIMESTAMP WHERE role = ?",
                params![p, role],
            )?;
        }
        if let Some(prov) = provider {
            conn.execute(
                "UPDATE system_prompts SET provider = ?, updated_at = CURRENT_TIMESTAMP WHERE role = ?",
                params![prov.to_string(), role],
            )?;
        }
        if model.is_some() {
            conn.execute(
                "UPDATE system_prompts SET model = ?, updated_at = CURRENT_TIMESTAMP WHERE role = ?",
                params![model, role],
            )?;
        }
    } else {
        // Insert new row
        let prompt_val = prompt.unwrap_or("");
        let provider_val = provider.unwrap_or(Provider::DeepSeek).to_string();
        conn.execute(
            "INSERT INTO system_prompts (role, prompt, provider, model, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
            params![role, prompt_val, provider_val, model],
        )?;
    }

    Ok(())
}

/// Delete custom system prompt (sync version for pool.interact)
pub fn delete_custom_prompt_sync(conn: &Connection, role: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM system_prompts WHERE role = ?", params![role])?;
    Ok(deleted > 0)
}

/// List all custom prompts with provider info (sync version for pool.interact)
pub fn list_custom_prompts_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<PromptConfigRow>> {
    let mut stmt = conn.prepare(
        "SELECT role, prompt, COALESCE(provider, 'deepseek'), model FROM system_prompts ORDER BY role",
    )?;

    let results = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

// ============================================================================
// Types
// ============================================================================

/// Expert configuration including prompt, provider, and model
#[derive(Debug, Clone)]
pub struct ExpertConfig {
    pub prompt: Option<String>,
    pub provider: Provider,
    pub model: Option<String>,
}

impl Default for ExpertConfig {
    fn default() -> Self {
        Self {
            prompt: None,
            provider: Provider::DeepSeek,
            model: None,
        }
    }
}
