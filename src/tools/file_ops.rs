// src/tools/file_ops.rs
use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

/// Load complete file contents from git repository on disk
pub async fn load_complete_file(pool: &SqlitePool, path: &str, project_id: &str) -> Result<String> {
    let normalized_path = Path::new(path)
        .strip_prefix("./")
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .to_string();

    // Get git attachment for project
    let attachment = sqlx::query!(
        r#"SELECT id, local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("No git repository attached to project"))?;

    // Read file from git repository on disk
    let full_path = Path::new(&attachment.local_path).join(&normalized_path);
    
    tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", normalized_path, e))
}

/// Check if path is a directory in the git repository
pub async fn check_is_directory(pool: &SqlitePool, path: &str, project_id: &str) -> Result<bool> {
    // Get git attachment
    let attachment = sqlx::query!(
        r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("No git repository attached to project"))?;

    let full_path = Path::new(&attachment.local_path).join(path);
    
    Ok(tokio::fs::metadata(&full_path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false))
}

/// List files in a directory from git repository
pub async fn list_project_files(pool: &SqlitePool, path: &str, project_id: &str) -> Result<Vec<String>> {
    // Get git attachment
    let attachment = sqlx::query!(
        r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("No git repository attached to project"))?;

    let base_path = Path::new(&attachment.local_path);
    let dir_path = if path.is_empty() || path == "." {
        base_path.to_path_buf()
    } else {
        base_path.join(path)
    };

    let mut files = Vec::new();
    let mut dir = tokio::fs::read_dir(&dir_path).await?;

    while let Some(entry) = dir.next_entry().await? {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let relative_path = if path.is_empty() || path == "." {
            file_name
        } else {
            format!("{}/{}", path.trim_end_matches('/'), file_name)
        };
        files.push(relative_path);
    }

    files.sort();
    Ok(files)
}
