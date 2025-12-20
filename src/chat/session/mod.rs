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
pub mod git_tracker;
mod summarization;
mod types;

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::chat::context::MiraContext;
use mira_core::semantic::SemanticSearch;

pub use chain::ResetDecision;
pub use context::{DeepSeekBudget, TokenUsage};
pub use types::{
    AssembledContext, CallReference, ChatMessage, Checkpoint, CodeIndexFileHint,
    CodeIndexSymbolHint, IndexStatus, PastDecision, RejectedApproach, RelatedFile,
    SemanticHit, SessionStats, SimilarFix,
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

            // Git activity - still useful in handoff mode
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

        // 8. Git activity - recent commits and changes
        // Gives the LLM awareness of "what just happened" in the codebase
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
        // Prevents repeating mistakes and ensures past decisions are remembered
        ctx.rejected_approaches = self.load_rejected_approaches(query, 5).await;
        ctx.past_decisions = self.load_past_decisions(query, 5).await;

        if !ctx.rejected_approaches.is_empty() || !ctx.past_decisions.is_empty() {
            debug!("Loaded anti-amnesia context: {} rejected approaches, {} past decisions",
                ctx.rejected_approaches.len(), ctx.past_decisions.len());
        }

        // 10. Graph-enhanced context: related files and call graph
        // Files that change together + what calls/is called by current focus
        let active_files = self.get_active_files();
        ctx.related_files = self.load_related_files(&active_files, 8).await;

        // Get symbols from code hints for call graph expansion
        let symbols = Self::extract_symbols_from_hints(&ctx.code_index_hints);
        ctx.call_context = self.load_call_context(&symbols, 15).await;

        if !ctx.related_files.is_empty() || !ctx.call_context.is_empty() {
            debug!("Loaded graph context: {} related files, {} call refs",
                ctx.related_files.len(), ctx.call_context.len());
        }

        // 11. Smart error pattern matching: detect errors and find similar fixes
        // Scan recent messages for error patterns and suggest relevant past fixes
        let error_patterns = Self::detect_error_patterns(&ctx.recent_messages);
        if !error_patterns.is_empty() {
            ctx.similar_fixes = self.load_similar_fixes(&error_patterns, 5).await;
            if !ctx.similar_fixes.is_empty() {
                debug!("Found {} similar fixes for detected error patterns", ctx.similar_fixes.len());
            }
        }

        // 12. Index freshness check: warn about stale index entries
        // Helps the LLM know if code intelligence might be outdated
        ctx.index_status = self.check_index_freshness().await;
        if let Some(ref status) = ctx.index_status {
            if !status.stale_files.is_empty() {
                debug!("Index freshness: {} stale files detected", status.stale_files.len());
            }
        }

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

    /// Load rejected approaches relevant to the query (anti-amnesia)
    /// Searches by keyword matching in problem_context, approach, and related_topics
    async fn load_rejected_approaches(&self, query: &str, limit: usize) -> Vec<types::RejectedApproach> {
        // Extract keywords from query for matching
        let keywords: Vec<&str> = query
            .split_whitespace()
            .filter(|w| w.len() > 3)  // Skip short words
            .take(5)
            .collect();

        if keywords.is_empty() {
            return Vec::new();
        }

        // Build LIKE conditions for each keyword
        let like_conditions: Vec<String> = keywords
            .iter()
            .map(|_| "(problem_context LIKE ? OR approach LIKE ? OR related_topics LIKE ?)".to_string())
            .collect();

        let query_str = format!(
            r#"
            SELECT problem_context, approach, rejection_reason
            FROM rejected_approaches
            WHERE {}
            ORDER BY created_at DESC
            LIMIT ?
            "#,
            like_conditions.join(" OR ")
        );

        // Build the query with bound parameters
        let mut sql_query = sqlx::query_as::<_, (String, String, String)>(&query_str);

        for kw in &keywords {
            let pattern = format!("%{}%", kw);
            sql_query = sql_query.bind(pattern.clone()).bind(pattern.clone()).bind(pattern);
        }
        sql_query = sql_query.bind(limit as i64);

        match sql_query.fetch_all(&self.db).await {
            Ok(rows) => rows
                .into_iter()
                .map(|(problem_context, approach, rejection_reason)| {
                    types::RejectedApproach {
                        problem_context,
                        approach,
                        rejection_reason,
                    }
                })
                .collect(),
            Err(e) => {
                debug!("Failed to load rejected approaches: {}", e);
                Vec::new()
            }
        }
    }

    /// Load past decisions relevant to the query (anti-amnesia)
    /// Uses semantic search when available, falls back to keyword matching
    async fn load_past_decisions(&self, query: &str, limit: usize) -> Vec<types::PastDecision> {
        // Try keyword-based search first (simpler, always works)
        let keywords: Vec<&str> = query
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .take(5)
            .collect();

        if keywords.is_empty() {
            return Vec::new();
        }

        // Build LIKE conditions
        let like_conditions: Vec<String> = keywords
            .iter()
            .map(|_| "(key LIKE ? OR decision LIKE ? OR context LIKE ?)".to_string())
            .collect();

        let query_str = format!(
            r#"
            SELECT key, decision, context
            FROM decisions
            WHERE {}
            ORDER BY created_at DESC
            LIMIT ?
            "#,
            like_conditions.join(" OR ")
        );

        let mut sql_query = sqlx::query_as::<_, (String, String, Option<String>)>(&query_str);

        for kw in &keywords {
            let pattern = format!("%{}%", kw);
            sql_query = sql_query.bind(pattern.clone()).bind(pattern.clone()).bind(pattern);
        }
        sql_query = sql_query.bind(limit as i64);

        match sql_query.fetch_all(&self.db).await {
            Ok(rows) => rows
                .into_iter()
                .map(|(key, decision, context)| {
                    types::PastDecision {
                        key,
                        decision,
                        context,
                    }
                })
                .collect(),
            Err(e) => {
                debug!("Failed to load past decisions: {}", e);
                Vec::new()
            }
        }
    }

    /// Load related files from cochange patterns
    /// Files that historically change together with the active files
    async fn load_related_files(&self, active_files: &[String], limit: usize) -> Vec<types::RelatedFile> {
        if active_files.is_empty() {
            return Vec::new();
        }

        let mut related: Vec<types::RelatedFile> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Add active files to seen set to avoid including them
        for f in active_files {
            seen.insert(f.clone());
        }

        for file in active_files.iter().take(3) {
            let rows = sqlx::query_as::<_, (String, i64, f64)>(
                r#"
                SELECT file2 as related_file, cochange_count, confidence
                FROM cochange_patterns
                WHERE file1 LIKE $1
                  AND cochange_count >= 2
                ORDER BY confidence DESC, cochange_count DESC
                LIMIT $2
                "#,
            )
            .bind(format!("%{}", file))
            .bind(limit as i64)
            .fetch_all(&self.db)
            .await
            .unwrap_or_default();

            for (file_path, cochange_count, confidence) in rows {
                if seen.contains(&file_path) {
                    continue;
                }
                seen.insert(file_path.clone());

                related.push(types::RelatedFile {
                    file_path,
                    cochange_count,
                    confidence,
                });
            }
        }

        // Sort by confidence and truncate
        related.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        related.truncate(limit);

        related
    }

    /// Load call graph context for active symbols
    /// Shows what functions call or are called by the current focus
    async fn load_call_context(&self, symbols: &[String], limit: usize) -> Vec<types::CallReference> {
        if symbols.is_empty() {
            return Vec::new();
        }

        let mut call_refs: Vec<types::CallReference> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for symbol in symbols.iter().take(5) {
            // Get callers (who calls this symbol)
            let callers = sqlx::query_as::<_, (String, String, Option<String>)>(
                r#"
                SELECT caller.name, caller.file_path, cg.call_type
                FROM call_graph cg
                JOIN code_symbols caller ON cg.caller_id = caller.id
                JOIN code_symbols callee ON cg.callee_id = callee.id
                WHERE callee.name = $1 OR cg.callee_name = $1
                LIMIT 10
                "#,
            )
            .bind(symbol)
            .fetch_all(&self.db)
            .await
            .unwrap_or_default();

            for (name, file_path, call_type) in callers {
                let key = format!("caller:{}:{}", file_path, name);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                call_refs.push(types::CallReference {
                    symbol_name: name,
                    file_path,
                    direction: "caller".to_string(),
                    call_type,
                });
            }

            // Get callees (what this symbol calls)
            let callees = sqlx::query_as::<_, (String, String, Option<String>)>(
                r#"
                SELECT callee.name, callee.file_path, cg.call_type
                FROM call_graph cg
                JOIN code_symbols caller ON cg.caller_id = caller.id
                JOIN code_symbols callee ON cg.callee_id = callee.id
                WHERE caller.name = $1 OR cg.caller_name = $1
                LIMIT 10
                "#,
            )
            .bind(symbol)
            .fetch_all(&self.db)
            .await
            .unwrap_or_default();

            for (name, file_path, call_type) in callees {
                let key = format!("callee:{}:{}", file_path, name);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                call_refs.push(types::CallReference {
                    symbol_name: name,
                    file_path,
                    direction: "callee".to_string(),
                    call_type,
                });
            }
        }

        call_refs.truncate(limit);
        call_refs
    }

    /// Extract active files from git activity and touched files
    fn get_active_files(&self) -> Vec<String> {
        self.get_touched_files()
    }

    /// Extract symbol names from code hints for call graph expansion
    fn extract_symbols_from_hints(hints: &[types::CodeIndexFileHint]) -> Vec<String> {
        hints
            .iter()
            .flat_map(|h| h.symbols.iter().map(|s| s.name.clone()))
            .take(10)
            .collect()
    }

    /// Detect error patterns in recent messages
    /// Returns a list of error strings that can be used to find similar past fixes
    fn detect_error_patterns(messages: &[ChatMessage]) -> Vec<String> {
        use regex::Regex;
        use std::collections::HashSet;

        let mut patterns: HashSet<String> = HashSet::new();

        // Common error pattern regexes
        let error_patterns = [
            // Rust errors
            r"error\[E\d+\]:\s*(.+)",
            r"cannot\s+(find|move|borrow|infer)\s+(.+)",
            r"the trait .+ is not implemented",
            r"no method named .+ found",
            r"mismatched types",
            // General errors
            r"(?i)error:\s*(.+)",
            r"(?i)failed:\s*(.+)",
            r"(?i)panic(ked)?:\s*(.+)",
            r"(?i)exception:\s*(.+)",
            // Stack traces
            r"at .+:\d+:\d+",
            // Build/test failures
            r"(?i)build failed",
            r"(?i)test failed",
            r"(?i)compilation error",
        ];

        // Compile regexes
        let compiled: Vec<Regex> = error_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        // Check each message for error patterns
        for msg in messages {
            // Skip assistant messages for error detection (focus on user-reported errors)
            // But check both for debugging context
            for line in msg.content.lines().take(50) {
                for re in &compiled {
                    if re.is_match(line) {
                        // Truncate long error lines
                        let error = if line.len() > 150 {
                            format!("{}...", &line[..150])
                        } else {
                            line.to_string()
                        };
                        patterns.insert(error);

                        // Limit patterns to avoid query bloat
                        if patterns.len() >= 10 {
                            return patterns.into_iter().collect();
                        }
                    }
                }
            }

            // Also check for explicit error indicators in message content
            let lower = msg.content.to_lowercase();
            if lower.contains("not working")
                || lower.contains("doesn't work")
                || lower.contains("broken")
                || lower.contains("fix this")
                || lower.contains("getting an error")
            {
                // Mark as "implicit error" for context
                patterns.insert("__implicit_error__".to_string());
            }
        }

        patterns.into_iter().collect()
    }

    /// Load similar fixes from error_fixes table
    /// Uses semantic search when available, falls back to text matching
    async fn load_similar_fixes(&self, error_patterns: &[String], limit: usize) -> Vec<types::SimilarFix> {
        if error_patterns.is_empty() {
            return Vec::new();
        }

        // Skip implicit error marker for search
        let searchable: Vec<&String> = error_patterns
            .iter()
            .filter(|p| *p != "__implicit_error__")
            .collect();

        if searchable.is_empty() {
            return Vec::new();
        }

        // Build a combined search query
        let combined_query = searchable.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");

        // Try semantic search first if available
        if self.semantic.is_available() {
            if let Ok(results) = self.semantic
                .search("mira_error_fixes", &combined_query, limit, None)
                .await
            {
                if !results.is_empty() {
                    return results
                        .into_iter()
                        .map(|r| types::SimilarFix {
                            error_pattern: r.metadata
                                .get("error_pattern")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            fix_description: r.content,
                            score: r.score,
                        })
                        .collect();
                }
            }
        }

        // Fallback to SQL text matching
        let mut fixes: Vec<types::SimilarFix> = Vec::new();

        for pattern in searchable.iter().take(3) {
            let like_pattern = format!("%{}%", pattern);

            let rows = sqlx::query_as::<_, (String, String)>(
                r#"
                SELECT error_pattern, fix_description
                FROM error_fixes
                WHERE error_pattern LIKE $1
                ORDER BY created_at DESC
                LIMIT $2
                "#,
            )
            .bind(&like_pattern)
            .bind(limit as i64)
            .fetch_all(&self.db)
            .await
            .unwrap_or_default();

            for (error_pattern, fix_description) in rows {
                fixes.push(types::SimilarFix {
                    error_pattern,
                    fix_description,
                    score: 0.5, // Default score for text match
                });
            }
        }

        fixes.truncate(limit);
        fixes
    }

    /// Check index freshness for files in the project
    /// Returns status showing stale files that may have outdated symbol information
    async fn check_index_freshness(&self) -> Option<types::IndexStatus> {
        // Get last indexed time for the project
        let last_indexed: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT MAX(analyzed_at) FROM code_symbols
            WHERE file_path LIKE $1
            "#,
        )
        .bind(format!("{}%", self.project_path))
        .fetch_optional(&self.db)
        .await
        .ok()?;

        let last_indexed_ts = last_indexed.and_then(|r| Some(r.0));

        // Get list of files modified since last index
        let stale_files = self.get_stale_files(last_indexed_ts).await;

        if stale_files.is_empty() && last_indexed_ts.is_none() {
            return None;
        }

        Some(types::IndexStatus {
            stale_files,
            last_indexed: last_indexed_ts,
        })
    }

    /// Get files modified since last index
    async fn get_stale_files(&self, since: Option<i64>) -> Vec<String> {
        use std::process::Command;

        let Some(since_ts) = since else {
            return Vec::new();
        };

        // Use git to find files modified since the timestamp
        let since_date = chrono::DateTime::from_timestamp(since_ts, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default();

        if since_date.is_empty() {
            return Vec::new();
        }

        // Get files modified since the index time using git
        let output = Command::new("git")
            .args([
                "diff", "--name-only",
                &format!("--since={}", since_date),
                "HEAD",
            ])
            .current_dir(&self.project_path)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter(|l| l.ends_with(".rs") || l.ends_with(".ts") || l.ends_with(".py"))
                    .take(10)
                    .map(|s| s.to_string())
                    .collect()
            }
            _ => {
                // Fallback: check file mtime vs index time
                self.check_file_mtimes(since_ts).await
            }
        }
    }

    /// Fallback: check file modification times vs index time
    async fn check_file_mtimes(&self, since_ts: i64) -> Vec<String> {
        // Get indexed files with their analyzed_at times
        let indexed_files: Vec<(String, i64)> = sqlx::query_as(
            r#"
            SELECT DISTINCT file_path, MAX(analyzed_at) as last_analyzed
            FROM code_symbols
            WHERE file_path LIKE $1
            GROUP BY file_path
            LIMIT 100
            "#,
        )
        .bind(format!("{}%", self.project_path))
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        let mut stale = Vec::new();

        for (file_path, analyzed_at) in indexed_files {
            if let Ok(metadata) = std::fs::metadata(&file_path) {
                if let Ok(mtime) = metadata.modified() {
                    let mtime_ts = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    if mtime_ts > analyzed_at {
                        // File was modified after indexing
                        stale.push(file_path);
                    }
                }
            }

            if stale.len() >= 10 {
                break;
            }
        }

        stale
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
