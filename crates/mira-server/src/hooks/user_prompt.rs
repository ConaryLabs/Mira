// crates/mira-server/src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for proactive context injection

use crate::config::EnvConfig;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::{
    get_code_db_path, get_db_path, read_hook_input, resolve_project, write_hook_output,
};
use crate::proactive::behavior::BehaviorTracker;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

/// Get embeddings client if available (with pool for usage tracking)
fn get_embeddings(pool: Option<Arc<DatabasePool>>) -> Option<Arc<EmbeddingClient>> {
    EmbeddingClient::from_env(pool).map(Arc::new)
}

/// Log user query for behavior tracking
pub(crate) async fn log_behavior(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    session_id: &str,
    message: &str,
) {
    let session_id_clone = session_id.to_string();
    let message_clone = message.to_string();
    pool.try_interact("behavior logging", move |conn| {
        let mut tracker = BehaviorTracker::for_session(conn, session_id_clone, project_id);
        let _ = tracker.log_query(conn, &message_clone, "user_prompt");
        Ok(())
    })
    .await;
}

/// Get proactive context from pondering-based insights and pre-generated suggestions
pub(crate) async fn get_proactive_context(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    _project_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<String> {
    let session_id_owned = session_id.map(|s| s.to_string());
    pool.interact(move |conn| {
        let config = crate::proactive::get_proactive_config(conn, None, project_id);

        if !config.enabled {
            return Ok::<Option<String>, anyhow::Error>(None);
        }

        // Pondering-based insights (behavior_patterns + documentation interventions)
        let pending = crate::proactive::interventions::get_pending_interventions_sync(
            conn, project_id, &config,
        )
        .unwrap_or_default();

        let mut context_lines: Vec<String> = Vec::new();
        for intervention in pending.iter().take(2) {
            context_lines.push(format!("[Mira/insight] {}", intervention.format()));

            // Record that we showed this intervention (for cooldown/dedup/feedback)
            let _ = crate::proactive::interventions::record_intervention_sync(
                conn,
                project_id,
                session_id_owned.as_deref(),
                intervention,
            );
        }

        // Pre-generated proactive suggestions (from background pattern mining / LLM)
        // Budget: surface up to 2 suggestions if we have room (max 4 total proactive lines)
        let suggestion_budget = 4_usize.saturating_sub(context_lines.len()).min(2);
        if suggestion_budget > 0 {
            // Use empty trigger_key for general/session-level suggestions
            if let Ok(suggestions) =
                crate::proactive::background::get_pre_generated_suggestions(conn, project_id, "")
            {
                for (text, confidence) in suggestions.iter().take(suggestion_budget) {
                    context_lines.push(format!(
                        "[Mira/suggestion] ({:.0}%) {}",
                        confidence * 100.0,
                        text
                    ));
                }
                // Mark shown for feedback tracking
                if !suggestions.is_empty() {
                    let _ =
                        crate::proactive::background::mark_suggestion_shown(conn, project_id, "");
                }
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
            let completed = total.saturating_sub(pending.len());
            Some(format!(
                "[Mira/tasks] {} pending task(s) ({}/{} completed):\n{}",
                pending.len(),
                completed,
                total,
                lines.join("\n")
            ))
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("[mira] Failed to read native tasks: {}", e);
            None
        }
    }
}

/// Run UserPromptSubmit hook
pub async fn run() -> Result<()> {
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;

    let user_message = input
        .get("prompt")
        .or_else(|| input.get("user_message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session_id = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    tracing::debug!(
        "[mira] UserPromptSubmit hook triggered (session: {}, message length: {})",
        truncate_at_boundary(session_id, 8),
        user_message.len()
    );
    tracing::debug!(
        "[mira] Hook input keys: {:?}",
        input.as_object().map(|obj| obj.keys().collect::<Vec<_>>())
    );

    // Try IPC first — single composite call runs everything server-side
    let mut client = crate::ipc::client::HookClient::connect().await;

    if let Some(ctx) = client
        .get_user_prompt_context(user_message, session_id)
        .await
    {
        return assemble_output_from_ipc(ctx);
    }

    // Direct fallback: open local pools and run full logic
    run_direct(user_message, session_id).await
}

/// Assemble hook output from the composite IPC result.
fn assemble_output_from_ipc(ctx: crate::ipc::client::UserPromptContextResult) -> Result<()> {
    use crate::context::{
        BudgetEntry, BudgetManager, PRIORITY_CROSS_PROJECT, PRIORITY_PROACTIVE, PRIORITY_REACTIVE,
        PRIORITY_TASKS, PRIORITY_TEAM,
    };

    // Get pending native tasks (filesystem-only, not served via IPC)
    let task_context = get_task_context();
    if task_context.is_some() {
        tracing::debug!("[mira] Added pending task context");
    }

    let mut budget_entries: Vec<BudgetEntry> = Vec::new();

    if !ctx.reactive_context.is_empty() {
        budget_entries.push(BudgetEntry::new(
            PRIORITY_REACTIVE,
            ctx.reactive_context.clone(),
            "reactive",
        ));
        if !ctx.reactive_summary.is_empty() {
            tracing::debug!("[mira] {}", ctx.reactive_summary);
        }
    }

    if let Some(ref tc) = ctx.team_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TEAM, tc.clone(), "team"));
        tracing::debug!("[mira] Added team context");
    }

    let has_proactive = match ctx.proactive_context {
        Some(ref pc) if !pc.is_empty() => {
            budget_entries.push(BudgetEntry::new(
                PRIORITY_PROACTIVE,
                pc.clone(),
                "proactive",
            ));
            tracing::debug!("[mira] Added proactive context suggestions");
            true
        }
        _ => false,
    };

    if let Some(ref cpc) = ctx.cross_project_context {
        budget_entries.push(BudgetEntry::new(
            PRIORITY_CROSS_PROJECT,
            cpc.clone(),
            "cross-project",
        ));
        tracing::debug!("[mira] Added cross-project context");
    }

    if let Some(ref tc) = task_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TASKS, tc.clone(), "tasks"));
    }

    let hook_budget = BudgetManager::with_limit(ctx.config_max_chars.saturating_mul(2).max(3000));
    let final_context = hook_budget.apply_budget_prioritized(budget_entries);

    if !final_context.is_empty() {
        let mut output = serde_json::json!({});
        output["metadata"] = serde_json::json!({
            "sources": ctx.reactive_sources,
            "from_cache": ctx.reactive_from_cache,
            "has_proactive": has_proactive
        });
        output["hookSpecificOutput"] = serde_json::json!({
            "hookEventName": "UserPromptSubmit",
            "additionalContext": final_context
        });
        write_hook_output(&output);
    } else {
        if let Some(reason) = &ctx.reactive_skip_reason {
            tracing::debug!("[mira] Context injection skipped: {}", reason);
        }
        write_hook_output(&serde_json::json!({}));
    }

    Ok(())
}

/// Direct-pool fallback when IPC is unavailable.
async fn run_direct(user_message: &str, session_id: &str) -> Result<()> {
    // Open database pools (main + code index)
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open_hook(std::path::Path::new(&db_path)).await?);
    let code_db_path = get_code_db_path();
    let code_pool = if code_db_path.exists() {
        match DatabasePool::open_code_db(&code_db_path).await {
            Ok(cp) => Some(Arc::new(cp)),
            Err(e) => {
                tracing::warn!("[mira] Failed to open code database: {}", e);
                None
            }
        }
    } else {
        None
    };
    let env_config = EnvConfig::load();
    let embeddings = get_embeddings(Some(pool.clone()));
    let embeddings_for_cross_project = embeddings.clone();
    let fuzzy = if env_config.fuzzy_search {
        Some(Arc::new(FuzzyCache::new()))
    } else {
        None
    };
    let manager =
        crate::context::ContextInjectionManager::new(pool.clone(), code_pool, embeddings, fuzzy)
            .await;

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

    // Get proactive predictions if enabled.
    let session_id_for_proactive = if session_id.is_empty() {
        None
    } else {
        Some(session_id)
    };
    let proactive_context: Option<String> = if let Some(project_id) = project_id {
        if crate::context::is_simple_command(user_message) {
            tracing::debug!("[mira] Proactive context skipped: simple command");
            None
        } else {
            let config = manager.config();
            let msg_len = user_message.trim().len();
            if msg_len < config.min_message_len || msg_len > config.max_message_len {
                tracing::debug!("[mira] Proactive context skipped: message length out of bounds");
                None
            } else {
                get_proactive_context(
                    &pool,
                    project_id,
                    project_path.as_deref(),
                    session_id_for_proactive,
                )
                .await
            }
        }
    } else {
        None
    };

    // Cross-project knowledge: search memories from other projects
    let cross_project_context: Option<String> = if let Some(project_id) = project_id {
        if crate::context::is_simple_command(user_message) {
            None
        } else {
            get_cross_project_context(
                &pool,
                &embeddings_for_cross_project,
                project_id,
                user_message,
            )
            .await
        }
    } else {
        None
    };

    // Get pending native tasks
    let task_context = get_task_context();
    if task_context.is_some() {
        tracing::debug!("[mira] Added pending task context");
    }

    // Route ALL context through a unified budget with priority scoring.
    use crate::context::{
        BudgetEntry, PRIORITY_CROSS_PROJECT, PRIORITY_PROACTIVE, PRIORITY_REACTIVE, PRIORITY_TASKS,
        PRIORITY_TEAM,
    };

    let mut budget_entries: Vec<BudgetEntry> = Vec::new();

    if !result.context.is_empty() {
        budget_entries.push(BudgetEntry::new(
            PRIORITY_REACTIVE,
            result.context.clone(),
            "reactive",
        ));
        tracing::debug!("[mira] {}", result.summary());
    }

    if let Some(ref tc) = team_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TEAM, tc.clone(), "team"));
        tracing::debug!("[mira] Added team context");
    }

    let has_proactive = match proactive_context {
        Some(ref pc) if !pc.is_empty() => {
            budget_entries.push(BudgetEntry::new(
                PRIORITY_PROACTIVE,
                pc.clone(),
                "proactive",
            ));
            tracing::debug!("[mira] Added proactive context suggestions");
            true
        }
        _ => false,
    };

    if let Some(ref cpc) = cross_project_context {
        budget_entries.push(BudgetEntry::new(
            PRIORITY_CROSS_PROJECT,
            cpc.clone(),
            "cross-project",
        ));
        tracing::debug!("[mira] Added cross-project context");
    }

    if let Some(ref tc) = task_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TASKS, tc.clone(), "tasks"));
    }

    let hook_budget = crate::context::BudgetManager::with_limit(
        manager.config().max_chars.saturating_mul(2).max(3000),
    );
    let final_context = hook_budget.apply_budget_prioritized(budget_entries);

    if !final_context.is_empty() {
        let mut output = serde_json::json!({});
        output["metadata"] = serde_json::json!({
            "sources": result.sources,
            "from_cache": result.from_cache,
            "has_proactive": has_proactive
        });
        output["hookSpecificOutput"] = serde_json::json!({
            "hookEventName": "UserPromptSubmit",
            "additionalContext": final_context
        });
        write_hook_output(&output);
    } else {
        if let Some(reason) = &result.skip_reason {
            tracing::debug!("[mira] Context injection skipped: {}", reason);
        }
        write_hook_output(&serde_json::json!({}));
    }

    Ok(())
}

/// Get team context: lazy detection, heartbeat, and recent team discoveries.
pub(crate) async fn get_team_context(pool: &Arc<DatabasePool>, session_id: &str) -> Option<String> {
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

            // Validate that the detected config file actually exists and is recent
            // to avoid registering phantom membership from leftover .agent-team.json files
            let config_path = Path::new(&det.config_path);
            if !config_path.is_file() {
                tracing::debug!(
                    "Lazy team detection skipped: config file does not exist: {}",
                    det.config_path
                );
                return None;
            }
            if let Ok(metadata) = config_path.metadata()
                && let Ok(modified) = metadata.modified()
            {
                let age = modified.elapsed().unwrap_or_default();
                if age > std::time::Duration::from_secs(24 * 60 * 60) {
                    tracing::debug!(
                        "Lazy team detection skipped: stale config file ({:.0?} old): {}",
                        age,
                        det.config_path
                    );
                    return None;
                }
            }

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
            tracing::info!(
                "[mira] Lazy team detection: {} (team_id: {})",
                membership.team_name,
                membership.team_id
            );
            membership
        }
    };

    // Heartbeat: update last_heartbeat for this session
    let tid = membership.team_id;
    let sid = session_id.to_string();
    pool.try_interact("team heartbeat", move |conn| {
        crate::db::heartbeat_team_session_sync(conn, tid, &sid)?;
        Ok(())
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
                 FROM system_observations
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
            "[Mira/team] {} - You are {} ({} teammate(s) active: {})",
            team_name,
            member_name,
            other_count,
            others.join(", ")
        ));
    }

    if !discoveries.is_empty() {
        let disc_lines: Vec<String> = discoveries
            .iter()
            .map(|(content, cat)| format!("  - [{}] {}", cat, content))
            .collect();
        parts.push(format!(
            "[Mira/team] Discoveries (last hour):\n{}",
            disc_lines.join("\n")
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

/// Get cross-project knowledge: search memories from other projects.
///
/// First tries tight "you solved this" matching (decisions/patterns, distance < 0.25),
/// then falls back to general cross-project recall (distance < 0.35).
/// Returns formatted context string or None if no relevant matches.
pub(crate) async fn get_cross_project_context(
    pool: &Arc<DatabasePool>,
    embeddings: &Option<Arc<EmbeddingClient>>,
    project_id: i64,
    message: &str,
) -> Option<String> {
    let embeddings = embeddings.as_ref()?;
    let query_embedding = embeddings.embed(message).await.ok()?;
    let embedding_bytes = crate::search::embedding_to_bytes(&query_embedding);

    let pool_clone = pool.clone();
    pool_clone
        .interact(move |conn| {
            // First: tight match for "You solved this in Project X"
            let solved = crate::db::find_solved_in_other_project_sync(
                conn,
                &embedding_bytes,
                project_id,
                0.25,
                2,
            )
            .unwrap_or_default();

            if !solved.is_empty() {
                return Ok::<Option<String>, anyhow::Error>(Some(
                    crate::db::format_cross_project_context(&solved),
                ));
            }

            // Fallback: general cross-project recall with looser threshold
            let results =
                crate::db::recall_cross_project_sync(conn, &embedding_bytes, project_id, 5)
                    .unwrap_or_default();

            // Filter to only reasonably relevant results
            let relevant: Vec<_> = results.into_iter().filter(|r| r.distance < 0.35).collect();

            if relevant.is_empty() {
                return Ok(None);
            }

            Ok(Some(crate::db::format_cross_project_context(&relevant)))
        })
        .await
        .ok()
        .flatten()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::*;

    #[tokio::test]
    async fn log_behavior_inserts_event() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Seed a session so behavior tracker can reference it
        pool.interact(move |conn| {
            seed_session(conn, "sess-behav", project_id, "active");
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        // Log a user query via log_behavior
        log_behavior(&pool, project_id, "sess-behav", "how do I add auth?").await;

        // Verify session_behavior_log table has an entry
        let count: i64 = pool
            .interact(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = 'sess-behav'",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(count > 0, "expected at least one behavior log entry");
    }

    #[tokio::test]
    async fn log_behavior_no_session_does_not_panic() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        // Log behavior without seeding a session — should not panic
        log_behavior(&pool, project_id, "nonexistent-sess", "test query").await;
    }

    #[test]
    fn simple_command_detects_cli_commands() {
        use crate::context::is_simple_command;
        assert!(is_simple_command("git status"));
        assert!(is_simple_command("cargo build"));
        assert!(is_simple_command("ls -la"));
        assert!(is_simple_command("/commit"));
        assert!(is_simple_command("https://example.com"));
        assert!(is_simple_command("ok"));
        // File paths
        assert!(is_simple_command("src/main.rs"));
        // Claude Code questions
        assert!(is_simple_command("how do i use claude code to commit?"));
    }

    #[test]
    fn simple_command_allows_real_queries() {
        use crate::context::is_simple_command;
        assert!(!is_simple_command(
            "how does the authentication module work?"
        ));
        assert!(!is_simple_command("refactor the database connection pool"));
    }

    #[test]
    fn message_length_bounds_from_config() {
        use crate::context::InjectionConfig;
        let config = InjectionConfig::default();
        let check = |msg: &str| {
            let len = msg.trim().len();
            len >= config.min_message_len && len <= config.max_message_len
        };
        // Too short (< 30 chars)
        assert!(!check("short"));
        // Too long (> 500 chars)
        assert!(!check(&"x".repeat(501)));
        // Within bounds
        assert!(check(
            "how does the authentication module work in this project?"
        ));
    }
}
