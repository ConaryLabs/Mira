// src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for proactive context injection

use crate::config::EnvConfig;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::{read_hook_input, write_hook_output};
use crate::proactive::background::get_pre_generated_suggestions;
use crate::proactive::{behavior::BehaviorTracker, predictor};
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Get database path (same as other hooks)
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Get embeddings client if available (with pool for usage tracking)
fn get_embeddings(pool: Option<Arc<DatabasePool>>) -> Option<Arc<EmbeddingClient>> {
    EmbeddingClient::from_env(pool).map(Arc::new)
}

/// Resolve active project ID and path in a single DB call
async fn resolve_project(pool: &Arc<DatabasePool>) -> (Option<i64>, Option<String>) {
    pool.interact(move |conn| {
        let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
        let result = if let Some(ref path) = path {
            crate::db::get_or_create_project_sync(conn, path, None)
                .ok()
                .map(|(id, _)| id)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((result, path))
    })
    .await
    .unwrap_or_default()
}

/// Log user query for behavior tracking
async fn log_behavior(pool: &Arc<DatabasePool>, project_id: i64, session_id: &str, message: &str) {
    let pool_clone = pool.clone();
    let session_id_clone = session_id.to_string();
    let message_clone = message.to_string();
    let _ = pool_clone
        .interact(move |conn| {
            let mut tracker = BehaviorTracker::for_session(conn, session_id_clone, project_id);
            let _ = tracker.log_query(conn, &message_clone, "user_prompt");
            Ok::<_, anyhow::Error>(())
        })
        .await;
}

/// Get proactive context predictions (hybrid: pre-generated + on-the-fly)
async fn get_proactive_context(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: Option<&str>,
) -> Option<String> {
    let project_path_owned = project_path.map(|s| s.to_string());
    pool.interact(move |conn| {
        let config =
            crate::proactive::get_proactive_config(conn, None, project_id).unwrap_or_default();

        if !config.enabled {
            return Ok::<Option<String>, anyhow::Error>(None);
        }

        let recent_files =
            crate::proactive::behavior::get_recent_file_sequence(conn, project_id, 3)
                .unwrap_or_default();
        let current_file = recent_files.first().cloned();

        // 1. Try pre-generated LLM suggestions (fast O(1) lookup)
        if let Some(ref file) = current_file
            && let Ok(pre_gen) = get_pre_generated_suggestions(conn, project_id, file)
            && !pre_gen.is_empty()
        {
            let context_lines: Vec<String> = pre_gen
                .iter()
                .take(2)
                .map(|(text, conf)| {
                    let conf_label = if *conf >= 0.9 {
                        "high confidence"
                    } else if *conf >= 0.7 {
                        "medium confidence"
                    } else {
                        "suggested"
                    };
                    format!("[Proactive] {} ({})", text, conf_label)
                })
                .collect();

            if !context_lines.is_empty() {
                return Ok(Some(context_lines.join("\n")));
            }
        }

        // 2. Fallback: On-the-fly pattern matching
        let current_context = predictor::CurrentContext {
            current_file,
            last_tool: None,
            recent_queries: vec![],
            session_stage: None,
        };

        let mut predictions =
            predictor::generate_context_predictions(conn, project_id, &current_context, &config)
                .unwrap_or_default();

        // Filter out stale file predictions
        if let Some(ref base) = project_path_owned {
            predictions.retain(|p| match p.prediction_type {
                predictor::PredictionType::NextFile | predictor::PredictionType::RelatedFiles => {
                    let exists = Path::new(base).join(&p.content).exists();
                    if !exists {
                        tracing::debug!("Dropping stale file prediction: {}", p.content);
                    }
                    exists
                }
                _ => true,
            });
        }

        if predictions.is_empty() {
            return Ok(None);
        }

        let suggestions = predictor::predictions_to_interventions(&predictions, &config);
        let context_lines: Vec<String> = suggestions
            .iter()
            .take(2)
            .map(|s| s.to_context_string())
            .collect();

        if context_lines.is_empty() {
            Ok(None)
        } else {
            Ok(Some(context_lines.join("\n")))
        }
    })
    .await
    .unwrap_or_default()
}

/// Get pending native tasks as context string
fn get_task_context() -> Option<String> {
    let dir = crate::tasks::find_current_task_list()?;
    match crate::tasks::get_pending_tasks(&dir) {
        Ok(pending) if !pending.is_empty() => {
            let lines: Vec<String> = pending
                .iter()
                .map(|t| {
                    let marker = if t.status == "in_progress" {
                        "[...]"
                    } else {
                        "[ ]"
                    };
                    format!("  {} {}", marker, t.subject)
                })
                .collect();
            let total = crate::tasks::count_tasks(&dir)
                .map(|(c, r)| c + r)
                .unwrap_or(0);
            let completed = total - pending.len();
            Some(format!(
                "[Mira] {} pending task(s) ({}/{} completed):\n{}",
                pending.len(),
                completed,
                total,
                lines.join("\n")
            ))
        }
        Ok(_) => None,
        Err(e) => {
            eprintln!("[mira] Failed to read native tasks: {}", e);
            None
        }
    }
}

/// Run UserPromptSubmit hook
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;

    let user_message = input
        .get("prompt")
        .or_else(|| input.get("user_message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session_id = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    eprintln!(
        "[mira] UserPromptSubmit hook triggered (session: {}, message length: {})",
        truncate_at_boundary(session_id, 8),
        user_message.len()
    );
    eprintln!(
        "[mira] Hook input keys: {:?}",
        input.as_object().map(|obj| obj.keys().collect::<Vec<_>>())
    );

    // Open database and create context injection manager
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(std::path::Path::new(&db_path)).await?);
    let env_config = EnvConfig::load();
    let embeddings = get_embeddings(Some(pool.clone()));
    let fuzzy = if env_config.fuzzy_fallback {
        Some(Arc::new(FuzzyCache::new()))
    } else {
        None
    };
    let manager =
        crate::context::ContextInjectionManager::new(pool.clone(), embeddings, fuzzy).await;

    // Resolve project once (eliminates duplicate get_last_active_project_sync calls)
    let (project_id, project_path) = resolve_project(&pool).await;

    // Log query event for behavior tracking
    if let Some(project_id) = project_id {
        log_behavior(&pool, project_id, session_id, user_message).await;
    }

    // Get relevant context with metadata
    let result = manager
        .get_context_for_message(user_message, session_id)
        .await;

    // Get proactive predictions if enabled
    let proactive_context: Option<String> = if let Some(project_id) = project_id {
        get_proactive_context(&pool, project_id, project_path.as_deref()).await
    } else {
        None
    };

    // Get pending native tasks
    let task_context = get_task_context();
    if task_context.is_some() {
        eprintln!("[mira] Added pending task context");
    }

    // Combine reactive context with proactive predictions
    let mut final_context = result.context.clone();
    let has_proactive = if let Some(proactive_str) = proactive_context {
        if !proactive_str.is_empty() {
            if final_context.is_empty() {
                final_context = proactive_str;
            } else {
                final_context = format!("{}\n\n{}", final_context, proactive_str);
            }
            eprintln!("[mira] Added proactive context suggestions");
            true
        } else {
            false
        }
    } else {
        false
    };

    if !final_context.is_empty() || task_context.is_some() {
        let mut output = serde_json::json!({});

        if !final_context.is_empty() {
            eprintln!("[mira] {}", result.summary());
            output["systemMessage"] = serde_json::json!(final_context);
            output["metadata"] = serde_json::json!({
                "sources": result.sources,
                "from_cache": result.from_cache,
                "has_proactive": has_proactive
            });
        }

        if let Some(tc) = task_context {
            output["hookSpecificOutput"] = serde_json::json!({
                "hookEventName": "UserPromptSubmit",
                "additionalContext": tc
            });
        }

        write_hook_output(&output);
    } else {
        if let Some(reason) = &result.skip_reason {
            eprintln!("[mira] Context injection skipped: {}", reason);
        }
        write_hook_output(&serde_json::json!({}));
    }

    Ok(())
}
