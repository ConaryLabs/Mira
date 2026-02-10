// src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for proactive context injection

use crate::config::EnvConfig;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::{get_db_path, read_hook_input, resolve_project, write_hook_output};
use crate::proactive::background::get_pre_generated_suggestions;
use crate::proactive::{behavior::BehaviorTracker, predictor};
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

/// Get embeddings client if available (with pool for usage tracking)
fn get_embeddings(pool: Option<Arc<DatabasePool>>) -> Option<Arc<EmbeddingClient>> {
    EmbeddingClient::from_env(pool).map(Arc::new)
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

/// Get proactive context predictions (hybrid: pre-generated + on-the-fly + pondering insights)
async fn get_proactive_context(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<String> {
    let project_path_owned = project_path.map(|s| s.to_string());
    let session_id_owned = session_id.map(|s| s.to_string());
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
                    let base_path = Path::new(base);
                    let joined = base_path.join(&p.content);
                    // Canonicalize to resolve .. segments; reject paths escaping project root
                    let ok = joined
                        .canonicalize()
                        .map(|canon| canon.starts_with(base_path.canonicalize().unwrap_or_else(|_| base_path.to_path_buf())))
                        .unwrap_or(false);
                    if !ok {
                        tracing::debug!("Dropping invalid/stale file prediction: {}", p.content);
                    }
                    ok
                }
                _ => true,
            });
        }

        let mut context_lines: Vec<String> = Vec::new();

        // On-the-fly prediction context (file/tool patterns)
        if !predictions.is_empty() {
            let suggestions = predictor::predictions_to_interventions(&predictions, &config);
            context_lines.extend(suggestions.iter().take(2).map(|s| s.to_context_string()));
        }

        // 3. Pondering-based insights (behavior_patterns + documentation interventions)
        let remaining_slots = 2usize.saturating_sub(context_lines.len());
        if remaining_slots > 0
            && let Ok(pending) = crate::proactive::interventions::get_pending_interventions_sync(
                conn, project_id, &config,
            )
        {
            for intervention in pending.iter().take(remaining_slots) {
                context_lines.push(format!("[Insight] {}", intervention.format()));

                // Record that we showed this intervention (for cooldown/dedup/feedback)
                let _ = crate::proactive::interventions::record_intervention_sync(
                    conn,
                    project_id,
                    session_id_owned.as_deref(),
                    intervention,
                );
            }
        }

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

    // Team intelligence: lazy detection + heartbeat + discoveries
    let team_context: Option<String> = get_team_context(&pool, session_id).await;

    // Get relevant context with metadata
    let result = manager
        .get_context_for_message(user_message, session_id)
        .await;

    // Get proactive predictions if enabled
    let session_id_for_proactive = if session_id.is_empty() {
        None
    } else {
        Some(session_id)
    };
    let proactive_context: Option<String> = if let Some(project_id) = project_id {
        get_proactive_context(
            &pool,
            project_id,
            project_path.as_deref(),
            session_id_for_proactive,
        )
        .await
    } else {
        None
    };

    // Get pending native tasks
    let task_context = get_task_context();
    if task_context.is_some() {
        eprintln!("[mira] Added pending task context");
    }

    // Add team context if available
    let mut final_context = result.context.clone();
    if let Some(ref tc) = team_context {
        if final_context.is_empty() {
            final_context = tc.clone();
        } else {
            final_context = format!("{}\n\n{}", final_context, tc);
        }
        eprintln!("[mira] Added team context");
    }

    // Combine reactive context with proactive predictions
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

/// Get team context: lazy detection, heartbeat, and recent team discoveries.
async fn get_team_context(pool: &Arc<DatabasePool>, session_id: &str) -> Option<String> {
    if session_id.is_empty() {
        return None;
    }

    // Read team membership from DB (session-isolated), with filesystem fallback
    let membership = crate::hooks::session::read_team_membership_from_db(pool, session_id).await;
    let membership = match membership {
        Some(m) => m,
        None => {
            // Lazy re-detection: covers lead who creates team after their SessionStart
            let cwd = crate::hooks::session::read_claude_cwd();
            let input = serde_json::json!({});
            let det = crate::hooks::session::detect_team_membership(
                &input,
                Some(session_id),
                cwd.as_deref(),
            )?;

            // Register in DB
            let pool_clone = pool.clone();
            let team_name = det.team_name.clone();
            let config_path = det.config_path.clone();
            let member_name = det.member_name.clone();
            let role = det.role.clone();
            let agent_type = det.agent_type.clone();
            let sid = session_id.to_string();
            let cwd_c = cwd.clone();

            let membership = pool_clone
                .interact(move |conn| {
                    let project_id = cwd_c.as_deref().and_then(|c| {
                        crate::db::get_or_create_project_sync(conn, c, None)
                            .ok()
                            .map(|(id, _)| id)
                    });
                    let tid = crate::db::get_or_create_team_sync(
                        conn,
                        &team_name,
                        project_id,
                        &config_path,
                    )?;
                    crate::db::register_team_session_sync(
                        conn,
                        tid,
                        &sid,
                        &member_name,
                        &role,
                        agent_type.as_deref(),
                    )?;
                    Ok::<_, anyhow::Error>(crate::hooks::session::TeamMembership {
                        team_id: tid,
                        team_name: team_name.clone(),
                        member_name,
                        role,
                        config_path,
                    })
                })
                .await
                .ok()?;

            // Cache for future calls
            let _ = crate::hooks::session::write_team_membership(session_id, &membership);
            eprintln!(
                "[mira] Lazy team detection: {} (team_id: {})",
                membership.team_name, membership.team_id
            );
            membership
        }
    };

    // Heartbeat: update last_heartbeat for this session
    let pool_clone = pool.clone();
    let tid = membership.team_id;
    let sid = session_id.to_string();
    let _ = pool_clone
        .interact(move |conn| {
            crate::db::heartbeat_team_session_sync(conn, tid, &sid)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await;

    // Fetch recent team-scoped memories (last 1 hour, limit 3) as discoveries
    let pool_clone = pool.clone();
    let tid = membership.team_id;
    let team_name = membership.team_name.clone();
    let member_name = membership.member_name.clone();
    let discoveries: Vec<(String, String)> = pool_clone
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT content, COALESCE(category, 'general')
                 FROM memory_facts
                 WHERE scope = 'team' AND team_id = ?1
                   AND COALESCE(updated_at, created_at) > datetime('now', '-1 hour')
                 ORDER BY COALESCE(updated_at, created_at) DESC
                 LIMIT 3",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![tid], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(crate::db::log_and_discard)
                .collect();
            Ok::<_, anyhow::Error>(rows)
        })
        .await
        .unwrap_or_default();

    // Fetch active teammate count
    let pool_clone = pool.clone();
    let tid = membership.team_id;
    let members: Vec<crate::db::TeamMemberInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_active_team_members_sync(conn, tid))
        })
        .await
        .unwrap_or_default();

    // Build team context string
    let mut parts: Vec<String> = Vec::new();

    let other_count = members.len().saturating_sub(1);
    if other_count > 0 {
        let others: Vec<&str> = members
            .iter()
            .filter(|m| m.member_name != member_name)
            .map(|m| m.member_name.as_str())
            .collect();
        parts.push(format!(
            "[Team: {}] You are {} ({} teammate(s) active: {})",
            team_name,
            member_name,
            other_count,
            others.join(", ")
        ));
    }

    if !discoveries.is_empty() {
        let disc_lines: Vec<String> = discoveries
            .iter()
            .map(|(content, cat)| format!("  • [{}] {}", cat, content))
            .collect();
        parts.push(format!(
            "[Team discoveries (last hour)]:\n{}",
            disc_lines.join("\n")
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
