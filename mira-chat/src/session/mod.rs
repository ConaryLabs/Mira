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

mod chain;
mod code_hints;
mod context;
mod summarization;
mod types;

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::context::MiraContext;
use crate::semantic::SemanticSearch;

pub use chain::ResetDecision;
pub use context::DeepSeekBudget;
pub use types::{
    AssembledContext, ChatMessage, Checkpoint, CodeIndexFileHint, CodeIndexSymbolHint, SemanticHit, SessionStats,
};

/// Number of recent messages to keep raw in context (full fidelity)
const RECENT_RAW_COUNT: usize = 5;

/// Batch size for summarization (summarize this many at once)
const SUMMARIZE_BATCH_SIZE: usize = 5;

/// Message count threshold to trigger summarization (RECENT_RAW_COUNT + SUMMARIZE_BATCH_SIZE)
const SUMMARIZE_THRESHOLD: usize = 10;

/// Minimum similarity score for semantic recall
/// Raised from 0.65 to 0.75 to reduce "confident irrelevance"
const _RECALL_THRESHOLD: f32 = 0.75;

/// Number of semantic results to fetch
/// Lowered from 5 to 3 - budget will cap further for DeepSeek
const _RECALL_LIMIT: usize = 3;

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
            // This prevents duplicating summaries/goals/decisions that are already in the blob

            // The handoff blob becomes our summary (it includes recent convo + summary + goals + decisions)
            ctx.summaries = vec![handoff_blob];

            // Skip normal context that would duplicate handoff content:
            // - recent_messages: already in handoff
            // - mira_context: goals/decisions already in handoff
            // - summaries: already in handoff

            // Still load these (query-dependent, not in handoff):
            ctx.code_compaction = self.load_code_compaction().await?;

            // No previous_response_id (chain was reset)
            ctx.previous_response_id = None;

            // Semantic recall is query-specific, still useful
            if self.semantic.is_available() {
                if let Ok(hits) = self.semantic_recall(query).await {
                    ctx.semantic_context = hits;
                }
            }

            // Code index hints are query-specific
            ctx.code_index_hints = self.load_code_index_hints(query).await;

            return Ok(ctx);
        }

        // Normal mode: assemble full context

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

    /// Save a checkpoint after successful tool execution (DeepSeek continuity)
    ///
    /// Checkpoints replace server-side chain state for DeepSeek.
    /// Stored in work_context with 24h TTL.
    pub async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        let now = Utc::now().timestamp();
        let expires_at = now + (24 * 3600); // 24 hour TTL
        let value = serde_json::to_string(checkpoint)?;

        sqlx::query(
            r#"
            INSERT INTO work_context (context_type, context_key, context_value, priority, expires_at, created_at, updated_at, project_id)
            VALUES ('deepseek_checkpoint', $1, $2, 0, $3, $4, $4, NULL)
            ON CONFLICT(context_type, context_key) DO UPDATE SET
                context_value = excluded.context_value,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&self.project_path)
        .bind(&value)
        .bind(expires_at)
        .bind(now)
        .execute(&self.db)
        .await?;

        debug!("Saved checkpoint: {}", checkpoint.id);
        Ok(())
    }

    /// Load the most recent checkpoint for this project
    pub async fn load_checkpoint(&self) -> Result<Option<Checkpoint>> {
        let now = Utc::now().timestamp();

        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT context_value FROM work_context
            WHERE context_type = 'deepseek_checkpoint'
              AND context_key = $1
              AND expires_at > $2
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(&self.project_path)
        .bind(now)
        .fetch_optional(&self.db)
        .await?;

        match row {
            Some((json,)) => {
                let checkpoint: Checkpoint = serde_json::from_str(&json)?;
                debug!("Loaded checkpoint: {}", checkpoint.id);
                Ok(Some(checkpoint))
            }
            None => Ok(None),
        }
    }

    /// Clear checkpoint (call after conversation reset)
    pub async fn clear_checkpoint(&self) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM work_context
            WHERE context_type = 'deepseek_checkpoint'
              AND context_key = $1
            "#,
        )
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;

        debug!("Cleared checkpoint for {}", self.project_path);
        Ok(())
    }
}
