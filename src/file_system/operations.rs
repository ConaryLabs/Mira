// src/file_system/operations.rs
// File operations with modification history for undo functionality

use anyhow::{Result, Context};
use sqlx::SqlitePool;
use tracing::{info, warn};
use serde::Serialize;

/// Write file with history tracking for undo
/// IMPORTANT: This only executes when frontend sends explicit `files.write` command
/// after user clicks Apply button in the artifact viewer
pub async fn write_file_with_history(
    pool: &SqlitePool,
    project_id: &str,
    file_path: &str,
    content: &str,
) -> Result<()> {
    info!("User-initiated file write (via Apply button): project={}, file={}", project_id, file_path);
    
    // Get base path for project (works for both git and local directories)
    let base_path = crate::git::store::GitStore::new(pool.clone())
        .get_project_base_path(project_id)
        .await?;
    
    let full_path = base_path.join(file_path);
    
    // Read original content if file exists
    let original_content = if full_path.exists() {
        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => Some(content),
            Err(e) => {
                warn!("Could not read original content for {}: {}", file_path, e);
                None
            }
        }
    } else {
        info!("Creating new file: {}", file_path);
        None
    };
    
    // Save modification history (for undo) - only if file existed before
    if let Some(original) = &original_content {
        sqlx::query!(
            r#"
            INSERT INTO file_modifications 
                (project_id, file_path, original_content, modified_content, reverted)
            VALUES (?, ?, ?, ?, FALSE)
            "#,
            project_id,
            file_path,
            original,
            content
        )
        .execute(pool)
        .await
        .context("Failed to save file modification history")?;
        
        info!("Saved modification history for: {}", file_path);
    }
    
    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directories")?;
    }
    
    // Write new content to actual file on disk
    tokio::fs::write(&full_path, content)
        .await
        .context("Failed to write file to disk")?;
    
    // Increment project modification counter
    sqlx::query!(
        r#"
        UPDATE projects
        SET modification_count = COALESCE(modification_count, 0) + 1
        WHERE id = ?
        "#,
        project_id
    )
    .execute(pool)
    .await
    .context("Failed to update modification counter")?;
    
    info!("Successfully wrote file to disk: {}", full_path.display());
    Ok(())
}

/// Undo the most recent modification to a file
pub async fn undo_file_modification(
    pool: &SqlitePool,
    project_id: &str,
    file_path: &str,
) -> Result<()> {
    info!("Undoing file modification: project={}, file={}", project_id, file_path);
    
    // Find the most recent non-reverted modification
    let modification = sqlx::query!(
        r#"
        SELECT id, original_content
        FROM file_modifications
        WHERE project_id = ? AND file_path = ? AND reverted = FALSE
        ORDER BY modification_time DESC
        LIMIT 1
        "#,
        project_id,
        file_path
    )
    .fetch_optional(pool)
    .await
    .context("Failed to query modification history")?;
    
    let Some(mod_record) = modification else {
        anyhow::bail!("No modification history found for: {}", file_path);
    };
    
    // Get base path
    let base_path = crate::git::store::GitStore::new(pool.clone())
        .get_project_base_path(project_id)
        .await?;
    
    let full_path = base_path.join(file_path);
    
    // Restore original content
    tokio::fs::write(&full_path, &mod_record.original_content)
        .await
        .context("Failed to restore original file content")?;
    
    // Mark modification as reverted
    sqlx::query!(
        r#"
        UPDATE file_modifications
        SET reverted = TRUE
        WHERE id = ?
        "#,
        mod_record.id
    )
    .execute(pool)
    .await
    .context("Failed to mark modification as reverted")?;
    
    info!("Successfully undid modification for: {}", full_path.display());
    Ok(())
}

/// Get modification history for a file
pub async fn get_file_history(
    pool: &SqlitePool,
    project_id: &str,
    file_path: &str,
    limit: usize,
) -> Result<Vec<FileModification>> {
    let limit_i64 = limit as i64;
    
    let records = sqlx::query!(
        r#"
        SELECT id, project_id, file_path, original_content, modified_content, 
               modification_time, reverted
        FROM file_modifications
        WHERE project_id = ? AND file_path = ?
        ORDER BY modification_time DESC
        LIMIT ?
        "#,
        project_id,
        file_path,
        limit_i64
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch file modification history")?;
    
    Ok(records.into_iter().map(|r| FileModification {
        id: r.id.unwrap_or(0),
        project_id: r.project_id,
        file_path: r.file_path,
        original_content: r.original_content,
        modified_content: r.modified_content,
        modification_time: r.modification_time,
        reverted: r.reverted.unwrap_or(false),
    }).collect())
}

/// Get all modified files for a project
pub async fn get_modified_files(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<String>> {
    let records = sqlx::query!(
        r#"
        SELECT DISTINCT file_path
        FROM file_modifications
        WHERE project_id = ? AND reverted = FALSE
        ORDER BY file_path
        "#,
        project_id
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch modified files")?;
    
    Ok(records.into_iter().map(|r| r.file_path).collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct FileModification {
    pub id: i64,
    pub project_id: String,
    pub file_path: String,
    pub original_content: String,
    pub modified_content: String,
    pub modification_time: Option<i64>,
    pub reverted: bool,
}
