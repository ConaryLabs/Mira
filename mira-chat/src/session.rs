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

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::context::MiraContext;
use crate::semantic::{SemanticSearch, COLLECTION_MEMORY};

/// Number of recent messages to keep in the sliding window
const WINDOW_SIZE: usize = 20;

/// Minimum similarity score for semantic recall
const RECALL_THRESHOLD: f32 = 0.65;

/// Number of semantic results to fetch
const RECALL_LIMIT: usize = 5;

/// Message count threshold to trigger summarization
const SUMMARIZE_THRESHOLD: usize = 30;

/// Collection name for chat messages
const COLLECTION_CHAT: &str = "mira_chat_messages";

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

/// Assembled context for a query
#[derive(Debug, Default)]
pub struct AssembledContext {
    /// Recent messages in the sliding window
    pub recent_messages: Vec<ChatMessage>,
    /// Semantically relevant past context
    pub semantic_context: Vec<SemanticHit>,
    /// Mira context (corrections, goals, memories)
    pub mira_context: MiraContext,
    /// Rolling summaries of older conversation
    pub summaries: Vec<String>,
    /// Code compaction blob (if available)
    pub code_compaction: Option<String>,
    /// Previous response ID for OpenAI continuity
    pub previous_response_id: Option<String>,
}

/// A semantic search hit
#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub content: String,
    pub score: f32,
    pub role: String,
    pub created_at: i64,
}

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

        // 1. Load recent messages (sliding window)
        ctx.recent_messages = self.load_recent_messages(WINDOW_SIZE).await?;

        // 2. Semantic recall of relevant past context
        if self.semantic.is_available() && !query.is_empty() {
            ctx.semantic_context = self.semantic_recall(query).await?;
        }

        // 3. Load Mira context
        ctx.mira_context = MiraContext::load(&self.db, &self.project_path).await?;

        // 4. Load rolling summaries
        ctx.summaries = self.load_summaries(3).await?;

        // 5. Load code compaction blob
        ctx.code_compaction = self.load_code_compaction().await?;

        // 6. Get previous response ID
        ctx.previous_response_id = self.get_response_id().await?;

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
                let blocks: Vec<serde_json::Value> =
                    serde_json::from_str(&blocks_json).ok()?;
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
        let filter = Filter::must([Condition::matches(
            "project",
            self.project_path.clone(),
        )]);

        let results = self
            .semantic
            .search(COLLECTION_CHAT, query, RECALL_LIMIT, Some(filter))
            .await?;

        Ok(results
            .into_iter()
            .filter(|r| r.score >= RECALL_THRESHOLD)
            .map(|r| SemanticHit {
                content: r.content,
                score: r.score,
                role: r
                    .metadata
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                created_at: r
                    .metadata
                    .get("created_at")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
            })
            .collect())
    }

    /// Load rolling summaries
    async fn load_summaries(&self, limit: usize) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT summary FROM chat_summaries
            WHERE project_path = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(&self.project_path)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(|(s,)| s).collect())
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
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM chat_messages",
        )
        .fetch_one(&self.db)
        .await?;

        if count.0 as usize <= SUMMARIZE_THRESHOLD {
            return Ok(None);
        }

        // Get oldest messages outside the window (to be summarized)
        let to_summarize_count = count.0 as usize - WINDOW_SIZE;
        if to_summarize_count < 10 {
            return Ok(None);
        }

        info!("Summarization needed: {} messages to compress", to_summarize_count);

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

                let blocks: Vec<serde_json::Value> =
                    serde_json::from_str(&blocks_json).ok()?;
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

    /// Store a summary and delete the summarized messages
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

        // Delete the old messages
        for msg_id in message_ids {
            sqlx::query("DELETE FROM chat_messages WHERE id = $1")
                .bind(msg_id)
                .execute(&self.db)
                .await?;
        }

        // Update message count
        let remaining: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chat_messages")
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

        info!("Stored summary, deleted {} old messages", message_ids.len());
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

/// Session statistics
#[derive(Debug)]
pub struct SessionStats {
    pub total_messages: usize,
    pub summary_count: usize,
    pub has_active_conversation: bool,
    pub has_code_compaction: bool,
}

impl AssembledContext {
    /// Format context for injection into system prompt
    ///
    /// IMPORTANT: Order is optimized for LLM caching (prefix matching).
    /// Static/stable content comes FIRST for maximum cache hits:
    ///   1. Mira context (corrections, goals, memories) - stable per session
    ///   2. Code compaction blob - stable between compactions
    ///   3. Summaries - stable between summarizations
    ///   4. Semantic context - changes per query (LEAST cacheable)
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

        // 4. Semantic context - changes per query (LEAST STABLE)
        // This is query-dependent, so put it LAST to maximize prefix cache hits
        if !self.semantic_context.is_empty() {
            let mut semantic_section = String::from("## Relevant Past Context\n");
            for hit in &self.semantic_context {
                let preview = if hit.content.len() > 200 {
                    format!("{}...", &hit.content[..200])
                } else {
                    hit.content.clone()
                };
                semantic_section.push_str(&format!(
                    "- [{}] (relevance: {:.0}%): {}\n",
                    hit.role,
                    hit.score * 100.0,
                    preview
                ));
            }
            sections.push(semantic_section);
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
            let role_label = if msg.role == "user" { "User" } else { "Assistant" };
            let preview = if msg.content.len() > 500 {
                format!("{}...", &msg.content[..500])
            } else {
                msg.content.clone()
            };
            history.push_str(&format!("**{}**: {}\n\n", role_label, preview));
        }
        history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_serialize() {
        let msg = ChatMessage {
            id: "test".into(),
            role: "user".into(),
            content: "Hello".into(),
            created_at: 0,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("Hello"));
    }
}
