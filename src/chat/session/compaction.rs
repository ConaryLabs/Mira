//! Code compaction blob storage and checkpoints

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use tracing::debug;
use uuid::Uuid;

use super::types::Checkpoint;

/// Load the most recent code compaction blob
pub async fn load_code_compaction(
    db: &SqlitePool,
    project_path: &str,
) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT encrypted_content FROM code_compaction
        WHERE project_path = $1
          AND (expires_at IS NULL OR expires_at > $2)
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_path)
    .bind(Utc::now().timestamp())
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(c,)| c))
}

/// Store a code compaction blob
pub async fn store_compaction(
    db: &SqlitePool,
    project_path: &str,
    encrypted_content: &str,
    files: &[String],
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO code_compaction (id, project_path, encrypted_content, files_included, created_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(&id)
    .bind(project_path)
    .bind(encrypted_content)
    .bind(serde_json::to_string(files)?)
    .bind(now)
    .execute(db)
    .await?;

    // Update context
    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_compaction_id = $1, updated_at = $2
        WHERE project_path = $3
        "#,
    )
    .bind(&id)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(id)
}

/// Save a checkpoint after successful tool execution (DeepSeek continuity)
///
/// Checkpoints replace server-side chain state for DeepSeek.
/// Stored in work_context with 24h TTL.
pub async fn save_checkpoint(
    db: &SqlitePool,
    project_path: &str,
    checkpoint: &Checkpoint,
) -> Result<()> {
    let now = Utc::now().timestamp();
    let expires_at = now + (24 * 3600); // 24 hour TTL
    let value = serde_json::to_string(checkpoint)?;

    sqlx::query(
        r#"
        INSERT INTO work_context (context_type, context_key, context_value, priority, expires_at, created_at, updated_at, project_id)
        VALUES ('deepseek_checkpoint', $1, $2, 0, $3, $4, $4, NULL)
        ON CONFLICT(context_type, context_key) DO UPDATE SET
            context_value = excluded.context_value,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(project_path)
    .bind(&value)
    .bind(expires_at)
    .bind(now)
    .execute(db)
    .await?;

    debug!("Saved checkpoint: {}", checkpoint.id);
    Ok(())
}

/// Load the most recent checkpoint for this project
pub async fn load_checkpoint(
    db: &SqlitePool,
    project_path: &str,
) -> Result<Option<Checkpoint>> {
    let now = Utc::now().timestamp();

    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT context_value FROM work_context
        WHERE context_type = 'deepseek_checkpoint'
          AND context_key = $1
          AND expires_at > $2
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_path)
    .bind(now)
    .fetch_optional(db)
    .await?;

    match row {
        Some((json,)) => {
            let checkpoint: Checkpoint = serde_json::from_str(&json)?;
            debug!("Loaded checkpoint: {}", checkpoint.id);
            Ok(Some(checkpoint))
        }
        None => Ok(None),
    }
}

/// Clear checkpoint (call after conversation reset)
pub async fn clear_checkpoint(
    db: &SqlitePool,
    project_path: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM work_context
        WHERE context_type = 'deepseek_checkpoint'
          AND context_key = $1
        "#,
    )
    .bind(project_path)
    .execute(db)
    .await?;

    debug!("Cleared checkpoint for {}", project_path);
    Ok(())
}
