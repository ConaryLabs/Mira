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
