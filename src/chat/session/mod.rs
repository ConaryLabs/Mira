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

#![allow(dead_code)] // Session infrastructure (some items for future use)

mod anti_amnesia;
mod chain;
mod code_hints;
mod compaction;
mod context;
mod errors;
mod freshness;
pub mod git_tracker;
mod graph;
mod messages;
mod summarization;
mod types;

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::chat::context::MiraContext;
use crate::core::SemanticSearch;

pub use context::ContextBudget;
pub use types::{
    AssembledContext, ChatMessage, Checkpoint, CodeIndexFileHint, CodeIndexSymbolHint,
    SessionStats,
};

/// Number of recent messages to keep raw in context (full fidelity)
const RECENT_RAW_COUNT: usize = 5;

/// Batch size for summarization (summarize this many at once)
const SUMMARIZE_BATCH_SIZE: usize = 5;

/// Message count threshold to trigger summarization (RECENT_RAW_COUNT + SUMMARIZE_BATCH_SIZE)
const SUMMARIZE_THRESHOLD: usize = 10;

/// Number of level-1 summaries before meta-summarization
const META_SUMMARY_THRESHOLD: usize = 10;

/// Max summaries to load into context (keeps prompt size bounded)
const MAX_SUMMARIES_IN_CONTEXT: usize = 5;

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
            if let Err(e) = semantic.ensure_collection(messages::COLLECTION_CHAT).await {
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
                    .store(messages::COLLECTION_CHAT, &id_clone, &content_clone, metadata)
                    .await
                {
                    debug!("Failed to store message embedding: {}", e);
                }
            });
        }

        Ok(id)
    }

    /// Assemble context for a new query
    ///
    /// If a handoff is pending (after chain reset), uses the handoff blob instead
    /// of normal context to avoid duplication and maintain continuity.
    pub async fn assemble_context(&self, query: &str) -> Result<AssembledContext> {
        let mut ctx = AssembledContext::default();

        // Check if we have a pending handoff (after chain reset)
        if let Ok(Some(handoff_blob)) = self.consume_handoff().await {
            debug!("Injecting handoff context (chain was reset)");

            // Handoff mode: use the handoff blob instead of normal context
            ctx.summaries = vec![handoff_blob];

            // Still load query-dependent content:
            ctx.code_compaction = compaction::load_code_compaction(&self.db, &self.project_path).await?;
            ctx.previous_response_id = None;

            if self.semantic.is_available() {
                if let Ok(hits) = messages::semantic_recall(&self.semantic, &self.project_path, query).await {
                    ctx.semantic_context = hits;
                }
            }

            ctx.code_index_hints = self.load_code_index_hints(query).await;

            let repo_path = Path::new(&self.project_path);
            if let Ok(activity) = git_tracker::get_recent_activity(repo_path, 5) {
                if !activity.is_empty() {
                    ctx.repo_activity = Some(activity);
                }
            }

            return Ok(ctx);
        }

        // Normal mode: assemble full context

        // 1. Load recent messages (raw, full fidelity)
        ctx.recent_messages = messages::load_recent_messages(&self.db, RECENT_RAW_COUNT).await?;

        // 2. Load Mira context (corrections, goals, memories)
        ctx.mira_context = MiraContext::load(&self.db, &self.project_path).await?;

        // 3. Load rolling summaries
        ctx.summaries = self.load_summaries(MAX_SUMMARIES_IN_CONTEXT).await?;

        // 4. Load code compaction blob
        ctx.code_compaction = compaction::load_code_compaction(&self.db, &self.project_path).await?;

        // 5. Get previous response ID
        ctx.previous_response_id = self.get_response_id().await?;

        // 6. Semantic recall - relevant past conversation context
        if self.semantic.is_available() {
            if let Ok(hits) = messages::semantic_recall(&self.semantic, &self.project_path, query).await {
                ctx.semantic_context = hits;
            }
        }

        // 7. Code index hints - relevant symbols from codebase
        ctx.code_index_hints = self.load_code_index_hints(query).await;

        // 8. Git activity - recent commits and changes
        let repo_path = Path::new(&self.project_path);
        match git_tracker::get_recent_activity(repo_path, 5) {
            Ok(activity) if !activity.is_empty() => {
                debug!("Loaded git activity: {} commits, {} files changed",
                    activity.recent_commits.len(), activity.changed_files.len());
                ctx.repo_activity = Some(activity);
            }
            Ok(_) => {
                debug!("No recent git activity");
            }
            Err(e) => {
                debug!("Failed to load git activity: {}", e);
            }
        }

        // 9. Anti-amnesia: rejected approaches and past decisions
        ctx.rejected_approaches = anti_amnesia::load_rejected_approaches(&self.db, query, 5).await;
        ctx.past_decisions = anti_amnesia::load_past_decisions(&self.db, query, 5).await;

        if !ctx.rejected_approaches.is_empty() || !ctx.past_decisions.is_empty() {
            debug!("Loaded anti-amnesia context: {} rejected approaches, {} past decisions",
                ctx.rejected_approaches.len(), ctx.past_decisions.len());
        }

        // 10. Graph-enhanced context: related files and call graph
        let active_files = self.get_touched_files();
        ctx.related_files = graph::load_related_files(&self.db, &active_files, 8).await;

        let symbols = graph::extract_symbols_from_hints(&ctx.code_index_hints);
        ctx.call_context = graph::load_call_context(&self.db, &symbols, 15).await;

        if !ctx.related_files.is_empty() || !ctx.call_context.is_empty() {
            debug!("Loaded graph context: {} related files, {} call refs",
                ctx.related_files.len(), ctx.call_context.len());
        }

        // 11. Smart error pattern matching: detect errors and find similar fixes
        let error_patterns = errors::detect_error_patterns(&ctx.recent_messages);
        if !error_patterns.is_empty() {
            ctx.similar_fixes = errors::load_similar_fixes(&self.db, &self.semantic, &error_patterns, 5).await;
            if !ctx.similar_fixes.is_empty() {
                debug!("Found {} similar fixes for detected error patterns", ctx.similar_fixes.len());
            }
        }

        // 12. Index freshness check: warn about stale index entries
        ctx.index_status = freshness::check_index_freshness(&self.db, &self.project_path).await;
        if let Some(ref status) = ctx.index_status {
            if !status.stale_files.is_empty() {
                debug!("Index freshness: {} stale files detected", status.stale_files.len());
            }
        }

        Ok(ctx)
    }

    /// Store a code compaction blob
    pub async fn store_compaction(
        &self,
        encrypted_content: &str,
        files: &[String],
    ) -> Result<String> {
        compaction::store_compaction(&self.db, &self.project_path, encrypted_content, files).await
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

    /// Save a checkpoint after successful tool execution (DeepSeek continuity)
    pub async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        compaction::save_checkpoint(&self.db, &self.project_path, checkpoint).await
    }

    /// Load the most recent checkpoint for this project
    pub async fn load_checkpoint(&self) -> Result<Option<Checkpoint>> {
        compaction::load_checkpoint(&self.db, &self.project_path).await
    }

    /// Clear checkpoint (call after conversation reset)
    pub async fn clear_checkpoint(&self) -> Result<()> {
        compaction::clear_checkpoint(&self.db, &self.project_path).await
    }
}
