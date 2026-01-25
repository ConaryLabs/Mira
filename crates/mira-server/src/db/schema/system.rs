// crates/mira-server/src/db/schema/system.rs
// System configuration migrations

use anyhow::Result;
use rusqlite::Connection;
use crate::db::migration_helpers::{table_exists, column_exists};

/// Migrate system_prompts to add provider and model columns
pub fn migrate_system_prompts_provider(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "system_prompts") {
        return Ok(());
    }

    if !column_exists(conn, "system_prompts", "provider") {
        tracing::info!("Adding provider and model columns to system_prompts");
        conn.execute_batch(
            "ALTER TABLE system_prompts ADD COLUMN provider TEXT DEFAULT 'deepseek';
             ALTER TABLE system_prompts ADD COLUMN model TEXT;",
        )?;
    }

    Ok(())
}

/// Migrate system_prompts to strip old TOOL_USAGE_PROMPT suffix for KV cache optimization
pub fn migrate_system_prompts_strip_tool_suffix(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "system_prompts") {
        return Ok(());
    }

    // Get all prompts that might contain the old tool usage suffix
    let mut stmt = conn.prepare("SELECT role, prompt FROM system_prompts WHERE prompt LIKE '%Use tools to explore codebase before analysis.%'")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(Result::ok)
        .collect();

    if rows.is_empty() {
        return Ok(());
    }

    tracing::info!("Migrating {} system prompts to strip old tool usage suffix", rows.len());

    for (role, prompt) in rows {
        if let Some(pos) = prompt.find("Use tools to explore codebase before analysis.") {
            let stripped = prompt[..pos].trim_end().to_string();
            conn.execute(
                "UPDATE system_prompts SET prompt = ? WHERE role = ?",
                [&stripped, &role],
            )?;
            tracing::debug!("Stripped tool usage suffix from prompt for role: {}", role);
        }
    }

    Ok(())
}
