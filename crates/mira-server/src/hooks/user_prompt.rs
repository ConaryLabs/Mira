// crates/mira-server/src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for context injection

use crate::config::EnvConfig;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::{
    get_code_db_path, get_db_path, read_hook_input, resolve_project, write_hook_output,
};
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Maximum number of recent injection hashes to track for cross-prompt dedup
const MAX_INJECTION_HISTORY: usize = 5;

/// Get embeddings client if available (with pool for usage tracking)
fn get_embeddings(pool: Option<Arc<DatabasePool>>) -> Option<Arc<EmbeddingClient>> {
    EmbeddingClient::from_env(pool).map(Arc::new)
}

/// Get pending native tasks as context string
fn get_task_context(project_label: &str) -> Option<String> {
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
            let tag = if project_label.is_empty() {
                "[Mira/tasks]".to_string()
            } else {
                format!("[Mira/tasks ({})]", project_label)
            };
            Some(format!(
                "{} {} pending task(s) ({}/{} completed):\n{}",
                tag,
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

// -- Cross-prompt injection dedup ------------------------------------------------
// Tracks content hashes of recently injected context per session to avoid
// re-injecting identical context on consecutive prompts.

#[derive(Serialize, Deserialize, Default)]
struct InjectionDedupState {
    /// Ring buffer of recent context hashes (u64 FNV/default hasher output)
    recent_hashes: Vec<u64>,
}

fn injection_dedup_path(session_id: &str) -> std::path::PathBuf {
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".mira")
        .join("tmp");
    let sanitized: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let sid = if sanitized.len() > 16 {
        sanitized[..16].to_string()
    } else {
        sanitized
    };
    mira_dir.join(format!("inj_dedup_{}.json", sid))
}

fn load_injection_dedup(session_id: &str) -> InjectionDedupState {
    if session_id.is_empty() {
        return InjectionDedupState::default();
    }
    std::fs::read_to_string(injection_dedup_path(session_id))
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_injection_dedup(session_id: &str, state: &InjectionDedupState) {
    if session_id.is_empty() {
        return;
    }
    let path = injection_dedup_path(session_id);
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("injection dedup: failed to create cache dir: {e}");
        return;
    }
    let Ok(json) = serde_json::to_string(state) else {
        return;
    };
    let tmp_path = path.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(json.as_bytes())
            })
            .is_ok()
            && let Err(e) = std::fs::rename(&tmp_path, &path)
        {
            tracing::warn!("injection dedup: failed to persist cache file: {e}");
        }
    }
    #[cfg(not(unix))]
    {
        if std::fs::write(&tmp_path, &json).is_ok()
            && let Err(e) = std::fs::rename(&tmp_path, &path)
        {
            tracing::warn!("injection dedup: failed to persist cache file: {e}");
        }
    }
}

/// Compute a content hash of context string for dedup comparison.
/// Uses FNV-1a which is stable across compilations, unlike DefaultHasher.
fn context_hash(context: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in context.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Check if this context was recently injected and record it.
/// Returns true if the context is a duplicate (should be suppressed).
fn check_and_record_injection(session_id: &str, context: &str) -> bool {
    if session_id.is_empty() || context.is_empty() {
        return false;
    }
    let hash = context_hash(context);
    let mut state = load_injection_dedup(session_id);

    let is_dup = state.recent_hashes.contains(&hash);

    // Record this hash
    state.recent_hashes.push(hash);
    if state.recent_hashes.len() > MAX_INJECTION_HISTORY {
        state.recent_hashes.remove(0);
    }
    save_injection_dedup(session_id, &state);

    is_dup
}

/// Check for post-compaction state and return a recovery context string.
/// Consumes the flag only after successful extraction so a failed DB lookup
/// doesn't lose the recovery opportunity.
async fn get_post_compaction_recovery(session_id: &str) -> Option<String> {
    if !crate::hooks::precompact::check_post_compaction_flag(session_id) {
        return None;
    }

    tracing::debug!("[mira] Post-compaction recovery: loading saved context");

    let db_path = crate::hooks::get_db_path();
    let pool = Arc::new(
        crate::db::pool::DatabasePool::open_hook(&db_path)
            .await
            .ok()?,
    );

    let sid_owned = session_id.to_string();
    let result = pool
        .interact(move |conn| {
            let snapshot_json: Option<String> = conn
                .query_row(
                    "SELECT snapshot FROM session_snapshots
                     WHERE session_id = ?1
                     ORDER BY created_at DESC LIMIT 1",
                    rusqlite::params![sid_owned],
                    |row| row.get(0),
                )
                .ok();

            let Some(snapshot_json) = snapshot_json else {
                return Ok::<Option<String>, anyhow::Error>(None);
            };

            let snapshot: serde_json::Value = match serde_json::from_str(&snapshot_json) {
                Ok(v) => v,
                Err(_) => return Ok(None),
            };
            let cc_value = match snapshot.get("compaction_context") {
                Some(v) if !v.is_null() => v,
                _ => return Ok(None),
            };
            let ctx: crate::hooks::precompact::CompactionContext =
                match serde_json::from_value(cc_value.clone()) {
                    Ok(c) => c,
                    Err(_) => return Ok(None),
                };

            if ctx.is_empty() {
                return Ok(None);
            }

            let mut lines = Vec::new();
            lines.push(
                "[Mira/recovery] Context was just compacted. Key points from before:".to_string(),
            );

            if let Some(ref intent) = ctx.user_intent {
                lines.push(format!("  Intent: {}", intent));
            }
            for d in ctx.decisions.iter().take(3) {
                lines.push(format!("  Decision: {}", d));
            }
            for w in ctx.active_work.iter().take(2) {
                lines.push(format!("  In progress: {}", w));
            }
            for i in ctx.issues.iter().take(2) {
                lines.push(format!("  Issue: {}", i));
            }
            if !ctx.files_referenced.is_empty() {
                let files: Vec<&str> = ctx
                    .files_referenced
                    .iter()
                    .take(5)
                    .map(|s| s.as_str())
                    .collect();
                lines.push(format!("  Files: {}", files.join(", ")));
            }

            Ok(Some(lines.join("\n")))
        })
        .await
        .ok()
        .flatten();

    // Always consume the flag after attempting recovery
    crate::hooks::precompact::consume_post_compaction_flag(session_id);

    result
}

/// Record a context injection event. Best-effort, never blocks the hook.
/// Uses pool if available (direct path), otherwise opens a direct connection (IPC path).
async fn record_injection(
    pool: Option<&Arc<DatabasePool>>,
    record: crate::db::injection::InjectionRecord,
) {
    if let Some(pool) = pool {
        pool.try_interact("record injection", move |conn| {
            crate::db::injection::insert_injection_sync(conn, &record)?;
            Ok(())
        })
        .await;
    } else {
        let db_path = get_db_path();
        crate::db::injection::record_injection_fire_and_forget(&db_path, &record);
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

    // Check for post-compaction recovery (lightweight flag check)
    let recovery_context = get_post_compaction_recovery(session_id).await;

    // Try IPC first -- single composite call runs everything server-side
    let mut client = crate::ipc::client::HookClient::connect().await;

    if let Some(ctx) = client
        .get_user_prompt_context(user_message, session_id)
        .await
    {
        return assemble_output_from_ipc(ctx, session_id, recovery_context.as_deref()).await;
    }

    // Direct fallback: open local pools and run full logic
    run_direct(user_message, session_id, recovery_context.as_deref()).await
}

/// Assemble hook output from the composite IPC result.
async fn assemble_output_from_ipc(
    ctx: crate::ipc::client::UserPromptContextResult,
    session_id: &str,
    recovery_context: Option<&str>,
) -> Result<()> {
    use crate::context::{
        BudgetEntry, BudgetManager, PRIORITY_REACTIVE, PRIORITY_TASKS, PRIORITY_TEAM,
    };

    let inject_start = std::time::Instant::now();

    // Derive project label from path for context tags
    let project_label = ctx
        .project_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name().and_then(|f| f.to_str()))
        .unwrap_or("");

    // Get pending native tasks (filesystem-only, not served via IPC)
    let task_context = get_task_context(project_label);
    if task_context.is_some() {
        tracing::debug!("[mira] Added pending task context");
    }

    let mut budget_entries: Vec<BudgetEntry> = Vec::new();

    // Post-compaction recovery gets highest priority (10) so it's always included
    if let Some(rc) = recovery_context {
        budget_entries.push(BudgetEntry::new(10.0, rc.to_string(), "recovery"));
        tracing::debug!("[mira] Added post-compaction recovery context");
    }

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

    if let Some(ref tc) = task_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TASKS, tc.clone(), "tasks"));
    }

    let hook_budget = BudgetManager::with_limit(ctx.config_max_chars.max(3000));
    let budget_result = hook_budget.apply_budget_prioritized(budget_entries);
    let final_context = budget_result.content;

    if !final_context.is_empty() {
        // Cross-prompt dedup: suppress if identical context was recently injected
        let was_deduped = check_and_record_injection(session_id, &final_context);
        if was_deduped {
            tracing::debug!("[mira] Context injection suppressed (cross-prompt dedup)");
            record_injection(
                None,
                crate::db::injection::InjectionRecord {
                    hook_name: "UserPromptSubmit".to_string(),
                    session_id: Some(session_id.to_string()),
                    project_id: ctx.project_id,
                    chars_injected: 0,
                    sources_kept: budget_result.kept_sources.clone(),
                    sources_dropped: budget_result.dropped_sources.clone(),
                    latency_ms: Some(inject_start.elapsed().as_millis() as u64),
                    was_deduped: true,
                    was_cached: ctx.reactive_from_cache,
                },
            )
            .await;
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }

        // Record successful injection
        record_injection(
            None,
            crate::db::injection::InjectionRecord {
                hook_name: "UserPromptSubmit".to_string(),
                session_id: Some(session_id.to_string()),
                project_id: ctx.project_id,
                chars_injected: final_context.len(),
                sources_kept: budget_result.kept_sources.clone(),
                sources_dropped: budget_result.dropped_sources.clone(),
                latency_ms: Some(inject_start.elapsed().as_millis() as u64),
                was_deduped: false,
                was_cached: ctx.reactive_from_cache,
            },
        )
        .await;

        let mut output = serde_json::json!({});
        output["metadata"] = serde_json::json!({
            "sources": ctx.reactive_sources,
            "from_cache": ctx.reactive_from_cache,
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
async fn run_direct(
    user_message: &str,
    session_id: &str,
    recovery_context: Option<&str>,
) -> Result<()> {
    let inject_start = std::time::Instant::now();

    // Open database pools (main + code index)
    let db_path = get_db_path();
    let pool = Arc::new(
        DatabasePool::open_hook(std::path::Path::new(&db_path))
            .await
            .context("Failed to open Mira database for UserPromptSubmit")?,
    );
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
    let fuzzy = if env_config.fuzzy_search {
        Some(Arc::new(FuzzyCache::new()))
    } else {
        None
    };
    let manager =
        crate::context::ContextInjectionManager::new(pool.clone(), code_pool, embeddings, fuzzy)
            .await;

    // Resolve project once
    let sid = if session_id.is_empty() {
        None
    } else {
        Some(session_id)
    };
    let (_project_id, _project_path, project_name) = resolve_project(&pool, sid).await;

    // Derive project label for context tags
    let project_label = project_name
        .as_deref()
        .or_else(|| {
            _project_path
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name()?.to_str())
        })
        .unwrap_or("");

    // Team intelligence: lazy detection + heartbeat + discoveries
    let team_context: Option<String> = get_team_context(&pool, session_id).await;

    // Get relevant context with metadata
    let result = manager
        .get_context_for_message(user_message, session_id)
        .await;

    // Get pending native tasks
    let task_context = get_task_context(project_label);
    if task_context.is_some() {
        tracing::debug!("[mira] Added pending task context");
    }

    // Route ALL context through a unified budget with priority scoring.
    use crate::context::{BudgetEntry, PRIORITY_REACTIVE, PRIORITY_TASKS, PRIORITY_TEAM};

    let mut budget_entries: Vec<BudgetEntry> = Vec::new();

    // Post-compaction recovery gets highest priority so it's always included
    if let Some(rc) = recovery_context {
        budget_entries.push(BudgetEntry::new(10.0, rc.to_string(), "recovery"));
        tracing::debug!("[mira] Added post-compaction recovery context");
    }

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

    if let Some(ref tc) = task_context {
        budget_entries.push(BudgetEntry::new(PRIORITY_TASKS, tc.clone(), "tasks"));
    }

    let hook_budget =
        crate::context::BudgetManager::with_limit(manager.config().max_chars.max(3000));
    let budget_result = hook_budget.apply_budget_prioritized(budget_entries);
    let final_context = budget_result.content;

    if !final_context.is_empty() {
        // Cross-prompt dedup: suppress if identical context was recently injected
        let was_deduped = check_and_record_injection(session_id, &final_context);
        if was_deduped {
            tracing::debug!("[mira] Context injection suppressed (cross-prompt dedup)");
            record_injection(
                Some(&pool),
                crate::db::injection::InjectionRecord {
                    hook_name: "UserPromptSubmit".to_string(),
                    session_id: Some(session_id.to_string()),
                    project_id: _project_id,
                    chars_injected: 0,
                    sources_kept: budget_result.kept_sources.clone(),
                    sources_dropped: budget_result.dropped_sources.clone(),
                    latency_ms: Some(inject_start.elapsed().as_millis() as u64),
                    was_deduped: true,
                    was_cached: result.from_cache,
                },
            )
            .await;
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }

        // Record successful injection
        record_injection(
            Some(&pool),
            crate::db::injection::InjectionRecord {
                hook_name: "UserPromptSubmit".to_string(),
                session_id: Some(session_id.to_string()),
                project_id: _project_id,
                chars_injected: final_context.len(),
                sources_kept: budget_result.kept_sources.clone(),
                sources_dropped: budget_result.dropped_sources.clone(),
                latency_ms: Some(inject_start.elapsed().as_millis() as u64),
                was_deduped: false,
                was_cached: result.from_cache,
            },
        )
        .await;

        let mut output = serde_json::json!({});
        output["metadata"] = serde_json::json!({
            "sources": result.sources,
            "from_cache": result.from_cache,
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
            if let Err(e) = crate::hooks::session::write_team_membership(session_id, &membership) {
                tracing::warn!("failed to cache team membership: {e}");
            }
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

// ===============================================================================
// Tests
// ===============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        // Too short (< 50 chars)
        assert!(!check("short"));
        // Too long (> 500 chars)
        assert!(!check(&"x".repeat(501)));
        // Within bounds
        assert!(check(
            "how does the authentication module work in this project?"
        ));
    }

    // -- Cross-prompt injection dedup ------------------------------------------------

    #[test]
    fn injection_dedup_first_time_not_duplicate() {
        let sid = format!("test_dedup_{}", std::process::id());
        let _ = std::fs::remove_file(injection_dedup_path(&sid));

        let is_dup = check_and_record_injection(&sid, "some unique context");
        assert!(!is_dup, "First injection should not be a duplicate");

        let _ = std::fs::remove_file(injection_dedup_path(&sid));
    }

    #[test]
    fn injection_dedup_detects_repeat() {
        let sid = format!("test_dedup_rep_{}", std::process::id());
        let _ = std::fs::remove_file(injection_dedup_path(&sid));

        let context = "repeated context string for testing";
        let _ = check_and_record_injection(&sid, context);
        let is_dup = check_and_record_injection(&sid, context);
        assert!(
            is_dup,
            "Second identical injection should be detected as duplicate"
        );

        let _ = std::fs::remove_file(injection_dedup_path(&sid));
    }

    #[test]
    fn injection_dedup_different_context_not_duplicate() {
        let sid = format!("test_dedup_diff_{}", std::process::id());
        let _ = std::fs::remove_file(injection_dedup_path(&sid));

        let _ = check_and_record_injection(&sid, "context A");
        let is_dup = check_and_record_injection(&sid, "context B");
        assert!(!is_dup, "Different context should not be a duplicate");

        let _ = std::fs::remove_file(injection_dedup_path(&sid));
    }

    #[test]
    fn injection_dedup_empty_session_or_context() {
        assert!(!check_and_record_injection("", "some context"));
        assert!(!check_and_record_injection("some_session", ""));
    }

    #[test]
    fn injection_dedup_evicts_old_hashes() {
        let sid = format!("test_dedup_evict_{}", std::process::id());
        let _ = std::fs::remove_file(injection_dedup_path(&sid));

        // Fill beyond MAX_INJECTION_HISTORY with unique contexts
        for i in 0..MAX_INJECTION_HISTORY + 2 {
            let _ = check_and_record_injection(&sid, &format!("unique context {}", i));
        }

        // The oldest context should have been evicted
        let is_dup = check_and_record_injection(&sid, "unique context 0");
        assert!(
            !is_dup,
            "Evicted context should not be detected as duplicate"
        );

        let _ = std::fs::remove_file(injection_dedup_path(&sid));
    }

    #[test]
    fn context_hash_deterministic() {
        let h1 = context_hash("same content");
        let h2 = context_hash("same content");
        assert_eq!(h1, h2);

        let h3 = context_hash("different content");
        assert_ne!(h1, h3);
    }
}
