// src/tools/file_ops.rs
use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

/// Load complete file contents from git_files table
pub async fn load_complete_file(pool: &SqlitePool, path: &str, project_id: &str) -> Result<String> {
    let normalized_path = Path::new(path)
        .strip_prefix("./")
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .to_string();

    let result = sqlx::query_scalar::<_, String>(
        "SELECT content FROM git_files WHERE path = ? AND project_id = ?"
    )
    .bind(&normalized_path)
    .bind(project_id)
    .fetch_optional(pool)
    .await?;

    result.ok_or_else(|| anyhow::anyhow!("File not found: {}", path))
}

/// Check if path is a directory based on git_files
pub async fn check_is_directory(pool: &SqlitePool, path: &str, project_id: &str) -> Result<bool> {
    let pattern = format!("{}/%", path.trim_end_matches('/'));
    
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM git_files WHERE path LIKE ? AND project_id = ?"
    )
    .bind(&pattern)
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    Ok(count > 0)
}

/// List files in a directory
pub async fn list_project_files(pool: &SqlitePool, path: &str, project_id: &str) -> Result<Vec<String>> {
    let pattern = if path == "." || path.is_empty() {
        "%".to_string()
    } else {
        format!("{}/%", path.trim_end_matches('/'))
    };

    let files = sqlx::query_scalar::<_, String>(
        "SELECT path FROM git_files WHERE path LIKE ? AND project_id = ? ORDER BY path"
    )
    .bind(&pattern)
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(files)
}
