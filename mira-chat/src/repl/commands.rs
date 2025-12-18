//! Slash command handlers for the REPL
//!
//! Handles /help, /clear, /status, /switch, /remember, /recall, /tasks, /compact, etc.

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::context::MiraContext;
use crate::responses::Client;
use crate::semantic::SemanticSearch;
use crate::session::SessionManager;

/// Command handler with access to REPL state
pub struct CommandHandler<'a> {
    pub context: &'a mut MiraContext,
    pub previous_response_id: &'a mut Option<String>,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Arc<SemanticSearch>,
    pub session: &'a Option<Arc<SessionManager>>,
    pub client: &'a Option<Client>,
    pub start_time: Instant,
}

impl<'a> CommandHandler<'a> {
    /// Handle a slash command, returns true if handled
    pub async fn handle(&mut self, cmd: &str) -> Result<bool> {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");

        match command {
            "/version" => {
                println!("Mira Chat v{}", env!("CARGO_PKG_VERSION"));
                println!("  Model: GPT-5.2 Thinking");
                println!("  Backend: Mira Power Suit");
            }
            "/uptime" => {
                let elapsed = self.start_time.elapsed();
                println!("Uptime: {}", format_duration(elapsed));
            }
            "/help" => {
                println!("Commands:");
                println!("  /help              - Show this help");
                println!("  /version           - Show version info");
                println!("  /uptime            - Show session uptime");
                println!("  /clear             - Clear conversation history");
                println!("  /compact           - Compact code context");
                println!("  /context           - Show current Mira context");
                println!("  /status            - Show current state");
                println!("  /switch [path]     - Switch project");
                println!("  /remember <text>   - Store in memory");
                println!("  /recall <query>    - Search memory");
                println!("  /tasks             - List tasks");
                println!("  /usage [args]       - Token/caching analytics (try: /usage, /usage last 20, /usage day, /usage effort)");
                println!("  /quit              - Exit");
            }
            "/clear" => {
                *self.previous_response_id = None;
                if let Some(session) = self.session {
                    if let Err(e) = session.clear_conversation().await {
                        println!("Warning: failed to clear session: {}", e);
                    }
                }
                println!("Conversation cleared.");
            }
            "/compact" => {
                self.cmd_compact().await;
            }
            "/context" => {
                let ctx = self.context.as_system_prompt();
                if ctx.is_empty() {
                    println!("No context loaded.");
                } else {
                    println!("{}", ctx);
                }
            }
            "/status" => {
                self.cmd_status().await;
            }
            "/switch" => {
                self.cmd_switch(arg).await;
            }
            "/remember" => {
                if arg.is_empty() {
                    println!("Usage: /remember <text to remember>");
                } else {
                    self.cmd_remember(arg).await;
                }
            }
            "/recall" => {
                if arg.is_empty() {
                    println!("Usage: /recall <search query>");
                } else {
                    self.cmd_recall(arg).await;
                }
            }
            "/tasks" => {
                self.cmd_tasks().await;
            }
            "/usage" => {
                self.cmd_usage(arg).await;
            }
            "/quit" | "/exit" => {
                std::process::exit(0);
            }
            _ => {
                println!("Unknown command: {}. Try /help", command);
            }
        }
        Ok(true)
    }

    /// /status - Show current state
    async fn cmd_status(&self) {
        println!(
            "Project: {}",
            self.context.project_path.as_deref().unwrap_or("(none)")
        );
        println!(
            "Semantic search: {}",
            if self.semantic.is_available() {
                "enabled"
            } else {
                "disabled"
            }
        );

        // Show session info
        if let Some(session) = self.session {
            if let Ok(stats) = session.stats().await {
                println!(
                    "Session: {} messages, {} summaries{}{}",
                    stats.total_messages,
                    stats.summary_count,
                    if stats.has_active_conversation {
                        ", active"
                    } else {
                        ""
                    },
                    if stats.has_code_compaction {
                        ", has compaction"
                    } else {
                        ""
                    }
                );
            }
        } else {
            println!("Session: disabled");
        }

        if let Some(db) = self.db {
            // Count goals
            let goals: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM goals WHERE status = 'in_progress'")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));
            println!("Active goals: {}", goals.0);

            // Count tasks
            let tasks: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE status != 'completed'")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));
            println!("Pending tasks: {}", tasks.0);

            // Count memories
            let memories: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_facts")
                .fetch_one(db)
                .await
                .unwrap_or((0,));
            println!("Memories: {}", memories.0);
        }
    }

    /// /compact - Compact code context into encrypted blob
    async fn cmd_compact(&self) {
        let session = match self.session {
            Some(s) => s,
            None => {
                println!("Session not available for compaction.");
                return;
            }
        };

        let client = match self.client {
            Some(c) => c,
            None => {
                println!("API client not available.");
                return;
            }
        };

        // Get touched files
        let files = session.get_touched_files();
        if files.is_empty() {
            println!("No files touched in this session.");
            return;
        }

        // Get response ID for compaction
        let response_id = match session.get_response_id().await {
            Ok(Some(id)) => id,
            _ => {
                println!("No active conversation to compact.");
                return;
            }
        };

        println!("Compacting {} file(s)...", files.len());
        for f in &files {
            println!("  - {}", f);
        }

        // Build context description
        let context = format!(
            "Code context for project. Files touched: {}",
            files.join(", ")
        );

        // Call compaction endpoint
        match client.compact(&response_id, &context).await {
            Ok(response) => {
                // Store the compaction blob
                if let Err(e) = session
                    .store_compaction(&response.encrypted_content, &files)
                    .await
                {
                    println!("Failed to store compaction: {}", e);
                    return;
                }

                // Clear touched files
                session.clear_touched_files();

                let saved = response.tokens_saved.unwrap_or(0);
                println!("Compacted! {} tokens saved.", saved);
            }
            Err(e) => {
                println!("Compaction failed: {}", e);
            }
        }
    }

    /// /switch - Change project
    async fn cmd_switch(&mut self, path: &str) {
        let new_path = if path.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        } else {
            path.to_string()
        };

        // Reload context for new project
        if let Some(db) = self.db {
            match crate::context::MiraContext::load(db, &new_path).await {
                Ok(ctx) => {
                    *self.context = ctx;
                    println!("Switched to: {}", new_path);
                    println!(
                        "  {} corrections, {} goals, {} memories",
                        self.context.corrections.len(),
                        self.context.goals.len(),
                        self.context.memories.len()
                    );
                }
                Err(e) => {
                    println!("Failed to load context: {}", e);
                    self.context.project_path = Some(new_path.clone());
                    println!("Switched to: {} (no context)", new_path);
                }
            }
        } else {
            self.context.project_path = Some(new_path.clone());
            println!("Switched to: {} (no database)", new_path);
        }

        // Clear conversation on project switch
        *self.previous_response_id = None;
    }

    /// /remember - Store in memory
    async fn cmd_remember(&self, content: &str) {
        use chrono::Utc;
        use uuid::Uuid;

        if let Some(db) = self.db {
            let now = Utc::now().timestamp();
            let id = Uuid::new_v4().to_string();
            let key: String = content
                .chars()
                .take(50)
                .collect::<String>()
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
                .trim()
                .to_string();

            let result = sqlx::query(
                r#"
                INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
                VALUES ($1, 'general', $2, $3, NULL, 'mira-chat', 1.0, 0, $4, $4)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
            "#,
            )
            .bind(&id)
            .bind(&key)
            .bind(content)
            .bind(now)
            .execute(db)
            .await;

            match result {
                Ok(_) => {
                    println!(
                        "Remembered: \"{}\"",
                        if content.len() > 50 {
                            &content[..50]
                        } else {
                            content
                        }
                    );

                    // Also store in Qdrant if available
                    if self.semantic.is_available() {
                        use std::collections::HashMap;
                        let mut metadata = HashMap::new();
                        metadata.insert("fact_type".into(), serde_json::json!("general"));
                        metadata.insert("key".into(), serde_json::json!(key));

                        if let Err(e) = self
                            .semantic
                            .store(crate::semantic::COLLECTION_MEMORY, &id, content, metadata)
                            .await
                        {
                            println!("  (semantic index failed: {})", e);
                        }
                    }
                }
                Err(e) => println!("Failed to remember: {}", e),
            }
        } else {
            println!("No database connected.");
        }
    }

    /// /recall - Search memory
    async fn cmd_recall(&self, query: &str) {
        // Try semantic search first
        if self.semantic.is_available() {
            match self
                .semantic
                .search(crate::semantic::COLLECTION_MEMORY, query, 5, None)
                .await
            {
                Ok(results) if !results.is_empty() => {
                    println!("Found {} memories (semantic):", results.len());
                    for (i, r) in results.iter().enumerate() {
                        let preview = if r.content.len() > 80 {
                            format!("{}...", &r.content[..80])
                        } else {
                            r.content.clone()
                        };
                        println!("  {}. [score: {:.2}] {}", i + 1, r.score, preview);
                    }
                    return;
                }
                Ok(_) => {} // Fall through to text search
                Err(e) => {
                    println!("  (semantic search failed: {}, trying text...)", e);
                }
            }
        }

        // Fallback to text search
        if let Some(db) = self.db {
            let pattern = format!("%{}%", query);
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT key, value FROM memory_facts WHERE value LIKE $1 OR key LIKE $1 ORDER BY times_used DESC LIMIT 5"
            )
            .bind(&pattern)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if rows.is_empty() {
                println!("No memories found for: {}", query);
            } else {
                println!("Found {} memories (text):", rows.len());
                for (i, (key, value)) in rows.iter().enumerate() {
                    let preview = if value.len() > 80 {
                        format!("{}...", &value[..80])
                    } else {
                        value.clone()
                    };
                    println!("  {}. [{}] {}", i + 1, key, preview);
                }
            }
        } else {
            println!("No database connected.");
        }
    }

    /// /tasks - List tasks
    async fn cmd_tasks(&self) {
        if let Some(db) = self.db {
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT title, status, priority FROM tasks WHERE status != 'completed' ORDER BY
                 CASE priority WHEN 'urgent' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                 created_at DESC LIMIT 10",
            )
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if rows.is_empty() {
                println!("No pending tasks.");
            } else {
                println!("Tasks ({}):", rows.len());
                for (title, status, priority) in rows {
                    let icon = match status.as_str() {
                        "in_progress" => "◐",
                        "blocked" => "✗",
                        _ => "○",
                    };
                    println!("  {} [{}] {}", icon, priority, title);
                }
            }
        } else {
            println!("No database connected.");
        }
    }

    /// /usage - Token and caching analytics
    /// Subcommands: /usage, /usage last N, /usage day, /usage effort, /usage totals, /usage spikes
    async fn cmd_usage(&self, arg: &str) {
        let Some(db) = self.db else {
            println!("No database connected.");
            return;
        };

        let parts: Vec<&str> = arg.split_whitespace().collect();
        let subcmd = parts.first().copied().unwrap_or("last");

        match subcmd {
            "day" | "daily" => self.usage_daily(db).await,
            "effort" => self.usage_by_effort(db).await,
            "totals" | "total" => self.usage_totals(db).await,
            "spikes" | "spike" => self.usage_spikes(db).await,
            "last" | _ => {
                let limit: i32 = parts.get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(10);
                self.usage_last(db, limit).await;
            }
        }
    }

    /// Show last N usage records with chain + tools + spike flags
    async fn usage_last(&self, db: &SqlitePool, limit: i32) {
        let rows: Vec<(i64, String, i32, i32, i32, i32, Option<String>, i32, Option<String>)> = sqlx::query_as(
            r#"
            SELECT u.created_at, u.reasoning_effort, u.input_tokens, u.output_tokens,
                   u.reasoning_tokens, u.cached_tokens, u.previous_response_id,
                   COALESCE(u.tool_count, 0), u.tool_names
            FROM chat_usage u
            ORDER BY u.created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            println!("No usage data recorded.");
            return;
        }

        println!("┌─────────────────────┬────────┬───────┬──────────┬─────────┬─────────┬────────┬───────┐");
        println!("│ Time                │ Effort │ Chain │ Input    │ Output  │ Cached  │ Cache% │ Flags │");
        println!("├─────────────────────┼────────┼───────┼──────────┼─────────┼─────────┼────────┼───────┤");

        let rows_vec: Vec<_> = rows.into_iter().rev().collect();
        let mut prev_input: Option<i32> = None;

        for (i, (ts, effort, input, output, _reasoning, cached, prev_resp_id, tool_count, tool_names)) in rows_vec.iter().enumerate() {
            let cache_pct = if *input > 0 {
                (*cached as f64 / *input as f64) * 100.0
            } else {
                0.0
            };

            // Format timestamp
            let dt = chrono::DateTime::from_timestamp(*ts, 0)
                .map(|d| d.format("%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "?".to_string());

            // Format effort (truncate to 6 chars)
            let eff: String = effort.chars().take(6).collect();

            // Chain indicator: NEW or last 4 chars of previous_response_id
            let chain = match prev_resp_id {
                None => "NEW".to_string(),
                Some(id) => format!("…{}", id.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>()),
            };

            // Build flags
            let mut flags = String::new();

            // Input spike: >50% jump from previous
            if let Some(prev) = prev_input {
                if prev > 0 && *input > prev + (prev / 2) {
                    flags.push('!');
                }
            }

            // Cache drop: below 50%
            if cache_pct < 50.0 && *input > 1000 {
                flags.push('C');
            }

            // Tool burst: 3+ tools
            if *tool_count >= 3 {
                flags.push('T');
            }

            // Chain reset (N = NEW chain)
            if prev_resp_id.is_none() && i > 0 {
                flags.push('N');
            }

            // Tool info for tooltip-style display
            let tools_hint = if *tool_count > 0 {
                tool_names.as_ref().map(|n| format!(" [{}]", n)).unwrap_or_default()
            } else {
                String::new()
            };

            println!(
                "│ {:19} │ {:6} │ {:5} │ {:>8} │ {:>7} │ {:>7} │ {:>5.1}% │ {:5} │{}",
                dt, eff, chain,
                format_tokens(*input), format_tokens(*output),
                format_tokens(*cached), cache_pct,
                flags, tools_hint
            );

            prev_input = Some(*input);
        }
        println!("└─────────────────────┴────────┴───────┴──────────┴─────────┴─────────┴────────┴───────┘");

        // Show quick totals with cost estimate
        let total_input: i64 = rows_vec.iter().map(|r| r.2 as i64).sum();
        let total_output: i64 = rows_vec.iter().map(|r| r.3 as i64).sum();
        let total_cached: i64 = rows_vec.iter().map(|r| r.5 as i64).sum();
        let overall_cache = if total_input > 0 {
            (total_cached as f64 / total_input as f64) * 100.0
        } else {
            0.0
        };

        // Cost estimate
        let input_cost = (total_input as f64 / 1_000_000.0) * 2.50;
        let output_cost = (total_output as f64 / 1_000_000.0) * 10.0;
        let cached_savings = (total_cached as f64 / 1_000_000.0) * 2.50 * 0.5;

        println!();
        println!("  {} rows │ ↓{} in │ ↑{} out │ ⚡{} cached ({:.0}%) │ ~${:.2}",
            rows_vec.len(), format_tokens(total_input as i32),
            format_tokens(total_output as i32), format_tokens(total_cached as i32),
            overall_cache, input_cost + output_cost - cached_savings
        );
        println!("  Flags: ! input spike, C cache<50%, T tools≥3, N new chain");
    }

    /// Show only turns with spikes/anomalies
    async fn usage_spikes(&self, db: &SqlitePool) {
        let rows: Vec<(i64, String, i32, i32, i32, i32, Option<String>, i32, Option<String>)> = sqlx::query_as(
            r#"
            SELECT u.created_at, u.reasoning_effort, u.input_tokens, u.output_tokens,
                   u.reasoning_tokens, u.cached_tokens, u.previous_response_id,
                   COALESCE(u.tool_count, 0), u.tool_names
            FROM chat_usage u
            ORDER BY u.created_at DESC
            LIMIT 50
            "#,
        )
        .fetch_all(db)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            println!("No usage data recorded.");
            return;
        }

        println!("Spike Report (anomalies in last 50 turns)");
        println!("──────────────────────────────────────────");

        let rows_vec: Vec<_> = rows.into_iter().rev().collect();
        let mut prev_input: Option<i32> = None;
        let mut spike_count = 0;

        for (i, (ts, effort, input, output, _reasoning, cached, prev_resp_id, tool_count, tool_names)) in rows_vec.iter().enumerate() {
            let cache_pct = if *input > 0 {
                (*cached as f64 / *input as f64) * 100.0
            } else {
                0.0
            };

            let mut reasons: Vec<String> = Vec::new();

            // Input spike
            if let Some(prev) = prev_input {
                if prev > 0 && *input > prev + (prev / 2) {
                    let pct = ((*input - prev) as f64 / prev as f64) * 100.0;
                    reasons.push(format!("Input +{:.0}% ({} → {})", pct, format_tokens(prev), format_tokens(*input)));
                }
            }

            // Cache crash
            if cache_pct < 50.0 && *input > 5000 {
                if prev_resp_id.is_none() && i > 0 {
                    reasons.push(format!("Cache {:.0}% (chain reset)", cache_pct));
                } else {
                    reasons.push(format!("Cache {:.0}% (prefix changed?)", cache_pct));
                }
            }

            // Tool burst
            if *tool_count >= 5 {
                let names = tool_names.as_ref().map(|n| n.as_str()).unwrap_or("?");
                reasons.push(format!("Tool burst: {} calls [{}]", tool_count, names));
            }

            // Chain reset (not first row)
            if prev_resp_id.is_none() && i > 0 {
                reasons.push("Chain reset (new conversation)".to_string());
            }

            if !reasons.is_empty() {
                spike_count += 1;
                let dt = chrono::DateTime::from_timestamp(*ts, 0)
                    .map(|d| d.format("%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "?".to_string());

                println!();
                println!("  {} │ {} │ ↓{} ↑{} ⚡{:.0}%",
                    dt, effort, format_tokens(*input), format_tokens(*output), cache_pct);
                for reason in reasons {
                    println!("    → {}", reason);
                }
            }

            prev_input = Some(*input);
        }

        if spike_count == 0 {
            println!("  No spikes detected. Everything looks normal.");
        } else {
            println!();
            println!("  {} spike(s) found", spike_count);
        }
    }

    /// Show daily totals
    async fn usage_daily(&self, db: &SqlitePool) {
        let rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
            r#"
            SELECT date(created_at, 'unixepoch') as day,
                   SUM(input_tokens), SUM(output_tokens),
                   SUM(reasoning_tokens), SUM(cached_tokens)
            FROM chat_usage
            GROUP BY day
            ORDER BY day DESC
            LIMIT 14
            "#,
        )
        .fetch_all(db)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            println!("No usage data recorded.");
            return;
        }

        println!("┌────────────┬───────────┬──────────┬───────────┬──────────┬────────┐");
        println!("│ Date       │ Input     │ Output   │ Reasoning │ Cached   │ Cache% │");
        println!("├────────────┼───────────┼──────────┼───────────┼──────────┼────────┤");

        for (day, input, output, reasoning, cached) in rows.iter().rev() {
            let cache_pct = if *input > 0 {
                (*cached as f64 / *input as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "│ {:10} │ {:>9} │ {:>8} │ {:>9} │ {:>8} │ {:>5.1}% │",
                day,
                format_tokens(*input as i32), format_tokens(*output as i32),
                format_tokens(*reasoning as i32), format_tokens(*cached as i32),
                cache_pct
            );
        }
        println!("└────────────┴───────────┴──────────┴───────────┴──────────┴────────┘");
    }

    /// Show usage by effort level
    async fn usage_by_effort(&self, db: &SqlitePool) {
        let rows: Vec<(String, i64, i64, i64, i64, i64)> = sqlx::query_as(
            r#"
            SELECT reasoning_effort, COUNT(*), SUM(input_tokens), SUM(output_tokens),
                   SUM(reasoning_tokens), SUM(cached_tokens)
            FROM chat_usage
            GROUP BY reasoning_effort
            ORDER BY SUM(input_tokens) DESC
            "#,
        )
        .fetch_all(db)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            println!("No usage data recorded.");
            return;
        }

        println!("┌────────┬───────┬───────────┬──────────┬───────────┬──────────┬────────┐");
        println!("│ Effort │ Count │ Input     │ Output   │ Reasoning │ Cached   │ Cache% │");
        println!("├────────┼───────┼───────────┼──────────┼───────────┼──────────┼────────┤");

        for (effort, count, input, output, reasoning, cached) in &rows {
            let cache_pct = if *input > 0 {
                (*cached as f64 / *input as f64) * 100.0
            } else {
                0.0
            };
            let eff: String = effort.chars().take(6).collect();
            println!(
                "│ {:6} │ {:>5} │ {:>9} │ {:>8} │ {:>9} │ {:>8} │ {:>5.1}% │",
                eff, count,
                format_tokens(*input as i32), format_tokens(*output as i32),
                format_tokens(*reasoning as i32), format_tokens(*cached as i32),
                cache_pct
            );
        }
        println!("└────────┴───────┴───────────┴──────────┴───────────┴──────────┴────────┘");
    }

    /// Show all-time totals
    async fn usage_totals(&self, db: &SqlitePool) {
        let row: Option<(i64, i64, i64, i64, i64)> = sqlx::query_as(
            r#"
            SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
                   SUM(reasoning_tokens), SUM(cached_tokens)
            FROM chat_usage
            "#,
        )
        .fetch_optional(db)
        .await
        .unwrap_or(None);

        if let Some((count, input, output, reasoning, cached)) = row {
            let cache_pct = if input > 0 {
                (cached as f64 / input as f64) * 100.0
            } else {
                0.0
            };

            println!("Token Usage (All Time)");
            println!("──────────────────────");
            println!("  Messages:  {}", count);
            println!("  Input:     {} tokens", format_tokens(input as i32));
            println!("  Output:    {} tokens", format_tokens(output as i32));
            println!("  Reasoning: {} tokens", format_tokens(reasoning as i32));
            println!("  Cached:    {} tokens ({:.1}%)", format_tokens(cached as i32), cache_pct);
            println!();

            // Estimate cost (rough: $2.50/1M input, $10/1M output for GPT-4 class)
            let input_cost = (input as f64 / 1_000_000.0) * 2.50;
            let output_cost = (output as f64 / 1_000_000.0) * 10.0;
            let cached_savings = (cached as f64 / 1_000_000.0) * 2.50 * 0.5; // 50% discount
            println!("  Est. cost: ${:.2} (saved ~${:.2} from cache)",
                input_cost + output_cost - cached_savings, cached_savings);
        } else {
            println!("No usage data recorded.");
        }
    }
}

/// Format token count with k/M suffix
fn format_tokens(n: i32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a duration in human-readable form
pub fn format_duration(d: Duration) -> String {
    let mut secs = d.as_secs();

    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3_600;
    secs %= 3_600;
    let mins = secs / 60;
    secs %= 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}
