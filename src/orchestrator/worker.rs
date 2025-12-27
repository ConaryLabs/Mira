//! Background worker for orchestrator jobs
//!
//! Handles async tasks like pre-summarization, cache warming, and housekeeping.
//! Runs as a long-lived task spawned at daemon startup.

use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::context::ContextCategory;
use super::{GeminiOrchestrator, OrchestratorJob, CategorySummary};

/// Background worker handle
pub struct OrchestratorWorker {
    /// Job sender for submitting work
    job_tx: mpsc::Sender<OrchestratorJob>,
    /// Handle to the worker task
    _task: tokio::task::JoinHandle<()>,
}

impl OrchestratorWorker {
    /// Spawn a new background worker
    pub fn spawn(
        orchestrator: Arc<RwLock<Option<GeminiOrchestrator>>>,
        db: SqlitePool,
        summarize_interval_secs: u64,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<OrchestratorJob>(100);

        let task = tokio::spawn(worker_loop(
            job_rx,
            orchestrator,
            db,
            summarize_interval_secs,
        ));

        Self { job_tx, _task: task }
    }

    /// Get a sender for submitting jobs
    pub fn sender(&self) -> mpsc::Sender<OrchestratorJob> {
        self.job_tx.clone()
    }

    /// Submit a job to the worker
    pub async fn submit(&self, job: OrchestratorJob) -> Result<()> {
        self.job_tx
            .send(job)
            .await
            .map_err(|_| anyhow::anyhow!("Worker channel closed"))
    }
}

/// Main worker loop
async fn worker_loop(
    mut job_rx: mpsc::Receiver<OrchestratorJob>,
    orchestrator: Arc<RwLock<Option<GeminiOrchestrator>>>,
    db: SqlitePool,
    summarize_interval_secs: u64,
) {
    info!("Orchestrator background worker started (summarize interval: {}s)", summarize_interval_secs);

    let mut summarize_interval = tokio::time::interval(Duration::from_secs(summarize_interval_secs));
    let mut housekeeping_interval = tokio::time::interval(Duration::from_secs(3600)); // Every hour

    // Skip first tick (fires immediately)
    summarize_interval.tick().await;
    housekeeping_interval.tick().await;

    loop {
        tokio::select! {
            // Process incoming jobs
            Some(job) = job_rx.recv() => {
                if let Err(e) = process_job(&orchestrator, &db, job).await {
                    warn!("Worker job failed: {}", e);
                }
            }

            // Periodic pre-summarization
            _ = summarize_interval.tick() => {
                debug!("Running periodic pre-summarization");
                if let Err(e) = run_pre_summarization(&orchestrator, &db).await {
                    debug!("Pre-summarization skipped: {}", e);
                }
            }

            // Periodic housekeeping
            _ = housekeeping_interval.tick() => {
                info!("Running periodic housekeeping");
                if let Err(e) = run_housekeeping(&orchestrator, &db).await {
                    warn!("Housekeeping failed: {}", e);
                }
            }
        }
    }
}

/// Process a single job
async fn process_job(
    orchestrator: &Arc<RwLock<Option<GeminiOrchestrator>>>,
    db: &SqlitePool,
    job: OrchestratorJob,
) -> Result<()> {
    match job {
        OrchestratorJob::ExtractDecisions {
            transcript,
            session_id,
            callback,
        } => {
            debug!("Processing ExtractDecisions job for session {}", session_id);

            let result = if let Some(orch) = orchestrator.read().await.as_ref() {
                orch.extract(&transcript).await?
            } else {
                // Fallback extraction without orchestrator
                super::types::ExtractionResult {
                    decisions: vec![],
                    topics: vec![],
                    files_modified: vec![],
                    insights: vec![],
                    confidence: 0.0,
                }
            };

            // Store in database
            for decision in &result.decisions {
                let _ = sqlx::query(
                    "INSERT INTO extracted_decisions (id, session_id, content, confidence, decision_type, extracted_at)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT(id) DO UPDATE SET content = excluded.content"
                )
                .bind(format!("{}-{}", session_id, md5_short(&decision.content)))
                .bind(&session_id)
                .bind(&decision.content)
                .bind(decision.confidence)
                .bind(decision.decision_type.as_str())
                .bind(Utc::now().timestamp())
                .execute(db)
                .await;
            }

            // Send result back
            let _ = callback.send(result);
        }

        OrchestratorJob::SummarizeCategory {
            category,
            token_budget,
            project_id,
        } => {
            debug!("Processing SummarizeCategory job for {:?}", category);

            if let Some(orch) = orchestrator.read().await.as_ref() {
                // Get raw content for this category
                let content = get_category_content(db, category, project_id).await?;

                if !content.is_empty() {
                    let summary = orch.summarize(&content, token_budget).await?;

                    // Store in database
                    sqlx::query(
                        "INSERT INTO category_summaries (category, content, token_count, generated_at)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT(category) DO UPDATE SET
                             content = excluded.content,
                             token_count = excluded.token_count,
                             generated_at = excluded.generated_at"
                    )
                    .bind(category.as_str())
                    .bind(&summary.content)
                    .bind(summary.compressed_tokens as i64)
                    .bind(Utc::now().timestamp())
                    .execute(db)
                    .await?;

                    // Update in-memory cache
                    let mut orch_write = orchestrator.write().await;
                    if let Some(ref mut o) = *orch_write {
                        let mut summaries = o.category_summaries.write().await;
                        summaries.insert(category, CategorySummary {
                            category,
                            content: summary.content,
                            token_count: summary.compressed_tokens,
                            generated_at: Utc::now(),
                            project_id,
                        });
                    }
                }
            }
        }

        OrchestratorJob::CacheRouting { query, decision } => {
            debug!("Caching routing decision for query");

            // Store in database
            sqlx::query(
                "INSERT INTO routing_cache (query_hash, category, confidence, created_at, hits)
                 VALUES ($1, $2, $3, $4, 1)
                 ON CONFLICT(query_hash) DO UPDATE SET
                     hits = routing_cache.hits + 1"
            )
            .bind(super::types::hash_query(&query))
            .bind(decision.primary.as_str())
            .bind(decision.confidence)
            .bind(Utc::now().timestamp())
            .execute(db)
            .await?;
        }

        OrchestratorJob::Housekeeping => {
            run_housekeeping(orchestrator, db).await?;
        }
    }

    Ok(())
}

/// Run pre-summarization for all categories
async fn run_pre_summarization(
    orchestrator: &Arc<RwLock<Option<GeminiOrchestrator>>>,
    db: &SqlitePool,
) -> Result<()> {
    let orch_guard = orchestrator.read().await;
    let orch = orch_guard.as_ref().ok_or_else(|| anyhow::anyhow!("No orchestrator"))?;

    if !orch.config().summarization_enabled || !orch.is_available() {
        return Ok(());
    }

    let token_budget = orch.config().summary_token_budget;
    drop(orch_guard); // Release read lock

    // Get active project ID (if any)
    let project_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE is_active = 1 LIMIT 1"
    )
    .fetch_optional(db)
    .await?;

    // Summarize high-priority categories
    let priority_categories = [
        ContextCategory::Goals,
        ContextCategory::Decisions,
        ContextCategory::RecentErrors,
    ];

    for category in priority_categories {
        let content = get_category_content(db, category, project_id).await?;

        if content.is_empty() {
            continue;
        }

        // Check if we need to update (content changed significantly)
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM category_summaries WHERE category = $1"
        )
        .bind(category.as_str())
        .fetch_optional(db)
        .await?;

        // Skip if content hasn't changed much
        if let Some((existing_content,)) = existing {
            if content.len().abs_diff(existing_content.len()) < 100 {
                continue;
            }
        }

        // Summarize
        let orch_guard = orchestrator.read().await;
        if let Some(orch) = orch_guard.as_ref() {
            match orch.summarize(&content, token_budget).await {
                Ok(summary) => {
                    drop(orch_guard);

                    sqlx::query(
                        "INSERT INTO category_summaries (category, content, token_count, generated_at)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT(category) DO UPDATE SET
                             content = excluded.content,
                             token_count = excluded.token_count,
                             generated_at = excluded.generated_at"
                    )
                    .bind(category.as_str())
                    .bind(&summary.content)
                    .bind(summary.compressed_tokens as i64)
                    .bind(Utc::now().timestamp())
                    .execute(db)
                    .await?;

                    debug!("Pre-summarized {:?}: {} -> {} tokens",
                        category, summary.original_tokens, summary.compressed_tokens);
                }
                Err(e) => {
                    debug!("Failed to summarize {:?}: {}", category, e);
                }
            }
        }
    }

    Ok(())
}

/// Run housekeeping tasks
async fn run_housekeeping(
    orchestrator: &Arc<RwLock<Option<GeminiOrchestrator>>>,
    db: &SqlitePool,
) -> Result<()> {
    // Clean up old debounce entries (older than 24 hours)
    if let Some(orch) = orchestrator.read().await.as_ref() {
        let cleaned = orch.cleanup_debounce(86400).await?;
        if cleaned > 0 {
            info!("Cleaned up {} old debounce entries", cleaned);
        }
    }

    // Clean up old routing cache entries (older than 1 hour)
    let routing_cleaned = sqlx::query(
        "DELETE FROM routing_cache WHERE created_at < $1"
    )
    .bind(Utc::now().timestamp() - 3600)
    .execute(db)
    .await?
    .rows_affected();

    if routing_cleaned > 0 {
        debug!("Cleaned up {} old routing cache entries", routing_cleaned);
    }

    // Clean up old extracted decisions (older than 7 days)
    let decisions_cleaned = sqlx::query(
        "DELETE FROM extracted_decisions WHERE extracted_at < $1"
    )
    .bind(Utc::now().timestamp() - 7 * 86400)
    .execute(db)
    .await?
    .rows_affected();

    if decisions_cleaned > 0 {
        debug!("Cleaned up {} old extracted decisions", decisions_cleaned);
    }

    // Vacuum to reclaim space (only if significant cleanup happened)
    if routing_cleaned + decisions_cleaned > 100 {
        let _ = sqlx::query("VACUUM")
            .execute(db)
            .await;
        info!("Database vacuumed after cleanup");
    }

    Ok(())
}

/// Get raw content for a category (for summarization)
async fn get_category_content(
    db: &SqlitePool,
    category: ContextCategory,
    project_id: Option<i64>,
) -> Result<String> {
    let content = match category {
        ContextCategory::Goals => {
            let goals: Vec<(String, String)> = sqlx::query_as(
                "SELECT title, description FROM goals
                 WHERE (project_id = $1 OR project_id IS NULL)
                   AND status IN ('active', 'in_progress')
                 ORDER BY priority DESC, created_at DESC
                 LIMIT 10"
            )
            .bind(project_id)
            .fetch_all(db)
            .await?;

            goals
                .iter()
                .map(|(t, d)| format!("- {}: {}", t, d))
                .collect::<Vec<_>>()
                .join("\n")
        }

        ContextCategory::Decisions => {
            let decisions: Vec<(String,)> = sqlx::query_as(
                "SELECT content FROM memories
                 WHERE fact_type = 'decision'
                   AND (project_id = $1 OR project_id IS NULL)
                 ORDER BY created_at DESC
                 LIMIT 20"
            )
            .bind(project_id)
            .fetch_all(db)
            .await?;

            decisions
                .iter()
                .map(|(c,)| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n")
        }

        ContextCategory::Memories => {
            let memories: Vec<(String,)> = sqlx::query_as(
                "SELECT content FROM memories
                 WHERE fact_type IN ('preference', 'fact')
                   AND (project_id = $1 OR project_id IS NULL)
                 ORDER BY confidence DESC, created_at DESC
                 LIMIT 20"
            )
            .bind(project_id)
            .fetch_all(db)
            .await?;

            memories
                .iter()
                .map(|(c,)| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n")
        }

        ContextCategory::RecentErrors => {
            let errors: Vec<(String, String)> = sqlx::query_as(
                "SELECT error_message, resolution FROM error_fixes
                 WHERE (project_id = $1 OR project_id IS NULL)
                 ORDER BY created_at DESC
                 LIMIT 10"
            )
            .bind(project_id)
            .fetch_all(db)
            .await?;

            errors
                .iter()
                .map(|(e, r)| format!("Error: {}\nFix: {}", e, r))
                .collect::<Vec<_>>()
                .join("\n\n")
        }

        ContextCategory::GitActivity => {
            let commits: Vec<(String, String)> = sqlx::query_as(
                "SELECT message, author FROM git_commits
                 WHERE project_id = $1
                 ORDER BY committed_at DESC
                 LIMIT 20"
            )
            .bind(project_id)
            .fetch_all(db)
            .await?;

            commits
                .iter()
                .map(|(m, a)| format!("- {} ({})", m.lines().next().unwrap_or(m), a))
                .collect::<Vec<_>>()
                .join("\n")
        }

        _ => String::new(),
    };

    Ok(content)
}

/// Short MD5 hash for IDs
fn md5_short(s: &str) -> String {
    format!("{:x}", md5::compute(s))[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_short() {
        let h = md5_short("test content");
        assert_eq!(h.len(), 8);
    }
}
