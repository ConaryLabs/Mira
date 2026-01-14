// crates/mira-server/src/db/config.rs
// Configuration storage (custom system prompts, etc.)

use anyhow::Result;
use rusqlite::params;

use super::Database;

impl Database {
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

    /// Delete custom system prompt (revert to default)
    pub fn delete_custom_prompt(&self, role: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM system_prompts WHERE role = ?",
            params![role],
        )?;
        Ok(deleted > 0)
    }

    /// List all custom prompts
    pub fn list_custom_prompts(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, prompt FROM system_prompts ORDER BY role",
        )?;

        let results = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }
}
