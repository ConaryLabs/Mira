//! Batch processing tool handler.
//!
//! Provides MCP tool interface for batch job management:
//! - Create compaction/summarize/analyze jobs
//! - List pending and running jobs
//! - Get job status and results
//! - Cancel running jobs

use anyhow::{anyhow, Result};
use serde::Serialize;
use sqlx::SqlitePool;
use tracing::debug;

use crate::batch::create_compaction_job;

/// Result of batch job operations
#[derive(Debug, Serialize)]
pub struct BatchResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<Vec<BatchJobInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<BatchJobInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Batch job information
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BatchJobInfo {
    pub id: i64,
    pub job_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_batch_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub request_count: Option<i64>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Create a new batch job
pub async fn create_job(
    db: &SqlitePool,
    job_type: &str,
    memory_ids: Option<&str>,
    project_id: Option<i64>,
) -> Result<i64> {
    match job_type {
        "compaction" => {
            let ids: Vec<i64> = memory_ids
                .ok_or_else(|| anyhow!("memory_ids required for compaction job"))?
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();

            if ids.is_empty() {
                return Err(anyhow!("No valid memory IDs provided"));
            }

            create_compaction_job(db, project_id, &ids, "gemini-2.0-flash").await
        }
        "summarize" | "analyze" => {
            // For now, create a pending job that the worker will pick up
            let now = chrono::Utc::now().timestamp();
            let job_id = sqlx::query(
                r#"INSERT INTO batch_jobs (project_id, job_type, status, created_at)
                   VALUES (?, ?, 'pending', ?)"#,
            )
            .bind(project_id)
            .bind(job_type)
            .bind(now)
            .execute(db)
            .await?
            .last_insert_rowid();

            Ok(job_id)
        }
        _ => Err(anyhow!("Unknown job type: {}", job_type)),
    }
}

/// List batch jobs
pub async fn list_jobs(
    db: &SqlitePool,
    limit: i64,
    include_completed: bool,
) -> Result<Vec<BatchJobInfo>> {
    let jobs: Vec<BatchJobInfo> = if include_completed {
        sqlx::query_as(
            r#"SELECT id, job_type, status, gemini_batch_name, display_name,
                      request_count, created_at, submitted_at, completed_at, error_message
               FROM batch_jobs
               ORDER BY created_at DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT id, job_type, status, gemini_batch_name, display_name,
                      request_count, created_at, submitted_at, completed_at, error_message
               FROM batch_jobs
               WHERE status NOT IN ('succeeded', 'failed', 'cancelled')
               ORDER BY created_at DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(db)
        .await?
    };

    Ok(jobs)
}

/// Get a specific batch job
pub async fn get_job(db: &SqlitePool, job_id: i64) -> Result<Option<BatchJobInfo>> {
    let job: Option<BatchJobInfo> = sqlx::query_as(
        r#"SELECT id, job_type, status, gemini_batch_name, display_name,
                  request_count, created_at, submitted_at, completed_at, error_message
           FROM batch_jobs
           WHERE id = ?"#,
    )
    .bind(job_id)
    .fetch_optional(db)
    .await?;

    Ok(job)
}

/// Cancel a batch job
pub async fn cancel_job(db: &SqlitePool, job_id: i64) -> Result<()> {
    // Only pending or submitted jobs can be cancelled
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        r#"UPDATE batch_jobs
           SET status = 'cancelled', completed_at = ?
           WHERE id = ? AND status IN ('pending', 'submitted', 'running')"#,
    )
    .bind(now)
    .bind(job_id)
    .execute(db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow!("Job {} not found or already completed", job_id));
    }

    debug!("Cancelled batch job {}", job_id);
    Ok(())
}

/// Handle batch tool requests
pub async fn handle_batch(
    db: &SqlitePool,
    project_id: Option<i64>,
    action: &str,
    request: &super::types::BatchRequest,
) -> Result<String> {

    let result = match action {
        "create" => {
            let job_type = request.job_type.as_deref()
                .ok_or_else(|| anyhow!("job_type required for create action"))?;

            let job_id = create_job(
                db,
                job_type,
                request.memory_ids.as_deref(),
                project_id,
            ).await?;

            BatchResult {
                action: "created".to_string(),
                job_id: Some(job_id),
                jobs: None,
                job: None,
                message: Some(format!("Created {} batch job {}", job_type, job_id)),
            }
        }
        "list" => {
            let limit = request.limit.unwrap_or(20);
            let include_completed = request.include_completed.unwrap_or(false);
            let jobs = list_jobs(db, limit, include_completed).await?;

            BatchResult {
                action: "list".to_string(),
                job_id: None,
                jobs: Some(jobs),
                job: None,
                message: None,
            }
        }
        "get" => {
            let job_id = request.job_id
                .ok_or_else(|| anyhow!("job_id required for get action"))?;

            let job = get_job(db, job_id).await?;

            if job.is_none() {
                return Err(anyhow!("Job {} not found", job_id));
            }

            BatchResult {
                action: "get".to_string(),
                job_id: Some(job_id),
                jobs: None,
                job,
                message: None,
            }
        }
        "cancel" => {
            let job_id = request.job_id
                .ok_or_else(|| anyhow!("job_id required for cancel action"))?;

            cancel_job(db, job_id).await?;

            BatchResult {
                action: "cancelled".to_string(),
                job_id: Some(job_id),
                jobs: None,
                job: None,
                message: Some(format!("Cancelled batch job {}", job_id)),
            }
        }
        _ => return Err(anyhow!("Unknown action: {}", action)),
    };

    Ok(serde_json::to_string_pretty(&result)?)
}
