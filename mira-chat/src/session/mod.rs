//! Invisible session management
//!
//! Provides seamless context across restarts with no explicit session boundaries.
//! Context is assembled per-query from multiple sources:
//! - Recent messages (sliding window)
//! - Semantic recall (relevant past CONVERSATION context - NOT code)
//! - Mira context (corrections, goals, memories)
//! - Code compaction blobs (preserved code understanding metadata)
//! - Rolling summaries (compressed older context)
//!
//! IMPORTANT: Code is ALWAYS read fresh from disk. We don't store or recall
//! old code content. Semantic search is for conversation/memory only.
//! Code compaction stores understanding metadata, not code itself.

mod types;

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::context::MiraContext;
use crate::semantic::SemanticSearch;

pub use types::{
    AssembledContext, ChatMessage, CodeIndexFileHint, CodeIndexSymbolHint, SemanticHit, SessionStats,
};

/// Number of recent messages to keep raw in context (full fidelity)
const RECENT_RAW_COUNT: usize = 5;

/// Batch size for summarization (summarize this many at once)
const SUMMARIZE_BATCH_SIZE: usize = 5;

/// Message count threshold to trigger summarization (RECENT_RAW_COUNT + SUMMARIZE_BATCH_SIZE)
const SUMMARIZE_THRESHOLD: usize = 10;

/// Minimum similarity score for semantic recall (unused but kept for potential future use)
const _RECALL_THRESHOLD: f32 = 0.65;

/// Number of semantic results to fetch (unused but kept for potential future use)
const _RECALL_LIMIT: usize = 5;

/// Number of level-1 summaries before meta-summarization
const META_SUMMARY_THRESHOLD: usize = 10;

/// Max summaries to load into context (keeps prompt size bounded)
const MAX_SUMMARIES_IN_CONTEXT: usize = 5;

/// Collection name for chat messages
const COLLECTION_CHAT: &str = "mira_chat_messages";

/// Session manager for invisible persistence
pub struct SessionManager {
    db: SqlitePool,
    semantic: Arc<SemanticSearch>,
    project_path: String,
    /// Files touched during this session (for compaction context)
    touched_files: std::sync::RwLock<Vec<String>>,
}

impl SessionManager {
    /// Create a new session manager
    pub async fn new(
        db: SqlitePool,
        semantic: Arc<SemanticSearch>,
        project_path: String,
    ) -> Result<Self> {
        // Ensure chat collection exists
        if semantic.is_available() {
            if let Err(e) = semantic.ensure_collection(COLLECTION_CHAT).await {
                warn!("Failed to create chat collection: {}", e);
            }
        }

        // Ensure chat_context row exists for this project
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO chat_context (project_path, created_at, updated_at)
            VALUES ($1, $2, $2)
            "#,
        )
        .bind(&project_path)
        .bind(Utc::now().timestamp())
        .execute(&db)
        .await?;

        Ok(Self {
            db,
            semantic,
            project_path,
            touched_files: std::sync::RwLock::new(Vec::new()),
        })
    }

    /// Record a file as touched (read or written)
    pub fn track_file(&self, path: &str) {
        if let Ok(mut files) = self.touched_files.write() {
            if !files.contains(&path.to_string()) {
                files.push(path.to_string());
            }
        }
    }

    /// Get list of touched files
    pub fn get_touched_files(&self) -> Vec<String> {
        self.touched_files
            .read()
            .map(|f| f.clone())
            .unwrap_or_default()
    }

    /// Clear touched files list
    pub fn clear_touched_files(&self) {
        if let Ok(mut files) = self.touched_files.write() {
            files.clear();
        }
    }

    /// Save a message and update context
    pub async fn save_message(&self, role: &str, content: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        // Save to SQLite
        sqlx::query(
            r#"
            INSERT INTO chat_messages (id, role, blocks, created_at)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(&id)
        .bind(role)
        .bind(serde_json::json!([{"type": "text", "content": content}]).to_string())
        .bind(now)
        .execute(&self.db)
        .await?;

        // Update message count
        sqlx::query(
            r#"
            UPDATE chat_context
            SET total_messages = total_messages + 1, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(now)
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        // Save to Qdrant for semantic search (async, don't block on failure)
        if self.semantic.is_available() {
            let semantic = Arc::clone(&self.semantic);
            let id_clone = id.clone();
            let content_clone = content.to_string();
            let role_clone = role.to_string();
            let project = self.project_path.clone();

            tokio::spawn(async move {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("role".into(), serde_json::json!(role_clone));
                metadata.insert("project".into(), serde_json::json!(project));
                metadata.insert("created_at".into(), serde_json::json!(now));

                if let Err(e) = semantic
                    .store(COLLECTION_CHAT, &id_clone, &content_clone, metadata)
                    .await
                {
                    debug!("Failed to store message embedding: {}", e);
                }
            });
        }

        // Check if we need to summarize
        self.maybe_summarize().await?;

        Ok(id)
    }

    /// Update the previous response ID
    pub async fn set_response_id(&self, response_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = $1, updated_at = $2
            WHERE project_path = $3
            "#,
        )
        .bind(response_id)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Clear the previous response ID and prepare handoff for next turn
    /// Use this when token usage is too high to reset server-side accumulation
    /// The handoff blob preserves continuity so the reset isn't obvious
    pub async fn clear_response_id_with_handoff(&self) -> Result<()> {
        // Build handoff blob BEFORE clearing (captures current state)
        let handoff = self.build_handoff_blob().await?;

        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = NULL, needs_handoff = 1, handoff_blob = $1, updated_at = $2
            WHERE project_path = $3
            "#,
        )
        .bind(&handoff)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Clear the previous response ID (no handoff - hard reset)
    pub async fn clear_response_id(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = NULL, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Check if next request needs handoff context
    pub async fn needs_handoff(&self) -> Result<bool> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT needs_handoff FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|(v,)| v != 0).unwrap_or(false))
    }

    /// Get the handoff blob (if any) and clear the flag
    pub async fn consume_handoff(&self) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT handoff_blob FROM chat_context WHERE project_path = $1 AND needs_handoff = 1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        if let Some((blob,)) = row {
            // Clear the handoff flag
            sqlx::query(
                "UPDATE chat_context SET needs_handoff = 0, handoff_blob = NULL, updated_at = $1 WHERE project_path = $2",
            )
            .bind(Utc::now().timestamp())
            .bind(&self.project_path)
            .execute(&self.db)
            .await?;
            Ok(blob)
        } else {
            Ok(None)
        }
    }

    /// Build a compact handoff blob for continuity after reset
    /// This captures: recent turns, current plan/decisions, persona reminders
    async fn build_handoff_blob(&self) -> Result<String> {
        let mut sections = Vec::new();

        // 1. Recent conversation (last 3-4 turns for immediate context)
        let recent: Vec<(String, String, i64)> = sqlx::query_as(
            r#"
            SELECT role, blocks, created_at FROM chat_messages
            WHERE archived_at IS NULL
            ORDER BY created_at DESC
            LIMIT 6
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        if !recent.is_empty() {
            let mut convo_lines = vec!["## Recent Conversation".to_string()];
            for (role, blocks_json, _) in recent.into_iter().rev() {
                // Extract just text content from blocks
                if let Ok(blocks) = serde_json::from_str::<Vec<serde_json::Value>>(&blocks_json) {
                    for block in blocks {
                        if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                            // Truncate long messages (use chars to avoid UTF-8 boundary panic)
                            let text = if content.chars().count() > 500 {
                                format!("{}...", content.chars().take(500).collect::<String>())
                            } else {
                                content.to_string()
                            };
                            convo_lines.push(format!("**{}**: {}", role, text));
                        }
                    }
                }
            }
            if convo_lines.len() > 1 {
                sections.push(convo_lines.join("\n"));
            }
        }

        // 2. Latest summary (captures older context)
        let summary: Option<(String,)> = sqlx::query_as(
            "SELECT summary FROM chat_summaries WHERE project_path = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        if let Some((summary_text,)) = summary {
            sections.push(format!("## Earlier Context (Summary)\n{}", summary_text));
        }

        // 3. Active goals/tasks
        let goals: Vec<(String, String, i32)> = sqlx::query_as(
            r#"
            SELECT title, status, progress_percent FROM goals
            WHERE project_id = (SELECT id FROM projects WHERE path = $1)
              AND status IN ('planning', 'in_progress', 'blocked')
            ORDER BY updated_at DESC LIMIT 3
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        if !goals.is_empty() {
            let mut goal_lines = vec!["## Active Goals".to_string()];
            for (title, status, progress) in goals {
                goal_lines.push(format!("- {} [{}] ({}%)", title, status, progress));
            }
            sections.push(goal_lines.join("\n"));
        }

        // 4. Key decisions (recent ones that might be relevant)
        let decisions: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT value FROM memory_facts
            WHERE (project_id = (SELECT id FROM projects WHERE path = $1) OR project_id IS NULL)
              AND fact_type = 'decision'
            ORDER BY updated_at DESC LIMIT 5
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        if !decisions.is_empty() {
            let mut dec_lines = vec!["## Recent Decisions".to_string()];
            for (decision,) in decisions {
                dec_lines.push(format!("- {}", decision));
            }
            sections.push(dec_lines.join("\n"));
        }

        // 5. Continuity note
        sections.push(
            "## Continuity Note\nThis is a continuation of an ongoing conversation. \
             The context above summarizes where we left off. Continue naturally without \
             re-introducing yourself or asking what we're working on."
                .to_string(),
        );

        Ok(format!(
            "# Handoff Context (Thread Reset)\n\n{}",
            sections.join("\n\n")
        ))
    }

    /// Get the previous response ID
    pub async fn get_response_id(&self) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT last_response_id FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.and_then(|(id,)| id))
    }

    /// Assemble context for a new query
    pub async fn assemble_context(&self, query: &str) -> Result<AssembledContext> {
        let mut ctx = AssembledContext::default();

        // 1. Load recent messages (raw, full fidelity)
        ctx.recent_messages = self.load_recent_messages(RECENT_RAW_COUNT).await?;

        // 2. Load Mira context (corrections, goals, memories)
        ctx.mira_context = MiraContext::load(&self.db, &self.project_path).await?;

        // 3. Load rolling summaries
        ctx.summaries = self.load_summaries(MAX_SUMMARIES_IN_CONTEXT).await?;

        // 4. Load code compaction blob
        ctx.code_compaction = self.load_code_compaction().await?;

        // 5. Get previous response ID
        ctx.previous_response_id = self.get_response_id().await?;

        // 6. Semantic recall - relevant past conversation context
        // Added at END of prompt to preserve prefix caching for stable content
        if self.semantic.is_available() {
            if let Ok(hits) = self.semantic_recall(query).await {
                ctx.semantic_context = hits;
            }
        }

        // 7. Code index hints - relevant symbols from codebase
        // Added at END of prompt to preserve prefix caching for stable content
        ctx.code_index_hints = self.load_code_index_hints(query).await;

        Ok(ctx)
    }

    async fn load_code_index_hints(&self, query: &str) -> Vec<CodeIndexFileHint> {
        match self.load_code_index_hints_inner(query).await {
            Ok(hints) => hints,
            Err(e) => {
                // This is an optional enhancement. If the DB doesn't have the code tables,
                // we just skip it.
                debug!("Code index hints unavailable: {}", e);
                Vec::new()
            }
        }
    }

    async fn load_code_index_hints_inner(&self, query: &str) -> Result<Vec<CodeIndexFileHint>> {
        use std::collections::{HashMap, HashSet};

        let terms = extract_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        // Indexer stores absolute file paths; keep results scoped to the current project.
        let project_prefix = format!("{}%", self.project_path);

        let mut files: HashMap<String, Vec<CodeIndexSymbolHint>> = HashMap::new();
        let mut seen: HashMap<String, HashSet<(String, i64)>> = HashMap::new();

        // Pull a small number of hits per term; merge/dedup across terms.
        for term in terms.iter().take(6) {
            let like = format!("%{}%", term);

            let rows = sqlx::query(
                r#"
                SELECT file_path, name, qualified_name, symbol_type, signature, start_line, end_line
                FROM code_symbols
                WHERE file_path LIKE $1
                  AND (name LIKE $2 OR qualified_name LIKE $2)
                ORDER BY analyzed_at DESC
                LIMIT 50
                "#,
            )
            .bind(&project_prefix)
            .bind(&like)
            .fetch_all(&self.db)
            .await?;

            for row in rows {
                let file_path: String = row.get("file_path");
                let name: String = row.get("name");
                let start_line: i64 = row.get("start_line");

                let entry_seen = seen.entry(file_path.clone()).or_default();
                if entry_seen.contains(&(name.clone(), start_line)) {
                    continue;
                }
                entry_seen.insert((name.clone(), start_line));

                let hint = CodeIndexSymbolHint {
                    name,
                    qualified_name: row.get("qualified_name"),
                    symbol_type: row.get("symbol_type"),
                    signature: row.get("signature"),
                    start_line,
                    end_line: row.get("end_line"),
                };

                files.entry(file_path).or_default().push(hint);
            }

            // Stop early if we already have enough breadth.
            if files.len() >= 8 {
                break;
            }
        }

        if files.is_empty() {
            return Ok(Vec::new());
        }

        // Convert to a ranked list: most hits per file first.
        let mut file_list: Vec<CodeIndexFileHint> = files
            .into_iter()
            .map(|(file_path, mut symbols)| {
                symbols.truncate(8);
                CodeIndexFileHint { file_path, symbols }
            })
            .collect();

        file_list.sort_by_key(|f| std::cmp::Reverse(f.symbols.len()));
        file_list.truncate(6);

        Ok(file_list)
    }

    /// Load recent messages
    async fn load_recent_messages(&self, limit: usize) -> Result<Vec<ChatMessage>> {
        let rows = sqlx::query(
            r#"
            SELECT id, role, blocks, created_at
            FROM chat_messages
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await?;

        let mut messages: Vec<ChatMessage> = rows
            .into_iter()
            .filter_map(|row| {
                let id: String = row.get("id");
                let role: String = row.get("role");
                let blocks_json: String = row.get("blocks");
                let created_at: i64 = row.get("created_at");

                // Extract text content from blocks
                let blocks: Vec<serde_json::Value> = serde_json::from_str(&blocks_json).ok()?;
                let content = blocks
                    .iter()
                    .filter_map(|b| b.get("content")?.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                Some(ChatMessage {
                    id,
                    role,
                    content,
                    created_at,
                })
            })
            .collect();

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    /// Semantic recall of relevant past CONVERSATION context (not code!)
    /// Scoped to current project
    async fn semantic_recall(&self, query: &str) -> Result<Vec<SemanticHit>> {
        use qdrant_client::qdrant::{Condition, Filter};

        // Filter to only this project's messages
        let filter = Filter::must([Condition::matches("project", self.project_path.clone())]);

        let results = self
            .semantic
            .search(COLLECTION_CHAT, query, _RECALL_LIMIT, Some(filter))
            .await?;

        Ok(results
            .into_iter()
            .filter(|r| r.score >= _RECALL_THRESHOLD)
            .map(|r| SemanticHit {
                content: r.content,
                score: r.score,
                role: r
                    .metadata
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                created_at: r.metadata.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
            })
            .collect())
    }

    /// Load rolling summaries with tiered support
    /// Prioritizes meta-summaries (level 2) over regular summaries (level 1)
    async fn load_summaries(&self, limit: usize) -> Result<Vec<String>> {
        // First get any meta-summaries (level 2)
        let meta: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT summary FROM chat_summaries
            WHERE project_path = $1 AND level = 2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        let mut summaries: Vec<String> = meta.into_iter().map(|(s,)| s).collect();
        let remaining = limit.saturating_sub(summaries.len());

        // Then get recent level-1 summaries
        if remaining > 0 {
            let recent: Vec<(String,)> = sqlx::query_as(
                r#"
                SELECT summary FROM chat_summaries
                WHERE project_path = $1 AND level = 1
                ORDER BY created_at DESC
                LIMIT $2
                "#,
            )
            .bind(&self.project_path)
            .bind(remaining as i64)
            .fetch_all(&self.db)
            .await?;

            summaries.extend(recent.into_iter().map(|(s,)| s));
        }

        Ok(summaries)
    }

    /// Check if meta-summarization is needed (too many level-1 summaries)
    pub async fn check_meta_summarization_needed(&self) -> Result<Option<Vec<(String, String)>>> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_summaries WHERE project_path = $1 AND level = 1",
        )
        .bind(&self.project_path)
        .fetch_one(&self.db)
        .await?;

        if (count.0 as usize) < META_SUMMARY_THRESHOLD {
            return Ok(None);
        }

        info!(
            "Meta-summarization needed: {} level-1 summaries to compress",
            count.0
        );

        // Get oldest level-1 summaries to compress
        let rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT id, summary FROM chat_summaries
            WHERE project_path = $1 AND level = 1
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(&self.project_path)
        .bind(META_SUMMARY_THRESHOLD as i64)
        .fetch_all(&self.db)
        .await?;

        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows))
        }
    }

    /// Store a meta-summary (level 2) and delete the summarized level-1 summaries
    pub async fn store_meta_summary(&self, summary: &str, summary_ids: &[String]) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        // Store the meta-summary
        sqlx::query(
            r#"
            INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, level, created_at)
            VALUES ($1, $2, $3, $4, $5, 2, $6)
            "#,
        )
        .bind(&id)
        .bind(&self.project_path)
        .bind(summary)
        .bind(serde_json::to_string(summary_ids)?)
        .bind(summary_ids.len() as i64)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Delete the old level-1 summaries
        for sum_id in summary_ids {
            sqlx::query("DELETE FROM chat_summaries WHERE id = $1")
                .bind(sum_id)
                .execute(&self.db)
                .await?;
        }

        info!(
            "Stored meta-summary, deleted {} level-1 summaries",
            summary_ids.len()
        );
        Ok(())
    }

    /// Load the most recent code compaction blob
    async fn load_code_compaction(&self) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT encrypted_content FROM code_compaction
            WHERE project_path = $1
              AND (expires_at IS NULL OR expires_at > $2)
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(&self.project_path)
        .bind(Utc::now().timestamp())
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|(c,)| c))
    }

    /// Check if summarization is needed
    /// Returns messages to summarize if threshold exceeded
    pub async fn check_summarization_needed(&self) -> Result<Option<Vec<ChatMessage>>> {
        // Count total messages
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chat_messages")
            .fetch_one(&self.db)
            .await?;

        if count.0 as usize <= SUMMARIZE_THRESHOLD {
            return Ok(None);
        }

        // Get oldest messages outside the recent window (to be summarized)
        let to_summarize_count = count.0 as usize - RECENT_RAW_COUNT;
        if to_summarize_count < SUMMARIZE_BATCH_SIZE {
            return Ok(None);
        }

        info!(
            "Summarization needed: {} messages to compress (batch of {})",
            to_summarize_count, SUMMARIZE_BATCH_SIZE
        );

        // Fetch the oldest messages that will be summarized
        let rows = sqlx::query(
            r#"
            SELECT id, role, blocks, created_at
            FROM chat_messages
            ORDER BY created_at ASC
            LIMIT $1
            "#,
        )
        .bind(to_summarize_count as i64)
        .fetch_all(&self.db)
        .await?;

        let messages: Vec<ChatMessage> = rows
            .into_iter()
            .filter_map(|row| {
                let id: String = row.get("id");
                let role: String = row.get("role");
                let blocks_json: String = row.get("blocks");
                let created_at: i64 = row.get("created_at");

                let blocks: Vec<serde_json::Value> = serde_json::from_str(&blocks_json).ok()?;
                let content = blocks
                    .iter()
                    .filter_map(|b| b.get("content")?.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                Some(ChatMessage {
                    id,
                    role,
                    content,
                    created_at,
                })
            })
            .collect();

        if messages.is_empty() {
            Ok(None)
        } else {
            Ok(Some(messages))
        }
    }

    /// Store a summary and archive the summarized messages (no longer deletes!)
    pub async fn store_summary(&self, summary: &str, message_ids: &[String]) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        // Store the summary
        sqlx::query(
            r#"
            INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&id)
        .bind(&self.project_path)
        .bind(summary)
        .bind(serde_json::to_string(message_ids)?)
        .bind(message_ids.len() as i64)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Archive the old messages (don't delete - preserve for recall)
        for msg_id in message_ids {
            sqlx::query(
                "UPDATE chat_messages SET archived_at = $1, summary_id = $2 WHERE id = $3",
            )
            .bind(now)
            .bind(&id)
            .bind(msg_id)
            .execute(&self.db)
            .await?;
        }

        // Update message count (only active, non-archived)
        let remaining: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_messages WHERE archived_at IS NULL",
        )
        .fetch_one(&self.db)
        .await?;

        sqlx::query(
            "UPDATE chat_context SET total_messages = $1, updated_at = $2 WHERE project_path = $3",
        )
        .bind(remaining.0)
        .bind(now)
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        info!("Stored summary, archived {} old messages", message_ids.len());
        Ok(())
    }

    /// Store a per-turn summary (doesn't delete messages - just adds summary)
    /// Used for immediate turn summarization in fresh-chain-per-turn mode
    pub async fn store_turn_summary(&self, summary: &str) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO chat_summaries (id, project_path, summary, message_ids, message_count, level, created_at)
            VALUES ($1, $2, $3, '[]', 1, 1, $4)
            "#,
        )
        .bind(&id)
        .bind(&self.project_path)
        .bind(summary)
        .bind(now)
        .execute(&self.db)
        .await?;

        debug!("Stored turn summary");
        Ok(())
    }

    /// Check if we need to summarize (called after saving message)
    async fn maybe_summarize(&self) -> Result<()> {
        // This is now a no-op - summarization is handled explicitly by the caller
        // after checking check_summarization_needed()
        Ok(())
    }

    /// Store a code compaction blob
    pub async fn store_compaction(
        &self,
        encrypted_content: &str,
        files: &[String],
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO code_compaction (id, project_path, encrypted_content, files_included, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(&id)
        .bind(&self.project_path)
        .bind(encrypted_content)
        .bind(serde_json::to_string(files)?)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Update context
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_compaction_id = $1, updated_at = $2
            WHERE project_path = $3
            "#,
        )
        .bind(&id)
        .bind(now)
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        Ok(id)
    }

    /// Clear conversation (but keep memories and summaries)
    pub async fn clear_conversation(&self) -> Result<()> {
        sqlx::query("DELETE FROM chat_messages")
            .execute(&self.db)
            .await?;

        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = NULL, total_messages = 0, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get conversation stats
    pub async fn stats(&self) -> Result<SessionStats> {
        let ctx: Option<(Option<String>, i64)> = sqlx::query_as(
            "SELECT last_response_id, total_messages FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        let (has_response, total_messages) = ctx
            .map(|(r, m)| (r.is_some(), m as usize))
            .unwrap_or((false, 0));

        let summaries: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_summaries WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_one(&self.db)
        .await?;

        let has_compaction: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM code_compaction
            WHERE project_path = $1
              AND (expires_at IS NULL OR expires_at > $2)
            "#,
        )
        .bind(&self.project_path)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.db)
        .await?;

        Ok(SessionStats {
            total_messages,
            summary_count: summaries.0 as usize,
            has_active_conversation: has_response,
            has_code_compaction: has_compaction.0 > 0,
        })
    }
}

impl AssembledContext {
    /// Format context for injection into system prompt
    ///
    /// IMPORTANT: Order is optimized for LLM caching (prefix matching).
    /// Static/stable content comes FIRST for maximum cache hits:
    ///   1. Mira context (corrections, goals, memories) - stable per session
    ///   2. Code compaction blob - stable between compactions
    ///   3. Summaries - stable between batch summarizations
    ///   4. Semantic context - changes per query
    ///   5. Code index hints - changes per query
    ///   6. Recent messages (raw) - changes every turn (LEAST cacheable)
    pub fn format_for_prompt(&self) -> String {
        let mut sections = Vec::new();

        // 1. Mira context (corrections, goals, memories) - MOST STABLE
        // These rarely change within a session
        let mira = self.mira_context.as_system_prompt();
        if !mira.is_empty() {
            sections.push(mira);
        }

        // 2. Code compaction blob - stable between compactions
        // This is an opaque encrypted blob from OpenAI that preserves code understanding
        if let Some(ref blob) = self.code_compaction {
            sections.push(format!(
                "## Code Context (Compacted)\n<compacted_context>{}</compacted_context>",
                blob
            ));
        }

        // 3. Summaries - stable between summarizations
        if !self.summaries.is_empty() {
            let mut summary_section = String::from("## Previous Context (Summarized)\n");
            for (i, s) in self.summaries.iter().enumerate() {
                summary_section.push_str(&format!("{}. {}\n", i + 1, s));
            }
            sections.push(summary_section);
        }

        // 4. Semantic context - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant past conversation snippets based on current query
        if !self.semantic_context.is_empty() {
            let mut semantic_section = String::from("## Relevant Past Context\n");
            for hit in &self.semantic_context {
                let preview = if hit.content.len() > 200 {
                    // Find valid char boundary near 200
                    let mut end = 200;
                    while !hit.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &hit.content[..end])
                } else {
                    hit.content.clone()
                };
                semantic_section.push_str(&format!("- [{}] {}\n", hit.role, preview));
            }
            sections.push(semantic_section);
        }

        // 5. Code index hints - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant symbols from the codebase based on current query
        if !self.code_index_hints.is_empty() {
            let mut code_section = String::from("## Relevant Code Locations\n");
            for hint in &self.code_index_hints {
                code_section.push_str(&format!("**{}**\n", hint.file_path));
                for sym in &hint.symbols {
                    let sig = sym.signature.as_deref().unwrap_or("");
                    if sig.is_empty() {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{})\n",
                            sym.symbol_type, sym.name, sym.start_line
                        ));
                    } else {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{}): {}\n",
                            sym.symbol_type, sym.name, sym.start_line, sig
                        ));
                    }
                }
            }
            sections.push(code_section);
        }

        // 6. Recent messages (raw) - CHANGES EVERY TURN (at very end)
        // Full fidelity for the most recent conversation turns
        if !self.recent_messages.is_empty() {
            let mut recent_section = String::from("## Recent Conversation\n");
            for msg in &self.recent_messages {
                let role_label = if msg.role == "user" { "User" } else { "Assistant" };
                // Truncate long messages for context efficiency
                let content = if msg.content.len() > 500 {
                    let mut end = 500;
                    while !msg.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                recent_section.push_str(&format!("**{}**: {}\n\n", role_label, content));
            }
            sections.push(recent_section);
        }

        if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        }
    }

    /// Format recent messages as conversation history
    pub fn format_conversation_history(&self) -> String {
        if self.recent_messages.is_empty() {
            return String::new();
        }

        let mut history = String::from("## Recent Conversation\n");
        for msg in &self.recent_messages {
            let role_label = if msg.role == "user" {
                "User"
            } else {
                "Assistant"
            };
            // Use chars to avoid UTF-8 boundary panic
            let preview = if msg.content.chars().count() > 500 {
                format!("{}...", msg.content.chars().take(500).collect::<String>())
            } else {
                msg.content.clone()
            };
            history.push_str(&format!("**{}**: {}\n\n", role_label, preview));
        }
        history
    }
}

fn extract_terms(query: &str) -> Vec<String> {
    use std::collections::HashSet;

    let mut cleaned = String::with_capacity(query.len());
    for c in query.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cleaned.push(c.to_ascii_lowercase());
        } else {
            cleaned.push(' ');
        }
    }

    // Super light filtering. We just want a handful of useful tokens.
    let noise: HashSet<&'static str> = [
        "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "with", "without",
        "this", "that", "these", "those", "it", "is", "are", "be", "was", "were",
        "use", "using", "used", "make", "makes", "making", "do", "does", "did",
        "how", "what", "where", "why", "when",
        "file", "files", "function", "functions", "struct", "structs", "class", "classes",
        "module", "crate", "rust", "code",
    ]
    .into_iter()
    .collect();

    let mut uniq: HashSet<String> = HashSet::new();
    for raw in cleaned.split_whitespace() {
        if raw.len() < 3 {
            continue;
        }
        if noise.contains(raw) {
            continue;
        }
        if raw.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        uniq.insert(raw.to_string());
    }

    let mut terms: Vec<String> = uniq.into_iter().collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    terms.truncate(8);
    terms
}
