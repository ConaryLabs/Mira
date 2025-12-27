//! Background worker for batch job processing.
//!
//! Manages the lifecycle of batch jobs:
//! 1. Polls for pending jobs in local database
//! 2. Submits them to Gemini Batch API
//! 3. Monitors running jobs for completion
//! 4. Processes results and updates local state

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::chat::provider::{BatchClient, BatchRequest, BatchState, build_batch_request};

/// Job types that can be processed via batch API
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchJobType {
    /// Memory compaction - consolidate and summarize memories
    Compaction,
    /// Document summarization
    Summarize,
    /// Codebase analysis
    Analyze,
    /// Custom job type (stored as string in DB)
    Custom,
}

impl BatchJobType {
    /// Convert from database string
    pub fn from_str(s: &str) -> Self {
        match s {
            "compaction" => Self::Compaction,
            "summarize" => Self::Summarize,
            "analyze" => Self::Analyze,
            _ => Self::Custom,
        }
    }

    /// Convert to database string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
            Self::Summarize => "summarize",
            Self::Analyze => "analyze",
            Self::Custom => "custom",
        }
    }
}

/// Configuration for the batch worker
#[derive(Debug, Clone)]
pub struct BatchWorkerConfig {
    /// How often to poll for new/completed jobs (seconds)
    pub poll_interval_secs: u64,
    /// Maximum jobs to submit in one poll cycle
    pub max_jobs_per_cycle: usize,
    /// Model to use for batch processing
    pub model: String,
    /// API key for Gemini
    pub api_key: String,
}

impl Default for BatchWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            max_jobs_per_cycle: 10,
            model: "gemini-2.0-flash".to_string(),
            api_key: String::new(),
        }
    }
}

/// Background worker for batch processing
pub struct BatchWorker {
    db: SqlitePool,
    config: BatchWorkerConfig,
    client: BatchClient,
    /// Flag to signal shutdown
    shutdown: Arc<RwLock<bool>>,
}

/// Batch job record from database
#[derive(Debug, sqlx::FromRow)]
struct BatchJobRecord {
    id: i64,
    project_id: Option<i64>,
    job_type: String,
    status: String,
    gemini_batch_name: Option<String>,
    display_name: Option<String>,
    input_data: Option<String>,
    request_count: Option<i64>,  // SQLite returns INTEGER which maps to i64
}

/// Batch request record from database
#[derive(Debug, sqlx::FromRow)]
struct BatchRequestRecord {
    id: i64,
    request_key: String,
    request_data: String,
}

impl BatchWorker {
    /// Create a new batch worker
    pub fn new(db: SqlitePool, config: BatchWorkerConfig) -> Self {
        let client = BatchClient::new(&config.api_key);
        Self {
            db,
            config,
            client,
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Get a shutdown handle for external shutdown signaling
    pub fn shutdown_handle(&self) -> Arc<RwLock<bool>> {
        self.shutdown.clone()
    }

    /// Run the worker loop
    pub async fn run(&self) -> Result<()> {
        info!("Batch worker starting with {}s poll interval", self.config.poll_interval_secs);

        loop {
            // Check for shutdown signal
            if *self.shutdown.read().await {
                info!("Batch worker shutting down");
                break;
            }

            // Process pending and running jobs
            if let Err(e) = self.poll_cycle().await {
                error!("Batch worker poll cycle error: {}", e);
            }

            // Sleep until next cycle
            tokio::time::sleep(tokio::time::Duration::from_secs(self.config.poll_interval_secs)).await;
        }

        Ok(())
    }

    /// Run one poll cycle
    async fn poll_cycle(&self) -> Result<()> {
        // 1. Submit pending jobs
        self.submit_pending_jobs().await?;

        // 2. Check status of running jobs
        self.check_running_jobs().await?;

        Ok(())
    }

    /// Submit pending jobs to Gemini Batch API
    async fn submit_pending_jobs(&self) -> Result<()> {
        // Get pending jobs
        let pending_jobs: Vec<BatchJobRecord> = sqlx::query_as(
            r#"SELECT id, project_id, job_type, status, gemini_batch_name,
                      display_name, input_data, request_count
               FROM batch_jobs
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT ?"#,
        )
        .bind(self.config.max_jobs_per_cycle as i32)
        .fetch_all(&self.db)
        .await?;

        if pending_jobs.is_empty() {
            return Ok(());
        }

        debug!("Found {} pending batch jobs", pending_jobs.len());

        for job in pending_jobs {
            if let Err(e) = self.submit_job(&job).await {
                error!("Failed to submit batch job {}: {}", job.id, e);
                // Mark as failed
                self.mark_job_failed(job.id, &e.to_string()).await?;
            }
        }

        Ok(())
    }

    /// Submit a single job to Gemini
    async fn submit_job(&self, job: &BatchJobRecord) -> Result<()> {
        // Get requests for this job
        let requests: Vec<BatchRequestRecord> = sqlx::query_as(
            "SELECT id, request_key, request_data FROM batch_requests WHERE job_id = ?",
        )
        .bind(job.id)
        .fetch_all(&self.db)
        .await?;

        if requests.is_empty() {
            return Err(anyhow::anyhow!("No requests found for batch job {}", job.id));
        }

        // Convert to Gemini batch requests
        let batch_requests: Vec<BatchRequest> = requests
            .iter()
            .map(|r| {
                // Parse the stored request data
                let request_data: serde_json::Value = serde_json::from_str(&r.request_data)
                    .unwrap_or_else(|_| serde_json::json!({}));

                BatchRequest {
                    custom_id: r.request_key.clone(),
                    request: request_data,
                }
            })
            .collect();

        // Submit to Gemini
        let batch = self.client.create_batch(
            &self.config.model,
            batch_requests,
            job.display_name.as_deref(),
        ).await?;

        // Update job with Gemini batch name
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE batch_jobs SET status = 'submitted', gemini_batch_name = ?, submitted_at = ? WHERE id = ?",
        )
        .bind(&batch.name)
        .bind(now)
        .bind(job.id)
        .execute(&self.db)
        .await?;

        info!("Submitted batch job {} as {}", job.id, batch.name);

        Ok(())
    }

    /// Check status of running jobs
    async fn check_running_jobs(&self) -> Result<()> {
        // Get submitted/running jobs
        let running_jobs: Vec<BatchJobRecord> = sqlx::query_as(
            r#"SELECT id, project_id, job_type, status, gemini_batch_name,
                      display_name, input_data, request_count
               FROM batch_jobs
               WHERE status IN ('submitted', 'running')
               AND gemini_batch_name IS NOT NULL"#,
        )
        .fetch_all(&self.db)
        .await?;

        if running_jobs.is_empty() {
            return Ok(());
        }

        debug!("Checking {} running batch jobs", running_jobs.len());

        for job in running_jobs {
            if let Some(batch_name) = &job.gemini_batch_name {
                if let Err(e) = self.check_job_status(&job, batch_name).await {
                    warn!("Failed to check batch job {}: {}", job.id, e);
                }
            }
        }

        Ok(())
    }

    /// Check status of a single job
    async fn check_job_status(&self, job: &BatchJobRecord, batch_name: &str) -> Result<()> {
        let batch = self.client.get_batch(batch_name).await?;

        // Map Gemini state to our status
        let new_status = batch.state.to_status();

        // Update local status if changed
        if new_status != job.status {
            debug!("Batch job {} changed from {} to {}", job.id, job.status, new_status);

            let now = chrono::Utc::now().timestamp();

            if batch.state.is_terminal() {
                // Job completed - update with completion time
                let error_msg = batch.error.as_ref().map(|e| e.message.clone().unwrap_or_default());

                sqlx::query(
                    "UPDATE batch_jobs SET status = ?, completed_at = ?, error_message = ? WHERE id = ?",
                )
                .bind(new_status)
                .bind(now)
                .bind(&error_msg)
                .bind(job.id)
                .execute(&self.db)
                .await?;

                if batch.state == BatchState::JobStateSucceeded {
                    // Process results
                    self.process_job_results(job).await?;
                }

                info!(
                    "Batch job {} completed with status: {} ({}/{} succeeded)",
                    job.id,
                    new_status,
                    batch.succeeded_count.unwrap_or(0),
                    batch.total_count.unwrap_or(0)
                );
            } else {
                // Still running - just update status
                sqlx::query(
                    "UPDATE batch_jobs SET status = ? WHERE id = ?",
                )
                .bind(new_status)
                .bind(job.id)
                .execute(&self.db)
                .await?;
            }
        }

        Ok(())
    }

    /// Process results of a completed job
    async fn process_job_results(&self, job: &BatchJobRecord) -> Result<()> {
        // Get results from Gemini (if available)
        if let Some(batch_name) = &job.gemini_batch_name {
            let results = self.client.get_results(batch_name).await?;

            // Update individual request records
            for result in results {
                let status = if result.error.is_some() { "failed" } else { "succeeded" };
                let response_data = result.response.map(|r| r.to_string());
                let error_msg = result.error.map(|e| e.message.unwrap_or_default());
                let now = chrono::Utc::now().timestamp();

                sqlx::query(
                    r#"UPDATE batch_requests
                       SET status = ?, response_data = ?, error_message = ?, completed_at = ?
                       WHERE job_id = ? AND request_key = ?"#,
                )
                .bind(status)
                .bind(&response_data)
                .bind(&error_msg)
                .bind(now)
                .bind(job.id)
                .bind(&result.custom_id)
                .execute(&self.db)
                .await?;
            }
        }

        // Run job-type specific post-processing
        match BatchJobType::from_str(&job.job_type) {
            BatchJobType::Compaction => self.process_compaction_results(job).await?,
            BatchJobType::Summarize => self.process_summarize_results(job).await?,
            BatchJobType::Analyze => self.process_analyze_results(job).await?,
            BatchJobType::Custom => {} // No special processing
        }

        Ok(())
    }

    /// Process compaction job results
    async fn process_compaction_results(&self, job: &BatchJobRecord) -> Result<()> {
        // Get completed requests
        let requests: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT request_key, response_data FROM batch_requests WHERE job_id = ? AND status = 'succeeded'"
        )
        .bind(job.id)
        .fetch_all(&self.db)
        .await?;

        debug!("Processing {} compaction results for job {}", requests.len(), job.id);

        // For each successful compaction, update the memory with compacted content
        for (request_key, response_data) in requests {
            if let Some(response) = response_data {
                // Parse the response to extract compacted memory
                if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&response) {
                    // Extract text from response
                    if let Some(text) = response_json
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("content"))
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.get(0))
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        // The request_key should be "memory_{id}"
                        if let Some(memory_id) = request_key.strip_prefix("memory_") {
                            // Update the memory with compacted content
                            let now = chrono::Utc::now().timestamp();
                            sqlx::query(
                                r#"UPDATE memories
                                   SET content = ?, confidence = 0.8, updated_at = ?
                                   WHERE id = ?"#,
                            )
                            .bind(text)
                            .bind(now)
                            .bind(memory_id)
                            .execute(&self.db)
                            .await?;

                            debug!("Updated memory {} with compacted content", memory_id);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Process summarize job results
    async fn process_summarize_results(&self, job: &BatchJobRecord) -> Result<()> {
        // Summarization results are typically stored as documents
        debug!("Processing summarize results for job {}", job.id);
        // Implementation depends on where summaries should be stored
        Ok(())
    }

    /// Process analyze job results
    async fn process_analyze_results(&self, job: &BatchJobRecord) -> Result<()> {
        // Analysis results might update decisions, insights, etc.
        debug!("Processing analyze results for job {}", job.id);
        // Implementation depends on what kind of analysis was performed
        Ok(())
    }

    /// Mark a job as failed
    async fn mark_job_failed(&self, job_id: i64, error: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE batch_jobs SET status = 'failed', error_message = ?, completed_at = ? WHERE id = ?",
        )
        .bind(error)
        .bind(now)
        .bind(job_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

/// Create a batch job for memory compaction
pub async fn create_compaction_job(
    db: &SqlitePool,
    project_id: Option<i64>,
    memory_ids: &[i64],
    model: &str,
) -> Result<i64> {
    if memory_ids.is_empty() {
        return Err(anyhow::anyhow!("No memories to compact"));
    }

    let now = chrono::Utc::now().timestamp();

    // Create the batch job
    let job_id = sqlx::query(
        r#"INSERT INTO batch_jobs (project_id, job_type, status, request_count, created_at)
           VALUES (?, 'compaction', 'pending', ?, ?)"#,
    )
    .bind(project_id)
    .bind(memory_ids.len() as i32)
    .bind(now)
    .execute(db)
    .await?
    .last_insert_rowid();

    // Get memories and create requests
    for memory_id in memory_ids {
        let memory: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM memories WHERE id = ?"
        )
        .bind(memory_id)
        .fetch_optional(db)
        .await?;

        if let Some((content,)) = memory {
            let request_key = format!("memory_{}", memory_id);

            // Build the compaction request
            let request = build_batch_request(
                &request_key,
                model,
                vec![serde_json::json!({
                    "role": "user",
                    "parts": [{"text": format!(
                        "Compact this memory into a concise, essential summary while preserving key facts and context:\n\n{}",
                        content
                    )}]
                })],
                Some("You are a memory compaction assistant. Summarize the given memory into its essential points, preserving key facts, decisions, and context. Be concise but complete."),
            );

            // Insert the request
            let request_data = serde_json::to_string(&request.request)?;
            sqlx::query(
                r#"INSERT INTO batch_requests (job_id, request_key, request_data, status, created_at)
                   VALUES (?, ?, ?, 'pending', ?)"#,
            )
            .bind(job_id)
            .bind(&request_key)
            .bind(&request_data)
            .bind(now)
            .execute(db)
            .await?;
        }
    }

    info!("Created compaction batch job {} with {} memories", job_id, memory_ids.len());

    Ok(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_job_type_conversion() {
        assert_eq!(BatchJobType::from_str("compaction"), BatchJobType::Compaction);
        assert_eq!(BatchJobType::from_str("summarize"), BatchJobType::Summarize);
        assert_eq!(BatchJobType::from_str("analyze"), BatchJobType::Analyze);
        assert_eq!(BatchJobType::from_str("unknown"), BatchJobType::Custom);

        assert_eq!(BatchJobType::Compaction.as_str(), "compaction");
        assert_eq!(BatchJobType::Summarize.as_str(), "summarize");
        assert_eq!(BatchJobType::Analyze.as_str(), "analyze");
        assert_eq!(BatchJobType::Custom.as_str(), "custom");
    }

    #[test]
    fn test_default_config() {
        let config = BatchWorkerConfig::default();
        assert_eq!(config.poll_interval_secs, 60);
        assert_eq!(config.max_jobs_per_cycle, 10);
        assert_eq!(config.model, "gemini-2.0-flash");
    }
}
