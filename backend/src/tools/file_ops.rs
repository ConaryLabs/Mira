// src/tools/file_ops.rs
use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;
use tracing::{debug, info};

/// Load complete file contents - tries project repo first, then falls back to backend working directory
pub async fn load_complete_file(pool: &SqlitePool, path: &str, project_id: &str) -> Result<String> {
    let normalized_path = Path::new(path)
        .strip_prefix("./")
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .to_string();

    // Try project repo first if project has git attachment
    if let Some(content) = try_project_repo(pool, &normalized_path, project_id).await? {
        return Ok(content);
    }

    // Fallback: Try backend working directory
    info!(
        "File not found in project repo, trying backend working directory: {}",
        normalized_path
    );
    let backend_path = Path::new(&normalized_path);

    match tokio::fs::read_to_string(&backend_path).await {
        Ok(content) => {
            debug!(
                "Successfully read from backend working directory: {}",
                normalized_path
            );
            Ok(content)
        }
        Err(e) => Err(anyhow::anyhow!(
            "Failed to read file '{}': not found in project repo or backend directory ({})",
            normalized_path,
            e
        )),
    }
}

/// Try to read file from project's git repository
async fn try_project_repo(
    pool: &SqlitePool,
    normalized_path: &str,
    project_id: &str,
) -> Result<Option<String>> {
    // Get git attachment for project (if exists)
    let attachment = match sqlx::query!(
        r#"SELECT id, local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    {
        Some(att) => att,
        None => {
            debug!(
                "No git attachment for project {}, will try backend directory",
                project_id
            );
            return Ok(None);
        }
    };

    // Try to read from project repo
    let full_path = Path::new(&attachment.local_path).join(normalized_path);

    match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => {
            debug!(
                "Successfully read from project repo: {}",
                full_path.display()
            );
            Ok(Some(content))
        }
        Err(e) => {
            debug!("File not in project repo {}: {}", full_path.display(), e);
            Ok(None) // Return None to trigger fallback
        }
    }
}

/// Check if path is a directory - tries project repo first, then backend directory
pub async fn check_is_directory(pool: &SqlitePool, path: &str, project_id: &str) -> Result<bool> {
    // Try project repo first
    if let Some(attachment) = sqlx::query!(
        r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    {
        let full_path = Path::new(&attachment.local_path).join(path);
        if let Ok(metadata) = tokio::fs::metadata(&full_path).await {
            return Ok(metadata.is_dir());
        }
    }

    // Fallback: Check backend working directory
    let backend_path = Path::new(path);
    Ok(tokio::fs::metadata(&backend_path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false))
}

/// List files in a directory - tries project repo first, then backend directory
pub async fn list_project_files(
    pool: &SqlitePool,
    path: &str,
    project_id: &str,
) -> Result<Vec<String>> {
    // Try project repo first
    if let Some(files) = try_list_project_repo(pool, path, project_id).await? {
        return Ok(files);
    }

    // Fallback: List from backend working directory
    info!("Listing from backend working directory: {}", path);
    let backend_path = Path::new(path);
    let dir_path = if path.is_empty() || path == "." {
        std::env::current_dir()?
    } else {
        backend_path.to_path_buf()
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

/// Try to list files from project's git repository
async fn try_list_project_repo(
    pool: &SqlitePool,
    path: &str,
    project_id: &str,
) -> Result<Option<Vec<String>>> {
    // Get git attachment
    let attachment = match sqlx::query!(
        r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    {
        Some(att) => att,
        None => {
            debug!(
                "No git attachment for project {}, will list backend directory",
                project_id
            );
            return Ok(None);
        }
    };

    let base_path = Path::new(&attachment.local_path);
    let dir_path = if path.is_empty() || path == "." {
        base_path.to_path_buf()
    } else {
        base_path.join(path)
    };

    // Try to read directory
    let mut dir = match tokio::fs::read_dir(&dir_path).await {
        Ok(d) => d,
        Err(e) => {
            debug!(
                "Failed to list project repo directory {}: {}",
                dir_path.display(),
                e
            );
            return Ok(None); // Trigger fallback
        }
    };

    let mut files = Vec::new();
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
    Ok(Some(files))
}
