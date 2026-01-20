// crates/mira-server/src/background/summaries.rs
// Rate-limited DeepSeek summary generation

use crate::cartographer;
use crate::db::Database;
use crate::llm::{DeepSeekClient, Message, PromptBuilder};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Maximum summaries to process per batch
const BATCH_SIZE: usize = 5;

/// Delay between API calls (rate limiting)
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(2);

/// Process pending summaries with rate limiting
pub async fn process_queue(db: &Arc<Database>, deepseek: &Arc<DeepSeekClient>) -> Result<usize, String> {
    // Get all projects with pending summaries (run on blocking thread)
    let db_clone = db.clone();
    let projects = Database::run_blocking(db_clone, |conn| {
        get_projects_with_pending_summaries(conn)
    }).await?;
    if projects.is_empty() {
        return Ok(0);
    }

    let mut total_processed = 0;

    for (project_id, project_path) in projects {
        // Get modules needing summaries for this project
        let mut modules = cartographer::get_modules_needing_summaries(db, project_id)
            .map_err(|e| e.to_string())?;

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

        // Call DeepSeek
        let messages = PromptBuilder::for_summaries()
            .build_messages(prompt);
        match deepseek.chat(messages, None).await {
            Ok(result) => {
                if let Some(content) = result.content {
                    let summaries = cartographer::parse_summary_response(&content);
                    if !summaries.is_empty() {
                        match cartographer::update_module_purposes(db, project_id, &summaries) {
                            Ok(count) => {
                                tracing::info!("Updated {} module summaries for project {}", count, project_id);
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
                tracing::warn!("DeepSeek request failed: {}", e);
            }
        }

        // Rate limit between projects
        tokio::time::sleep(RATE_LIMIT_DELAY).await;
    }

    Ok(total_processed)
}

/// Get projects that have modules needing summaries
fn get_projects_with_pending_summaries(conn: &rusqlite::Connection) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT m.project_id, p.path
             FROM codebase_modules m
             JOIN projects p ON p.id = m.project_id
             WHERE m.purpose IS NULL OR m.purpose = ''
             LIMIT 10",
        )
        .map_err(|e| e.to_string())?;

    let results: Vec<(i64, String)> = stmt
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}
