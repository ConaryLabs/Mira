//! Mira MCP Server - Core server implementation
//!
//! NOTE: Some items are infrastructure for future features or external use.

#![allow(dead_code)] // Server infrastructure (some items for future use)

mod db;
pub mod handlers;

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_router, tool_handler,
};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::tools::*;
use crate::context::{ContextCarousel, CarouselTrigger};
use crate::orchestrator::GeminiOrchestrator;
use crate::core::ops::mcp_session::{self, SessionPhase};

// Re-export database utilities
pub use db::{create_optimized_pool, run_migrations};

/// Macro to extract a required field from a request, returning an error if missing.
/// Usage: `require!(req.title, "title required for create")`
macro_rules! require {
    ($field:expr, $msg:expr) => {
        $field.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!($msg)))?
    };
}

/// Helper to create an unknown action error response
fn unknown_action(action: &str, valid_actions: &str) -> CallToolResult {
    CallToolResult::error(vec![Content::text(format!(
        "Unknown action: {}. Use {}", action, valid_actions
    ))])
}

// === Project Context ===

/// Active project context for scoping data
#[derive(Clone, Debug)]
pub struct ProjectContext {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub project_type: Option<String>,
}

// === Mira MCP Server ===

#[derive(Clone)]
pub struct MiraServer {
    pub db: Arc<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub orchestrator: Arc<RwLock<Option<GeminiOrchestrator>>>,
    pub tool_router: ToolRouter<Self>,
    pub active_project: Arc<RwLock<Option<ProjectContext>>>,
    pub carousel: Arc<RwLock<Option<ContextCarousel>>>,
    /// Current MCP session ID (set on initialize or session_start)
    pub mcp_session_id: Arc<RwLock<Option<String>>>,
    /// Current session phase (detected from activity patterns)
    pub session_phase: Arc<RwLock<SessionPhase>>,
}

impl MiraServer {
    pub async fn new(database_url: &str, qdrant_url: Option<&str>, gemini_key: Option<String>) -> Result<Self> {
        info!("Connecting to database: {}", database_url);
        let db = create_optimized_pool(database_url).await?;
        info!("Database connected successfully");

        let semantic = SemanticSearch::new(qdrant_url, gemini_key.clone()).await;
        if semantic.is_available() {
            info!("Semantic search enabled (Qdrant + Gemini)");
        } else {
            info!("Semantic search disabled (using text-based fallback)");
        }

        // Initialize orchestrator if Gemini key is available
        let orchestrator = if let Some(key) = gemini_key {
            match GeminiOrchestrator::new(db.clone(), key).await {
                Ok(orch) => {
                    info!("Gemini orchestrator enabled");
                    Some(orch)
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize orchestrator: {}", e);
                    None
                }
            }
        } else {
            info!("Gemini orchestrator disabled (no API key)");
            None
        };

        Ok(Self {
            db: Arc::new(db),
            semantic: Arc::new(semantic),
            orchestrator: Arc::new(RwLock::new(orchestrator)),
            tool_router: Self::tool_router(),
            active_project: Arc::new(RwLock::new(None)),
            carousel: Arc::new(RwLock::new(None)),
            mcp_session_id: Arc::new(RwLock::new(None)),
            session_phase: Arc::new(RwLock::new(SessionPhase::Early)),
        })
    }

    /// Get the active project context (if set)
    pub async fn get_active_project(&self) -> Option<ProjectContext> {
        self.active_project.read().await.clone()
    }

    /// Set the active project context
    pub async fn set_active_project(&self, ctx: Option<ProjectContext>) {
        *self.active_project.write().await = ctx;
    }

    /// Get the current MCP session ID
    pub async fn get_mcp_session_id(&self) -> Option<String> {
        self.mcp_session_id.read().await.clone()
    }

    /// Set the MCP session ID and create/update session record
    pub async fn set_mcp_session_id(&self, session_id: Option<String>, project_id: Option<i64>) {
        *self.mcp_session_id.write().await = session_id.clone();

        // Create/update session record in database
        if let Some(sid) = session_id {
            let ctx = crate::core::OpContext::just_db(self.db.as_ref().clone());
            if let Err(e) = mcp_session::upsert_mcp_session(&ctx, &sid, project_id).await {
                tracing::warn!("Failed to upsert MCP session: {}", e);
            }
        }
    }

    /// Get the current session phase
    pub async fn get_session_phase(&self) -> SessionPhase {
        *self.session_phase.read().await
    }

    /// Record a tool call and update session metrics
    pub async fn record_tool_activity(&self, tool_name: &str, success: bool) {
        if let Some(session_id) = self.get_mcp_session_id().await {
            let ctx = crate::core::OpContext::just_db(self.db.as_ref().clone());
            match mcp_session::record_tool_call(&ctx, &session_id, tool_name, success).await {
                Ok(new_phase) => {
                    let mut phase = self.session_phase.write().await;
                    if *phase != new_phase {
                        tracing::info!("[SESSION] Phase transition: {:?} → {:?}", *phase, new_phase);
                        *phase = new_phase;
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to record tool activity: {}", e);
                }
            }
        }
    }

    /// Get the tool router (public wrapper for macro-generated function)
    pub fn get_tool_router() -> ToolRouter<Self> {
        Self::tool_router()
    }

    /// Get carousel context for injection into tool responses (simple version)
    pub async fn get_carousel_context(&self) -> Option<String> {
        self.get_carousel_context_with_query(None, &[]).await
    }

    /// Get carousel context with semantic interrupt detection
    ///
    /// Pass the tool's query/input text to enable semantic interrupts:
    /// - Query containing "error", "fail", "bug" → forces RecentErrors category
    /// - Query containing "goal", "milestone" → forces Goals category
    /// - etc.
    ///
    /// Pass triggers for explicit mode changes:
    /// - FileEdit(path) → focus CodeContext
    /// - BuildFailure(msg) → enter Panic mode
    /// - etc.
    pub async fn get_carousel_context_with_query(
        &self,
        query: Option<&str>,
        triggers: &[CarouselTrigger],
    ) -> Option<String> {
        let project_id = self.get_active_project().await.map(|p| p.id);

        // Use orchestrator for intelligent routing if available
        let mut all_triggers = triggers.to_vec();
        if let Some(query_text) = query {
            if let Some(orchestrator) = self.orchestrator.read().await.as_ref() {
                let routing = orchestrator.route(query_text).await;
                if routing.confidence >= 0.7 {
                    // Convert routing decision to carousel trigger
                    let trigger = CarouselTrigger::SemanticMatch(routing.primary, routing.confidence);
                    all_triggers.push(trigger);
                    tracing::debug!(
                        "[ORCHESTRATOR] Routed query to {:?} (confidence={:.2}, source={:?}, {}ms)",
                        routing.primary,
                        routing.confidence,
                        routing.source,
                        routing.latency_ms
                    );
                }
            }
        }

        // Initialize or get carousel
        let mut carousel_guard = self.carousel.write().await;
        let carousel = match carousel_guard.as_mut() {
            Some(c) => c,
            None => {
                // Initialize carousel
                match ContextCarousel::load(self.db.as_ref().clone(), project_id).await {
                    Ok(c) => {
                        *carousel_guard = Some(c);
                        carousel_guard.as_mut().expect("just set")
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load carousel: {}", e);
                        return None;
                    }
                }
            }
        };

        let mut parts = Vec::new();

        // 1. Always include critical items
        if let Ok(Some(critical)) = carousel.render_critical().await {
            parts.push(critical);
        }

        // 2. Get rotating category context
        if let Ok(Some(rotating)) = carousel.render_current().await {
            parts.push(rotating);
        }

        // 3. Tick carousel with context (enables semantic interrupts)
        // Note: If orchestrator provided a trigger, carousel will still run its own
        // detection as fallback, but orchestrator takes precedence via all_triggers
        match carousel.tick_with_context(&all_triggers, query).await {
            Ok((decision, _categories)) => {
                // Log semantic interrupt if it happened
                if !decision.triggers.is_empty() {
                    tracing::info!(
                        "[CAROUSEL] Semantic interrupt: {:?} → {:?}",
                        decision.triggers,
                        decision.category
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to tick carousel: {}", e);
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

#[tool_router]
impl MiraServer {
    // === Admin/Analytics ===

    #[tool(description = "List database tables with row counts.")]
    async fn list_tables(&self) -> Result<CallToolResult, McpError> {
        let result = analytics::list_tables(self.db.as_ref()).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Execute read-only SQL SELECT query.")]
    async fn query(&self, Parameters(req): Parameters<QueryRequest>) -> Result<CallToolResult, McpError> {
        match analytics::query(self.db.as_ref(), req).await {
            Ok(result) => Ok(json_response(result)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Check debounce state. Returns {\"proceed\": true} if action should run, false if debounced.")]
    async fn debounce(&self, Parameters(req): Parameters<DebounceRequest>) -> Result<CallToolResult, McpError> {
        // Use orchestrator if available, otherwise use direct DB
        if let Some(orchestrator) = self.orchestrator.read().await.as_ref() {
            let proceed = orchestrator.check_debounce(&req.key, req.ttl_secs).await;
            Ok(json_response(serde_json::json!({ "proceed": proceed })))
        } else {
            // Fallback: direct SQLite check
            let now = chrono::Utc::now().timestamp();
            let result = sqlx::query_as::<_, (i64,)>(
                "SELECT last_triggered FROM debounce_state WHERE key = $1"
            )
            .bind(&req.key)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| to_mcp_err(anyhow::anyhow!(e)))?;

            let proceed = match result {
                Some((last_triggered,)) => now - last_triggered >= req.ttl_secs as i64,
                None => true,
            };

            if proceed {
                let _ = sqlx::query(
                    "INSERT INTO debounce_state (key, last_triggered, trigger_count)
                     VALUES ($1, $2, 1)
                     ON CONFLICT(key) DO UPDATE SET
                         last_triggered = excluded.last_triggered,
                         trigger_count = trigger_count + 1"
                )
                .bind(&req.key)
                .bind(now)
                .execute(self.db.as_ref())
                .await;
            }

            Ok(json_response(serde_json::json!({ "proceed": proceed })))
        }
    }

    #[tool(description = "Track tool activity for session phase detection. Called by hooks after tool execution.")]
    async fn track_activity(&self, Parameters(req): Parameters<TrackActivityRequest>) -> Result<CallToolResult, McpError> {
        // Record the tool activity
        self.record_tool_activity(&req.tool_name, req.success).await;

        // Optionally track file touch
        if let Some(file_path) = &req.file_path {
            if let Some(session_id) = self.get_mcp_session_id().await {
                let ctx = crate::core::OpContext::just_db(self.db.as_ref().clone());
                let _ = mcp_session::record_file_touch(&ctx, &session_id, file_path).await;
            }
        }

        // Return current phase
        let phase = self.get_session_phase().await;
        Ok(json_response(serde_json::json!({
            "recorded": true,
            "phase": phase.as_str()
        })))
    }

    #[tool(description = "Session heartbeat for liveness tracking. Call every 30-60s during long tasks to prevent session timeout.")]
    async fn heartbeat(&self, Parameters(req): Parameters<HeartbeatRequest>) -> Result<CallToolResult, McpError> {
        let now = chrono::Utc::now().timestamp();

        // Update last_heartbeat in the database
        let result = sqlx::query(
            r#"
            UPDATE claude_sessions
            SET last_heartbeat = $1
            WHERE id = $2 AND status = 'running'
            "#,
        )
        .bind(now)
        .bind(&req.session_id)
        .execute(self.db.as_ref())
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => {
                Ok(json_response(serde_json::json!({
                    "status": "ok",
                    "session_id": req.session_id,
                    "timestamp": now
                })))
            }
            Ok(_) => {
                Ok(json_response(serde_json::json!({
                    "status": "session_not_found",
                    "session_id": req.session_id
                })))
            }
            Err(e) => {
                Ok(CallToolResult::error(vec![Content::text(format!("Heartbeat failed: {}", e))]))
            }
        }
    }

    #[tool(description = "Extract decisions, topics, and insights from transcript text using LLM analysis.")]
    async fn extract(&self, Parameters(req): Parameters<ExtractRequest>) -> Result<CallToolResult, McpError> {
        // Use orchestrator if available
        if let Some(orchestrator) = self.orchestrator.read().await.as_ref() {
            match orchestrator.extract(&req.transcript).await {
                Ok(result) => {
                    // Convert to JSON-serializable format
                    let decisions: Vec<serde_json::Value> = result.decisions.iter().map(|d| {
                        serde_json::json!({
                            "content": d.content,
                            "confidence": d.confidence,
                            "type": format!("{:?}", d.decision_type).to_lowercase(),
                            "context": d.context
                        })
                    }).collect();

                    Ok(json_response(serde_json::json!({
                        "decisions": decisions,
                        "topics": result.topics,
                        "files_modified": result.files_modified,
                        "insights": result.insights,
                        "confidence": result.confidence
                    })))
                }
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Extraction failed: {}", e))])),
            }
        } else {
            // Fallback: basic pattern matching
            let decisions: Vec<String> = req.transcript
                .lines()
                .filter(|line| {
                    let l = line.to_lowercase();
                    l.contains("i'll ") || l.contains("i will ") || l.contains("let's ") ||
                    l.contains("we should ") || l.contains("going to ") || l.contains("decided to ")
                })
                .take(10)
                .map(|s| s.chars().take(150).collect())
                .collect();

            Ok(json_response(serde_json::json!({
                "decisions": decisions.iter().map(|d| serde_json::json!({"content": d, "confidence": 0.5, "type": "approach", "context": "pattern-matched"})).collect::<Vec<_>>(),
                "topics": [],
                "files_modified": [],
                "insights": [],
                "confidence": 0.5
            })))
        }
    }

    // === Memory (core - high usage) ===

    #[tool(description = "Store a fact/decision/preference for future recall. Scoped to active project.")]
    async fn remember(&self, Parameters(req): Parameters<RememberRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let content_preview: String = req.content.chars().take(100).collect();
        let category = req.category.clone();
        let start = std::time::Instant::now();

        let result = memory::remember(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None, // session_id not available at tool level
            project_id,
            "remember",
            Some(&serde_json::json!({"content": &content_preview, "category": category})),
            &format!("Stored: {}", content_preview),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Search memories using semantic similarity.")]
    async fn recall(&self, Parameters(req): Parameters<RecallRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let start = std::time::Instant::now();

        let result = memory::recall(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "recall",
            Some(&serde_json::json!({"query": &query})),
            &format!("Searched: {} ({} results)", query, result.len()),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        // Inject carousel context into response (with query for semantic interrupts)
        let response = vec_response(result, format!("No memories found matching '{}'", query));
        let carousel_ctx = self.get_carousel_context_with_query(Some(&query), &[]).await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(&self, Parameters(req): Parameters<ForgetRequest>) -> Result<CallToolResult, McpError> {
        let result = memory::forget(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Session Context ===

    #[tool(description = "Initialize session: sets project, loads persona, context, corrections, goals. Call once at session start.")]
    async fn session_start(&self, Parameters(req): Parameters<SessionStartRequest>) -> Result<CallToolResult, McpError> {
        let result = sessions::session_start(self.db.as_ref(), req).await.map_err(to_mcp_err)?;

        // Set active project context so subsequent calls are scoped
        let ctx = ProjectContext {
            id: result.project_id,
            path: result.project_path.clone(),
            name: result.project_name.clone(),
            project_type: result.project_type.clone(),
        };
        self.set_active_project(Some(ctx)).await;

        // Create/update MCP session record
        // Generate a session ID if we don't have one yet
        let session_id = self.get_mcp_session_id().await
            .unwrap_or_else(|| format!("session-{}", uuid::Uuid::new_v4()));
        self.set_mcp_session_id(Some(session_id.clone()), Some(result.project_id)).await;

        // Reset session phase to Early for new session
        *self.session_phase.write().await = SessionPhase::Early;

        // Initialize carousel for this session
        {
            let mut carousel_guard = self.carousel.write().await;
            match ContextCarousel::load(self.db.as_ref().clone(), Some(result.project_id)).await {
                Ok(c) => { *carousel_guard = Some(c); }
                Err(e) => { tracing::warn!("Failed to init carousel: {}", e); }
            }
        }

        // Return formatted output with carousel context
        let response = text_response(format::session_start(&result));
        let carousel_ctx = self.get_carousel_context().await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Get context from previous sessions. Call at session start.")]
    async fn get_session_context(&self, Parameters(req): Parameters<GetSessionContextRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::get_session_context(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        let response = json_response(result);
        let carousel_ctx = self.get_carousel_context().await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Store session summary. Call at session end.")]
    async fn store_session(&self, Parameters(req): Parameters<StoreSessionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let summary_preview = req.summary.chars().take(100).collect::<String>();
        let start = std::time::Instant::now();

        let result = sessions::store_session(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "store_session",
            None,
            &format!("Session saved: {}", summary_preview),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Search past sessions semantically.")]
    async fn search_sessions(&self, Parameters(req): Parameters<SearchSessionsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::search_sessions(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No sessions found matching '{}'", query)))
    }

    #[tool(description = "Search MCP tool call history. Useful for recalling what tools were used and their results.")]
    async fn search_mcp_history(&self, Parameters(req): Parameters<SearchMcpHistoryRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let limit = req.limit.unwrap_or(20) as usize;

        // Use semantic search when query is provided, otherwise use regular search
        let result = if let Some(query) = &req.query {
            mcp_history::semantic_search(
                self.db.as_ref(),
                &self.semantic,
                query,
                project_id,
                limit,
            ).await.map_err(to_mcp_err)?
        } else {
            mcp_history::search_history(
                self.db.as_ref(),
                project_id,
                req.tool_name.as_deref(),
                None,
                limit as i64,
            ).await.map_err(to_mcp_err)?
        };

        Ok(vec_response(result, "No MCP history found".to_string()))
    }

    #[tool(description = "Store an important decision with context.")]
    async fn store_decision(&self, Parameters(req): Parameters<StoreDecisionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let decision_key = req.key.clone();
        let start = std::time::Instant::now();

        let result = sessions::store_decision(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "store_decision",
            Some(&serde_json::json!({"key": &decision_key})),
            &format!("Decision: {}", decision_key),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Sync work state for session resume.")]
    async fn sync_work_state(&self, Parameters(req): Parameters<SyncWorkStateRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::sync_work_state(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Get work state for session resume.")]
    async fn get_work_state(&self, Parameters(req): Parameters<GetWorkStateRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::get_work_state(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No work state found".to_string()))
    }

    // === Carousel Control ===

    #[tool(description = "Control context carousel. Actions: status/pin/unpin/advance/focus/panic/exit_panic/anchor/log")]
    async fn carousel(&self, Parameters(req): Parameters<CarouselRequest>) -> Result<CallToolResult, McpError> {
        use crate::context::{ContextCategory, CarouselMode};

        let project_id = self.get_active_project().await.map(|p| p.id);

        // Initialize or get carousel
        let mut carousel_guard = self.carousel.write().await;
        let carousel = match carousel_guard.as_mut() {
            Some(c) => c,
            None => {
                match ContextCarousel::load(self.db.as_ref().clone(), project_id).await {
                    Ok(c) => {
                        *carousel_guard = Some(c);
                        carousel_guard.as_mut().expect("just set")
                    }
                    Err(e) => return Ok(CallToolResult::error(vec![Content::text(format!("Failed to load carousel: {}", e))])),
                }
            }
        };

        match req.action.as_str() {
            "status" => {
                let current = carousel.current();
                let stats = carousel.stats();
                let mode_str = match carousel.mode() {
                    CarouselMode::Cruising => "Cruising".to_string(),
                    CarouselMode::Focus(cat) => format!("Focus({:?})", cat),
                    CarouselMode::Panic => "🚨 PANIC".to_string(),
                };
                let mut status = format!(
                    "Carousel Status:\n  Mode: {}\n  Current: {:?}\n  Position: {}/{}\n  Calls since advance: {}/{}\n  Total calls: {}",
                    mode_str,
                    current,
                    stats.index + 1,
                    ContextCategory::rotation().len(),
                    stats.calls_since_advance,
                    crate::context::ROTATION_INTERVAL,
                    stats.call_count
                );
                if let Some((cat, remaining)) = carousel.is_pinned() {
                    status.push_str(&format!("\n  📌 Pinned: {:?} ({}m remaining)", cat, remaining / 60));
                }
                if !stats.anchor_items.is_empty() {
                    status.push_str(&format!("\n  ⚓ Anchored: {} items", stats.anchor_items.len()));
                    for item in &stats.anchor_items {
                        let content_preview = if item.content.len() > 40 { format!("{}...", &item.content[..40]) } else { item.content.clone() };
                        status.push_str(&format!("\n    • {} (TTL: {} turns)", content_preview, item.ttl_turns));
                    }
                }
                Ok(text_response(status))
            }
            "pin" => {
                let cat_str = req.category.as_deref().ok_or_else(||
                    to_mcp_err(anyhow::anyhow!("category required for pin action")))?;
                let category = ContextCategory::from_str(cat_str).ok_or_else(||
                    to_mcp_err(anyhow::anyhow!("Invalid category: {}. Use: goals/decisions/memories/git/code/system/errors/patterns", cat_str)))?;
                let duration = req.duration_minutes.unwrap_or(30);
                carousel.pin(category, duration).await.map_err(to_mcp_err)?;
                Ok(text_response(format!("📌 Pinned {:?} for {} minutes", category, duration)))
            }
            "unpin" => {
                carousel.unpin().await.map_err(to_mcp_err)?;
                Ok(text_response("Unpinned carousel - rotation resumed"))
            }
            "advance" => {
                let next = carousel.force_advance().await.map_err(to_mcp_err)?;
                Ok(text_response(format!("Advanced to: {:?}", next)))
            }
            "focus" => {
                // Aggressive focus - like pin but enters Focus mode
                let cat_str = req.category.as_deref().ok_or_else(||
                    to_mcp_err(anyhow::anyhow!("category required for focus action")))?;
                let category = ContextCategory::from_str(cat_str).ok_or_else(||
                    to_mcp_err(anyhow::anyhow!("Invalid category: {}. Use: goals/decisions/memories/git/code/system/errors/patterns", cat_str)))?;
                let duration = req.duration_minutes.unwrap_or(30);
                carousel.pin(category, duration).await.map_err(to_mcp_err)?;
                Ok(text_response(format!("🎯 Focus mode: {:?} for {} minutes (suppresses rotation)", category, duration)))
            }
            "panic" => {
                let reason = req.reason.as_deref().unwrap_or("Manual panic mode");
                carousel.enter_panic(reason).await.map_err(to_mcp_err)?;
                Ok(text_response(format!("🚨 Panic mode activated: {}\nContext locked to: errors + code", reason)))
            }
            "exit_panic" => {
                carousel.exit_panic().await.map_err(to_mcp_err)?;
                Ok(text_response("✅ Exited panic mode - returning to Cruising"))
            }
            "anchor" => {
                let content = req.content.as_deref().ok_or_else(||
                    to_mcp_err(anyhow::anyhow!("content required for anchor action")))?;
                let reason = req.reason.as_deref().unwrap_or("User anchored");
                let category = req.category.as_deref()
                    .and_then(ContextCategory::from_str)
                    .unwrap_or(carousel.current());
                carousel.anchor_item(content.to_string(), reason.to_string(), category, 5);
                Ok(text_response(format!("⚓ Anchored item (TTL: 5 turns): {}", if content.len() > 50 { format!("{}...", &content[..50]) } else { content.to_string() })))
            }
            "log" => {
                let log = carousel.decision_log();
                if log.is_empty() {
                    return Ok(text_response("No decisions logged yet"));
                }
                let mut output = format!("Last {} decisions:\n", log.len());
                for (i, decision) in log.iter().rev().take(10).enumerate() {
                    let mode_str = match decision.mode {
                        CarouselMode::Cruising => "Cruise",
                        CarouselMode::Focus(_) => "Focus",
                        CarouselMode::Panic => "PANIC",
                    };
                    output.push_str(&format!(
                        "\n{}. [{}] {:?} - {}{}",
                        i + 1,
                        mode_str,
                        decision.category,
                        decision.reason,
                        if decision.starvation_rescue { " [RESCUE]" } else { "" }
                    ));
                    if !decision.triggers.is_empty() {
                        output.push_str(&format!("\n   Triggers: {}", decision.triggers.join(", ")));
                    }
                }
                Ok(text_response(output))
            }
            _ => Ok(unknown_action(&req.action, "status/pin/unpin/advance/focus/panic/exit_panic/anchor/log")),
        }
    }

    // === Project ===

    #[tool(description = "Set active project.")]
    async fn set_project(&self, Parameters(req): Parameters<SetProjectRequest>) -> Result<CallToolResult, McpError> {
        let result = project::set_project(self.db.as_ref(), req).await.map_err(to_mcp_err)?;

        if let Some(id) = result.get("id").and_then(|v| v.as_i64()) {
            let ctx = ProjectContext {
                id,
                path: result.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                name: result.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                project_type: result.get("project_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
            };
            self.set_active_project(Some(ctx)).await;
        }

        Ok(json_response(result))
    }

    #[tool(description = "Get currently active project.")]
    async fn get_project(&self, Parameters(_req): Parameters<GetProjectRequest>) -> Result<CallToolResult, McpError> {
        match self.get_active_project().await {
            Some(ctx) => Ok(json_response(serde_json::json!({
                "id": ctx.id,
                "path": ctx.path,
                "name": ctx.name,
                "project_type": ctx.project_type,
            }))),
            None => Ok(json_response(serde_json::json!({
                "active": false,
                "message": "No project set. Call set_project() first."
            }))),
        }
    }

    #[tool(description = "Get coding guidelines.")]
    async fn get_guidelines(&self, Parameters(req): Parameters<GetGuidelinesRequest>) -> Result<CallToolResult, McpError> {
        let result = project::get_guidelines(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No guidelines found."))
    }

    #[tool(description = "Add a coding guideline.")]
    async fn add_guideline(&self, Parameters(req): Parameters<AddGuidelineRequest>) -> Result<CallToolResult, McpError> {
        let result = project::add_guideline(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Consolidated Task Tool (6→1) ===

    #[tool(description = "Manage tasks. Actions: create/list/get/update/complete/delete")]
    async fn task(&self, Parameters(req): Parameters<TaskRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "create" => {
                let title = require!(req.title, "title required for create");
                let project_id = self.get_active_project().await.map(|p| p.id);
                let result = tasks::create_task(self.db.as_ref(), tasks::CreateTaskParams {
                    title: title.clone(),
                    description: req.description.clone(),
                    priority: req.priority.clone(),
                    parent_id: req.parent_id.clone(),
                }).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "task",
                    Some(&serde_json::json!({"action": "create", "title": &title})),
                    &format!("Created task: {}", title),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "list" => {
                let result = tasks::list_tasks(self.db.as_ref(), tasks::ListTasksParams {
                    status: req.status.clone(),
                    parent_id: req.parent_id.clone(),
                    include_completed: req.include_completed,
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No tasks found."))
            }
            "get" => {
                let task_id = require!(req.task_id, "task_id required");
                let result = tasks::get_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "update" => {
                let result = tasks::update_task(self.db.as_ref(), tasks::UpdateTaskParams {
                    task_id: require!(req.task_id, "task_id required"),
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Task not found"))
            }
            "complete" => {
                let task_id = require!(req.task_id, "task_id required");
                let project_id = self.get_active_project().await.map(|p| p.id);
                let result = tasks::complete_task(self.db.as_ref(), &task_id, req.notes.clone()).await.map_err(to_mcp_err)?;

                if let Some(ref task) = result {
                    let title = task.get("title").and_then(|t| t.as_str()).unwrap_or("unknown");
                    let _ = mcp_history::log_call_semantic(
                        self.db.as_ref(),
                        &self.semantic,
                        None,
                        project_id,
                        "task",
                        Some(&serde_json::json!({"action": "complete", "task_id": &task_id, "title": title})),
                        &format!("Completed task: {}", title),
                        true,
                        None,
                    ).await;
                }

                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "delete" => {
                let task_id = require!(req.task_id, "task_id required");
                match tasks::delete_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)? {
                    Some(title) => Ok(json_response(serde_json::json!({
                        "status": "deleted",
                        "task_id": task_id,
                        "title": title,
                    }))),
                    None => Ok(text_response(format!("Task {} not found", task_id))),
                }
            }
            action => Ok(unknown_action(action, "create/list/get/update/complete/delete")),
        }
    }

    // === Consolidated Goal Tool (7→1) ===

    #[tool(description = "Manage goals/milestones. Actions: create/list/get/update/delete/add_milestone/complete_milestone/progress")]
    async fn goal(&self, Parameters(req): Parameters<GoalRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "create" => {
                let title = require!(req.title, "title required");
                let result = goals::create_goal(self.db.as_ref(), goals::CreateGoalParams {
                    title: title.clone(),
                    description: req.description.clone(),
                    success_criteria: req.success_criteria.clone(),
                    priority: req.priority.clone(),
                }, project_id).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "goal",
                    Some(&serde_json::json!({"action": "create", "title": &title})),
                    &format!("Created goal: {}", title),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "list" => {
                let result = goals::list_goals(self.db.as_ref(), goals::ListGoalsParams {
                    status: req.status.clone(),
                    include_finished: req.include_finished,
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No goals found."))
            }
            "get" => {
                let goal_id = require!(req.goal_id, "goal_id required");
                let result = goals::get_goal(self.db.as_ref(), &goal_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Goal '{}' not found", goal_id)))
            }
            "update" => {
                let result = goals::update_goal(self.db.as_ref(), goals::UpdateGoalParams {
                    goal_id: require!(req.goal_id, "goal_id required"),
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                    progress_percent: req.progress_percent,
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Goal not found"))
            }
            "add_milestone" => {
                let result = goals::add_milestone(self.db.as_ref(), goals::AddMilestoneParams {
                    goal_id: require!(req.goal_id, "goal_id required"),
                    title: require!(req.title, "title required"),
                    description: req.description.clone(),
                    weight: req.weight,
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "complete_milestone" => {
                let milestone_id = require!(req.milestone_id, "milestone_id required");
                let result = goals::complete_milestone(self.db.as_ref(), &milestone_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Milestone '{}' not found", milestone_id)))
            }
            "progress" => {
                let result = goals::get_goal_progress(self.db.as_ref(), req.goal_id.clone(), project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "delete" => {
                let goal_id = require!(req.goal_id, "goal_id required");
                match goals::delete_goal(self.db.as_ref(), &goal_id).await.map_err(to_mcp_err)? {
                    Some(title) => Ok(json_response(serde_json::json!({
                        "status": "deleted",
                        "goal_id": goal_id,
                        "title": title,
                    }))),
                    None => Ok(text_response(format!("Goal {} not found", goal_id))),
                }
            }
            action => Ok(unknown_action(action, "create/list/get/update/delete/add_milestone/complete_milestone/progress")),
        }
    }

    // === Proposal Tool (Proactive Organization System) ===

    #[tool(description = "Manage proposals (auto-extracted goals/tasks/decisions). Actions: extract/list/confirm/reject/review")]
    async fn proposal(&self, Parameters(req): Parameters<types::ProposalRequest>) -> Result<CallToolResult, McpError> {
        match handlers::proposals::handle(self.db.as_ref(), req).await {
            Ok(result) => Ok(json_response(result)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    // === Consolidated Correction Tool (4→1) ===

    #[tool(description = "Manage corrections. Actions: record/get/validate/list")]
    async fn correction(&self, Parameters(req): Parameters<CorrectionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "record" => {
                let what_was_wrong = require!(req.what_was_wrong, "what_was_wrong required");
                let what_is_right = require!(req.what_is_right, "what_is_right required");
                let result = corrections::record_correction(self.db.as_ref(), &self.semantic, corrections::RecordCorrectionParams {
                    correction_type: require!(req.correction_type, "correction_type required"),
                    what_was_wrong: what_was_wrong.clone(),
                    what_is_right: what_is_right.clone(),
                    rationale: req.rationale.clone(),
                    scope: req.scope.clone(),
                    keywords: req.keywords.clone(),
                }, project_id).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "correction",
                    Some(&serde_json::json!({"action": "record", "wrong": &what_was_wrong, "right": &what_is_right})),
                    &format!("Correction: {} → {}", what_was_wrong.chars().take(50).collect::<String>(), what_is_right.chars().take(50).collect::<String>()),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "get" => {
                let result = corrections::get_corrections(self.db.as_ref(), &self.semantic, corrections::GetCorrectionsParams {
                    file_path: req.file_path.clone(),
                    topic: req.topic.clone(),
                    correction_type: req.correction_type.clone(),
                    context: req.keywords.clone(),
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            "validate" => {
                let result = corrections::validate_correction(
                    self.db.as_ref(),
                    &require!(req.correction_id, "correction_id required"),
                    &require!(req.outcome, "outcome required"),
                ).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let result = corrections::list_corrections(self.db.as_ref(), corrections::ListCorrectionsParams {
                    correction_type: req.correction_type.clone(),
                    scope: req.scope.clone(),
                    status: None,
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            action => Ok(unknown_action(action, "record/get/validate/list")),
        }
    }

    // === Consolidated Document Tool (3→1) ===

    #[tool(description = "Manage documents. Actions: list/search/get/ingest/delete")]
    async fn document(&self, Parameters(req): Parameters<DocumentRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "list" => {
                let result = documents::list_documents(self.db.as_ref(), documents::ListDocumentsParams {
                    doc_type: req.doc_type.clone(),
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No documents found."))
            }
            "search" => {
                let query = require!(req.query, "query required");
                let result = documents::search_documents(self.db.as_ref(), self.semantic.clone(), &query, req.limit).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, format!("No documents found matching '{}'", query)))
            }
            "get" => {
                let document_id = require!(req.document_id, "document_id required");
                let result = documents::get_document(self.db.as_ref(), &document_id, req.include_content.unwrap_or(false)).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Document '{}' not found", document_id)))
            }
            "ingest" => {
                let path = require!(req.path, "path required for ingest");
                let result = ingest::ingest_document(
                    self.db.as_ref(),
                    Some(self.semantic.as_ref()),
                    &path,
                    req.name.as_deref(),
                ).await.map_err(to_mcp_err)?;
                Ok(json_response(serde_json::json!({
                    "status": "ingested",
                    "document_id": result.document_id,
                    "name": result.name,
                    "doc_type": result.doc_type,
                    "chunk_count": result.chunk_count,
                    "total_tokens": result.total_tokens,
                })))
            }
            "delete" => {
                let document_id = require!(req.document_id, "document_id required for delete");
                let deleted = ingest::delete_document(self.db.as_ref(), Some(self.semantic.as_ref()), &document_id).await.map_err(to_mcp_err)?;
                if deleted {
                    Ok(text_response(format!("Document '{}' deleted", document_id)))
                } else {
                    Ok(text_response(format!("Document '{}' not found", document_id)))
                }
            }
            action => Ok(unknown_action(action, "list/search/get/ingest/delete")),
        }
    }

    // === File Search Tool (Gemini RAG) ===

    #[tool(description = "Manage Gemini File Search for per-project RAG. Actions: index/list/remove/status")]
    async fn file_search(&self, Parameters(req): Parameters<FileSearchRequest>) -> Result<CallToolResult, McpError> {
        let project = self.get_active_project().await
            .ok_or_else(|| to_mcp_err(anyhow::anyhow!("No active project. Call set_project first.")))?;

        // Create client from environment
        let client = crate::chat::provider::FileSearchClient::from_env()
            .map_err(to_mcp_err)?;

        match req.action.as_str() {
            "index" => {
                let path = require!(req.path, "path required for index");
                let metadata = if let Some(meta_str) = &req.metadata {
                    Some(serde_json::from_str::<Vec<crate::chat::provider::CustomMetadata>>(meta_str)
                        .map_err(|e| to_mcp_err(anyhow::anyhow!("Invalid metadata JSON: {}", e)))?)
                } else {
                    None
                };
                let result = file_search::index_file(
                    self.db.as_ref(),
                    &client,
                    &project.path,
                    &path,
                    req.display_name.as_deref(),
                    metadata,
                    req.wait.unwrap_or(false),
                ).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let files = file_search::list_indexed_files(self.db.as_ref(), &project.path)
                    .await.map_err(to_mcp_err)?;
                Ok(vec_response(files, "No files indexed yet."))
            }
            "remove" => {
                let path = require!(req.path, "path required for remove");
                let removed = file_search::remove_file(self.db.as_ref(), &project.path, &path)
                    .await.map_err(to_mcp_err)?;
                if removed {
                    Ok(text_response(format!("Removed '{}' from index", path)))
                } else {
                    Ok(text_response(format!("'{}' not found in index", path)))
                }
            }
            "status" => {
                let status = file_search::get_store_status(self.db.as_ref(), &project.path)
                    .await.map_err(to_mcp_err)?;
                Ok(option_response(status, "No FileSearch store for this project."))
            }
            action => Ok(unknown_action(action, "index/list/remove/status")),
        }
    }

    // === Batch Processing Tool (50% cost savings for async operations) ===

    #[tool(description = "Manage batch jobs for async processing via Gemini Batch API. Actions: create/list/get/cancel")]
    async fn batch(&self, Parameters(req): Parameters<BatchRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = batch::handle_batch(&self.db, project_id, &req.action, &req).await.map_err(to_mcp_err)?;
        Ok(text_response(result))
    }

    // === Consolidated Permission Tool (3→1) ===

    #[tool(description = "Manage permission rules. Actions: save/list/delete.")]
    async fn permission(&self, Parameters(req): Parameters<PermissionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "save" => {
                let result = permissions::save_permission(self.db.as_ref(), permissions::SavePermissionParams {
                    tool_name: require!(req.tool_name, "tool_name required"),
                    input_field: req.input_field.clone(),
                    input_pattern: req.input_pattern.clone(),
                    match_type: req.match_type.clone(),
                    scope: req.scope.clone(),
                    description: req.description.clone(),
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let result = permissions::list_permissions(self.db.as_ref(), permissions::ListPermissionsParams {
                    tool_name: req.tool_name.clone(),
                    scope: req.scope.clone(),
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No permission rules found."))
            }
            "delete" => {
                let result = permissions::delete_permission(self.db.as_ref(), &require!(req.rule_id, "rule_id required")).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            action => Ok(unknown_action(action, "save/list/delete")),
        }
    }

    // === Consolidated Build Tool (4→1) ===

    #[tool(description = "Manage build tracking. Actions: record/record_error/get_errors/resolve")]
    async fn build(&self, Parameters(req): Parameters<BuildRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "record" => {
                let command = require!(req.command, "command required");
                let success = require!(req.success, "success required");
                let result = build_intel::record_build(self.db.as_ref(), build_intel::RecordBuildParams {
                    command: command.clone(),
                    success,
                    duration_ms: req.duration_ms,
                }).await.map_err(to_mcp_err)?;

                // Trigger panic mode on build failure
                if !success {
                    let triggers = vec![CarouselTrigger::BuildFailure(command.clone())];
                    let _ = self.get_carousel_context_with_query(Some(&command), &triggers).await;
                }

                Ok(json_response(result))
            }
            "record_error" => {
                let message = require!(req.message, "message required");
                let result = build_intel::record_build_error(self.db.as_ref(), build_intel::RecordBuildErrorParams {
                    message: message.clone(),
                    category: req.category.clone(),
                    severity: req.severity.clone(),
                    file_path: req.file_path.clone(),
                    line_number: req.line_number,
                    code: req.code.clone(),
                }).await.map_err(to_mcp_err)?;

                // Trigger panic mode on error recording
                let triggers = vec![CarouselTrigger::BuildFailure(message.clone())];
                let _ = self.get_carousel_context_with_query(Some(&message), &triggers).await;

                Ok(json_response(result))
            }
            "get_errors" => {
                let result = build_intel::get_build_errors(self.db.as_ref(), build_intel::GetBuildErrorsParams {
                    file_path: req.file_path.clone(),
                    category: req.category.clone(),
                    include_resolved: req.include_resolved,
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No build errors found."))
            }
            "resolve" => {
                let error_id = require!(req.error_id, "error_id required");
                let result = build_intel::resolve_error(self.db.as_ref(), error_id).await.map_err(to_mcp_err)?;

                // Trigger to exit panic mode when error resolved
                let triggers = vec![CarouselTrigger::ErrorResolved];
                let _ = self.get_carousel_context_with_query(None, &triggers).await;

                Ok(json_response(result))
            }
            action => Ok(unknown_action(action, "record/record_error/get_errors/resolve")),
        }
    }

    // === Code Intelligence (keep separate - distinct use cases) ===

    #[tool(description = "Get symbols from a file.")]
    async fn get_symbols(&self, Parameters(req): Parameters<GetSymbolsRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = code_intel::get_symbols(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No symbols in '{}'", file_path)))
    }

    #[tool(description = "Get call graph for a function.")]
    async fn get_call_graph(&self, Parameters(req): Parameters<GetCallGraphRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_call_graph(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Find related files via imports or co-change patterns.")]
    async fn get_related_files(&self, Parameters(req): Parameters<GetRelatedFilesRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_related_files(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search code by meaning.")]
    async fn semantic_code_search(&self, Parameters(req): Parameters<SemanticCodeSearchRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = code_intel::semantic_code_search(self.db.as_ref(), self.semantic.clone(), req).await.map_err(to_mcp_err)?;
        let response = vec_response(result, format!("No code found for '{}'", query));
        // Pass query for semantic interrupt (code search → CodeContext)
        let carousel_ctx = self.get_carousel_context_with_query(Some(&query), &[]).await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Get codebase style metrics.")]
    async fn get_codebase_style(&self, Parameters(req): Parameters<GetCodebaseStyleRequest>) -> Result<CallToolResult, McpError> {
        let project_path = match req.project_path {
            Some(p) => p,
            None => self.get_active_project().await.map(|p| p.path).unwrap_or_else(|| ".".to_string()),
        };
        let report = code_intel::analyze_codebase_style(self.db.as_ref(), &project_path).await.map_err(to_mcp_err)?;
        Ok(text_response(format::style_report(&report)))
    }

    // === Git Intelligence ===

    #[tool(description = "Get recent commits.")]
    async fn get_recent_commits(&self, Parameters(req): Parameters<GetRecentCommitsRequest>) -> Result<CallToolResult, McpError> {
        let result = git_intel::get_recent_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No commits found"))
    }

    #[tool(description = "Search commits by message.")]
    async fn search_commits(&self, Parameters(req): Parameters<SearchCommitsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = git_intel::search_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No commits for '{}'", query)))
    }

    #[tool(description = "Find files that change together.")]
    async fn find_cochange_patterns(&self, Parameters(req): Parameters<FindCochangeRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = git_intel::find_cochange_patterns(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No co-changes for '{}'", file_path)))
    }

    #[tool(description = "Find similar past error fixes.")]
    async fn find_similar_fixes(&self, Parameters(req): Parameters<FindSimilarFixesRequest>) -> Result<CallToolResult, McpError> {
        let error = req.error.clone();
        let result = git_intel::find_similar_fixes(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;
        let response = vec_response(result, format!("No fixes for: {}", error));
        // Error queries should trigger RecentErrors context
        let carousel_ctx = self.get_carousel_context_with_query(Some(&error), &[]).await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Record an error fix.")]
    async fn record_error_fix(&self, Parameters(req): Parameters<RecordErrorFixRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let error_preview: String = req.error_pattern.chars().take(80).collect();
        let fix_preview: String = req.fix_description.chars().take(80).collect();

        let result = git_intel::record_error_fix(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;

        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "record_error_fix",
            Some(&serde_json::json!({"error": &error_preview, "fix": &fix_preview})),
            &format!("Error fix: {} → {}", error_preview, fix_preview),
            true,
            None,
        ).await;

        Ok(json_response(result))
    }

    // === Proactive Context ===

    #[tool(description = "Get all context for current work: corrections, decisions, goals, errors.")]
    async fn get_proactive_context(&self, Parameters(req): Parameters<GetProactiveContextRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        // Build query from task/error for semantic interrupt (clone before move)
        let query = req.task.clone()
            .or_else(|| req.error.clone())
            .unwrap_or_default();
        let result = proactive::get_proactive_context(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;
        let response = json_response(result);
        let carousel_ctx = self.get_carousel_context_with_query(Some(&query), &[]).await;
        Ok(with_carousel_context(response, carousel_ctx))
    }

    #[tool(description = "Record a rejected approach.")]
    async fn record_rejected_approach(&self, Parameters(req): Parameters<RecordRejectedApproachRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let approach_preview: String = req.approach.chars().take(80).collect();
        let reason_preview: String = req.rejection_reason.chars().take(80).collect();

        let result = goals::record_rejected_approach(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "record_rejected_approach",
            Some(&serde_json::json!({"approach": &approach_preview, "reason": &reason_preview})),
            &format!("Rejected: {} ({})", approach_preview, reason_preview),
            true,
            None,
        ).await;

        Ok(json_response(result))
    }

    // === Instruction Queue (Claude Code polling) ===

    #[tool(description = "Get pending instructions from Studio. Claude Code polls this to receive work.")]
    async fn get_pending_instructions(&self, Parameters(req): Parameters<GetPendingInstructionsRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let limit = req.limit.unwrap_or(5) as i32;

        // Get pending instructions, prioritized by urgency
        let rows = sqlx::query_as::<_, (String, String, Option<String>, String, String)>(
            r#"SELECT id, instruction, context, priority, created_at
               FROM instruction_queue
               WHERE status = 'pending'
                 AND ($1 IS NULL OR project_id = $1)
               ORDER BY
                 CASE priority
                   WHEN 'urgent' THEN 1
                   WHEN 'high' THEN 2
                   WHEN 'normal' THEN 3
                   ELSE 4
                 END,
                 created_at ASC
               LIMIT $2"#
        )
        .bind(project_id)
        .bind(limit)
        .fetch_all(self.db.as_ref())
        .await
        .map_err(|e| to_mcp_err(e.into()))?;

        if rows.is_empty() {
            return Ok(json_response(serde_json::json!({
                "pending": 0,
                "instructions": []
            })));
        }

        // Mark as delivered
        for (id, _, _, _, _) in &rows {
            let _ = sqlx::query(
                "UPDATE instruction_queue SET status = 'delivered', delivered_at = datetime('now') WHERE id = $1"
            )
            .bind(id)
            .execute(self.db.as_ref())
            .await;
        }

        let instructions: Vec<serde_json::Value> = rows.iter().map(|(id, instruction, context, priority, created_at)| {
            serde_json::json!({
                "id": id,
                "instruction": instruction,
                "context": context,
                "priority": priority,
                "created_at": created_at
            })
        }).collect();

        Ok(json_response(serde_json::json!({
            "pending": instructions.len(),
            "instructions": instructions
        })))
    }

    #[tool(description = "Mark an instruction as in_progress, completed, or failed.")]
    async fn mark_instruction(&self, Parameters(req): Parameters<MarkInstructionRequest>) -> Result<CallToolResult, McpError> {
        let valid_statuses = ["in_progress", "completed", "failed"];
        if !valid_statuses.contains(&req.status.as_str()) {
            return Ok(CallToolResult::error(vec![Content::text(
                format!("Invalid status: {}. Use: in_progress/completed/failed", req.status)
            )]));
        }

        let (time_field, time_value) = match req.status.as_str() {
            "in_progress" => ("started_at", "datetime('now')"),
            "completed" | "failed" => ("completed_at", "datetime('now')"),
            _ => ("", ""),
        };

        // Build dynamic query based on status
        let query = if req.status == "failed" {
            format!(
                "UPDATE instruction_queue SET status = $1, {} = {}, error = $2 WHERE id = $3",
                time_field, time_value
            )
        } else if req.status == "completed" {
            format!(
                "UPDATE instruction_queue SET status = $1, {} = {}, result = $2 WHERE id = $3",
                time_field, time_value
            )
        } else {
            format!(
                "UPDATE instruction_queue SET status = $1, {} = {} WHERE id = $2",
                time_field, time_value
            )
        };

        let result = if req.status == "in_progress" {
            sqlx::query(&query)
                .bind(&req.status)
                .bind(&req.instruction_id)
                .execute(self.db.as_ref())
                .await
        } else {
            sqlx::query(&query)
                .bind(&req.status)
                .bind(&req.result)
                .bind(&req.instruction_id)
                .execute(self.db.as_ref())
                .await
        };

        match result {
            Ok(r) if r.rows_affected() > 0 => {
                Ok(json_response(serde_json::json!({
                    "status": "updated",
                    "instruction_id": req.instruction_id,
                    "new_status": req.status
                })))
            }
            Ok(_) => {
                Ok(CallToolResult::error(vec![Content::text(
                    format!("Instruction {} not found", req.instruction_id)
                )]))
            }
            Err(e) => {
                Ok(CallToolResult::error(vec![Content::text(format!("Database error: {}", e))]))
            }
        }
    }

    // === Indexing ===

    #[tool(description = "Index code and git history. Actions: project/file/status/cleanup")]
    async fn index(&self, Parameters(req): Parameters<IndexRequest>) -> Result<CallToolResult, McpError> {
        use handlers::indexing;

        let result = match req.action.as_str() {
            "project" => indexing::index_project(self.db.as_ref(), self.semantic.clone(), &req).await,
            "file" => indexing::index_file(self.db.as_ref(), self.semantic.clone(), &req).await,
            "status" => indexing::index_status(self.db.as_ref()).await,
            "cleanup" => indexing::index_cleanup(self.db.as_ref()).await,
            action => return Ok(unknown_action(action, "project/file/status/cleanup")),
        };

        match result {
            Ok(r) => Ok(json_response(r)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }
}

#[tool_handler]
impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        // Include current date/time so Claude always knows the actual date
        let now = chrono::Local::now();
        let date_str = now.format("%Y-%m-%d %H:%M:%S %Z").to_string();

        let instructions = format!(
            "Mira Power Suit - Memory and Intelligence Layer for Claude Code.\n\n\
            CURRENT DATE/TIME: {}\n\n\
            Features: semantic memory (remember/recall), cross-session context, persistent tasks, \
            code intelligence, git intelligence, and document search. All search tools use semantic \
            similarity when Qdrant + Gemini are configured.",
            date_str
        );

        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "mira".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(instructions),
        }
    }
}
