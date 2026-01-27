// crates/mira-server/src/background/summaries.rs
// Rate-limited LLM summary generation

use crate::cartographer;
use crate::db::pool::DatabasePool;
use crate::db::{get_projects_with_pending_summaries_sync, get_modules_needing_summaries_sync, update_module_purposes_sync};
use crate::llm::{record_llm_usage, LlmClient, PromptBuilder};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Maximum summaries to process per batch
const BATCH_SIZE: usize = 5;

/// Delay between API calls (rate limiting)
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(2);

/// Process pending summaries with rate limiting
pub async fn process_queue(pool: &Arc<DatabasePool>, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
    // Get all projects with pending summaries
    let projects = pool.interact(move |conn| {
        get_projects_with_pending_summaries_sync(conn)
            .map_err(|e| anyhow::anyhow!("Failed to get projects: {}", e))
    }).await.map_err(|e| e.to_string())?;

    if projects.is_empty() {
        return Ok(0);
    }

    let mut total_processed = 0;

    for (project_id, project_path) in projects {
        // Get modules needing summaries for this project
        let mut modules = pool.interact(move |conn| {
            get_modules_needing_summaries_sync(conn, project_id)
                .map_err(|e| anyhow::anyhow!("Failed to get modules: {}", e))
        }).await.map_err(|e| e.to_string())?;

        if modules.is_empty() {
            continue;
        }

        // Limit to batch size
        modules.truncate(BATCH_SIZE);

        tracing::info!("Found {} modules needing summaries for project {}", modules.len(), project_id);

        // Fill in code previews
        let path = Path::new(&project_path);
        for module in &mut modules {
            module.code_preview = cartographer::get_module_code_preview(path, &module.path);
        }

        // Build prompt for batch
        let prompt = cartographer::build_summary_prompt(&modules);

        // Call LLM
        let messages = PromptBuilder::for_summaries()
            .build_messages(prompt);
        match client.chat(messages, None).await {
            Ok(result) => {
                // Record usage
                record_llm_usage(
                    pool,
                    client.provider_type(),
                    &client.model_name(),
                    "background:summaries",
                    &result,
                    Some(project_id),
                    None,
                ).await;

                if let Some(content) = result.content {
                    let summaries = cartographer::parse_summary_response(&content);
                    if !summaries.is_empty() {
                        match pool.interact(move |conn| {
                            update_module_purposes_sync(conn, project_id, &summaries)
                                .map_err(|e| anyhow::anyhow!("Failed to update: {}", e))
                        }).await
                        {
                            Ok(count) => {
                                tracing::info!(
                                    "Updated {} module summaries for project {}",
                                    count,
                                    project_id
                                );
                                total_processed += count;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to update summaries: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("LLM request failed: {}", e);
            }
        }

        // Rate limit between projects
        tokio::time::sleep(RATE_LIMIT_DELAY).await;
    }

    Ok(total_processed)
}
