// crates/mira-server/src/db/config.rs
// Configuration storage (custom system prompts, LLM provider config, etc.)

use crate::embeddings::EmbeddingModel;
use crate::llm::Provider;
use anyhow::Result;
use rusqlite::{params, Connection};

use super::Database;

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
    let deleted = conn.execute(
        "DELETE FROM system_prompts WHERE role = ?",
        params![role],
    )?;
    Ok(deleted > 0)
}

/// List all custom prompts with provider info (sync version for pool.interact)
pub fn list_custom_prompts_sync(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT role, prompt, COALESCE(provider, 'deepseek'), model FROM system_prompts ORDER BY role",
    )?;

    let results = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

// ============================================================================
// Database impl methods
// ============================================================================

/// Key for storing embedding model in server_state
const EMBEDDING_MODEL_KEY: &str = "embedding_model";

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

impl Database {
    /// Get full expert configuration for a role
    /// Returns default config if no custom config is set
    pub fn get_expert_config(&self, role: &str) -> Result<ExpertConfig> {
        let conn = self.conn();
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
            Err(e) => Err(e.into()),
        }
    }

    /// Get custom system prompt for an expert role
    /// Returns None if no custom prompt is set (use default)
    pub fn get_custom_prompt(&self, role: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT prompt FROM system_prompts WHERE role = ?",
            params![role],
            |row| row.get(0),
        );

        match result {
            Ok(prompt) => Ok(Some(prompt)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set custom system prompt for an expert role
    pub fn set_custom_prompt(&self, role: &str, prompt: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO system_prompts (role, prompt, updated_at)
             VALUES (?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(role) DO UPDATE SET prompt = excluded.prompt, updated_at = CURRENT_TIMESTAMP",
            params![role, prompt],
        )?;
        Ok(())
    }

    /// Set LLM provider for an expert role
    pub fn set_expert_provider(&self, role: &str, provider: Provider, model: Option<&str>) -> Result<()> {
        let conn = self.conn();
        // Use a dummy prompt if none exists, we're primarily setting provider/model
        conn.execute(
            "INSERT INTO system_prompts (role, prompt, provider, model, updated_at)
             VALUES (?1, '', ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(role) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                updated_at = CURRENT_TIMESTAMP",
            params![role, provider.to_string(), model],
        )?;
        Ok(())
    }

    /// Set full expert configuration (prompt, provider, model)
    pub fn set_expert_config(
        &self,
        role: &str,
        prompt: Option<&str>,
        provider: Option<Provider>,
        model: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();

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

    /// Delete custom system prompt (revert to default)
    pub fn delete_custom_prompt(&self, role: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM system_prompts WHERE role = ?",
            params![role],
        )?;
        Ok(deleted > 0)
    }

    /// List all custom prompts with provider info
    pub fn list_custom_prompts(&self) -> Result<Vec<(String, String, String, Option<String>)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, prompt, COALESCE(provider, 'deepseek'), model FROM system_prompts ORDER BY role",
        )?;

        let results = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    // =========================================================================
    // Embedding Model Configuration
    // =========================================================================

    /// Get the configured embedding model
    /// Returns None if no model has been configured yet
    pub fn get_embedding_model(&self) -> Result<Option<EmbeddingModel>> {
        match self.get_server_state(EMBEDDING_MODEL_KEY)? {
            Some(name) => Ok(EmbeddingModel::from_name(&name)),
            None => Ok(None),
        }
    }

    /// Set the embedding model configuration
    /// WARNING: Changing the model after vectors exist requires re-indexing
    pub fn set_embedding_model(&self, model: EmbeddingModel) -> Result<()> {
        self.set_server_state(EMBEDDING_MODEL_KEY, model.model_name())
    }

    /// Check if embedding model can be safely used
    /// Returns Ok(()) if safe, Err with warning message if model mismatch detected
    pub fn check_embedding_model(&self, model: EmbeddingModel) -> Result<EmbeddingModelCheck> {
        let stored = self.get_embedding_model()?;

        match stored {
            None => {
                // First time setup - check if vectors already exist
                let has_vectors = self.has_existing_vectors()?;
                if has_vectors {
                    // Vectors exist but no model recorded - assume they match current model
                    // This handles migration from before model tracking was added
                    self.set_embedding_model(model)?;
                }
                Ok(EmbeddingModelCheck::FirstUse)
            }
            Some(stored_model) if stored_model == model => {
                Ok(EmbeddingModelCheck::Matches)
            }
            Some(stored_model) => {
                // Model mismatch!
                let has_vectors = self.has_existing_vectors()?;
                Ok(EmbeddingModelCheck::Mismatch {
                    stored: stored_model,
                    requested: model,
                    has_vectors,
                })
            }
        }
    }

    /// Check if any vectors exist in the database
    fn has_existing_vectors(&self) -> Result<bool> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM vec_code LIMIT 1",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        Ok(count > 0)
    }
}

/// Result of embedding model compatibility check
#[derive(Debug)]
pub enum EmbeddingModelCheck {
    /// First time using embeddings - no prior model configured
    FirstUse,
    /// Requested model matches stored model
    Matches,
    /// Model mismatch detected
    Mismatch {
        stored: EmbeddingModel,
        requested: EmbeddingModel,
        has_vectors: bool,
    },
}
