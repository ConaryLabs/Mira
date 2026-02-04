// db/expert_config.rs
// Expert configuration accessors on DatabasePool.
//
// Separated from pool.rs to keep the pool module focused on connection management.

use super::pool::DatabasePool;
use anyhow::Result;

impl DatabasePool {
    /// Get custom system prompt for an expert role (async).
    /// Returns None if no custom prompt is set.
    pub async fn get_custom_prompt(&self, role: &str) -> Result<Option<String>> {
        let role = role.to_string();
        self.interact(move |conn| {
            let result = conn.query_row(
                "SELECT prompt FROM system_prompts WHERE role = ?",
                rusqlite::params![role],
                |row| row.get(0),
            );

            match result {
                Ok(prompt) => Ok(Some(prompt)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Get full expert configuration for a role (async).
    /// Returns default config if no custom config is set.
    pub async fn get_expert_config(&self, role: &str) -> Result<super::config::ExpertConfig> {
        use crate::llm::Provider;

        let role = role.to_string();
        self.interact(move |conn| {
            let result = conn.query_row(
                "SELECT prompt, provider, model FROM system_prompts WHERE role = ?",
                rusqlite::params![role],
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
                    Ok(super::config::ExpertConfig {
                        prompt,
                        provider,
                        model,
                    })
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Ok(super::config::ExpertConfig::default())
                }
                Err(e) => Err(e.into()),
            }
        })
        .await
    }
}
