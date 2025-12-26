//! HTTP server for Studio integration
//!
//! Exposes mira-chat functionality via REST/SSE endpoints:
//! - GET /api/status - Health check
//! - POST /api/chat/stream - SSE streaming chat
//! - POST /api/chat/sync - Synchronous chat (for Claude-to-Mira)
//! - GET /api/messages - Paginated message history

pub mod advisory;
mod chat;
mod handlers;
mod markdown_parser;
pub mod routing;
mod stream;
pub mod types;

use anyhow::Result;
use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method},
    routing::{get, post},
    Router,
};
use tower_http::set_header::SetResponseHeaderLayer;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{Any, CorsLayer};

use crate::core::SemanticSearch;

// ============================================================================
// Per-Project Locking
// ============================================================================

/// Manages per-project locks to prevent concurrent operations on the same project.
/// This prevents race conditions in:
/// - Message count updates
/// - Summary/archival operations
/// - Chain reset hysteresis
/// - Handoff blob creation/consumption
#[derive(Default)]
pub struct ProjectLocks {
    locks: RwLock<HashMap<String, Arc<Mutex<()>>>>,
}

impl ProjectLocks {
    pub fn new() -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create a lock for a project. Returns an Arc to the mutex.
    pub async fn get_lock(&self, project_path: &str) -> Arc<Mutex<()>> {
        // Fast path: check if lock exists
        {
            let locks = self.locks.read().await;
            if let Some(lock) = locks.get(project_path) {
                return lock.clone();
            }
        }

        // Slow path: create lock if needed
        let mut locks = self.locks.write().await;
        locks
            .entry(project_path.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Clean up unused locks (call periodically if needed)
    #[allow(dead_code)]
    pub async fn cleanup_unused(&self) {
        let mut locks = self.locks.write().await;
        // Remove locks that only have one reference (this one)
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_project_locks_get_or_create() {
        let locks = ProjectLocks::new();

        // First call creates the lock
        let lock1 = locks.get_lock("/project/a").await;

        // Second call returns the same lock
        let lock2 = locks.get_lock("/project/a").await;

        // Should be the same Arc (same address)
        assert!(Arc::ptr_eq(&lock1, &lock2));
    }

    #[tokio::test]
    async fn test_project_locks_different_projects() {
        let locks = ProjectLocks::new();

        let lock_a = locks.get_lock("/project/a").await;
        let lock_b = locks.get_lock("/project/b").await;

        // Different projects should have different locks
        assert!(!Arc::ptr_eq(&lock_a, &lock_b));
    }

    #[tokio::test]
    async fn test_project_locks_serialization() {
        let locks = Arc::new(ProjectLocks::new());
        let project = "/test/project";

        // Simulate concurrent access
        let locks1 = locks.clone();
        let locks2 = locks.clone();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(10);

        // Task 1: acquires lock, sends 1, waits, sends 3
        let tx1 = tx.clone();
        let t1 = tokio::spawn(async move {
            let lock = locks1.get_lock(project).await;
            let _guard = lock.lock().await;
            tx1.send(1).await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            tx1.send(3).await.unwrap();
        });

        // Task 2: tries to acquire lock immediately, sends 2 when it gets it
        let tx2 = tx.clone();
        let t2 = tokio::spawn(async move {
            // Small delay to ensure task 1 gets lock first
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            let lock = locks2.get_lock(project).await;
            let _guard = lock.lock().await;
            tx2.send(2).await.unwrap();
        });

        t1.await.unwrap();
        t2.await.unwrap();
        drop(tx);

        // Collect results - should be 1, 3, 2 (task 1 completes fully before task 2)
        let mut results = Vec::new();
        while let Some(v) = rx.recv().await {
            results.push(v);
        }

        assert_eq!(results, vec![1, 3, 2], "Lock should serialize access");
    }
}

// Types available for external use (currently internal only)
#[allow(unused_imports)]
pub(crate) use types::{ChatEvent, ChatRequest, MessageBlock, ToolCallResultData, UsageInfo};

// ============================================================================
// Server State
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pub db: Option<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub api_key: String,
    pub default_reasoning_effort: String,
    pub sync_token: Option<String>, // Bearer token for /api/chat/sync
    pub sync_semaphore: Arc<tokio::sync::Semaphore>, // Limit concurrent sync requests
    pub project_locks: Arc<ProjectLocks>, // Per-project locking for concurrency safety
}

// ============================================================================
// Routes
// ============================================================================

/// Max request body size for sync endpoint (64KB - allows for project_path + message overhead)
const SYNC_MAX_BODY_BYTES: usize = 64 * 1024;

/// Max concurrent sync requests
const SYNC_MAX_CONCURRENT: usize = 3;

/// API version - re-export from types module
pub use types::API_VERSION;

/// Create the router with all endpoints
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    // API version header on all responses
    let version_header = SetResponseHeaderLayer::if_not_present(
        header::HeaderName::from_static("x-api-version"),
        HeaderValue::from_static(API_VERSION),
    );

    Router::new()
        .route("/api/status", get(handlers::status_handler))
        .route("/api/chat/stream", post(stream::chat_stream_handler))
        .route(
            "/api/chat/sync",
            post(stream::chat_sync_handler).layer(DefaultBodyLimit::max(SYNC_MAX_BODY_BYTES)),
        )
        .route("/api/messages", get(handlers::messages_handler))
        // Orchestration endpoints
        .route("/api/mcp-history", get(handlers::mcp_history_handler))
        .route("/api/instructions", get(handlers::instructions_handler))
        .route("/api/instructions", post(handlers::create_instruction_handler))
        .layer(version_header)
        .layer(cors)
        .with_state(state)
}

/// Run the HTTP server
pub async fn run(
    port: u16,
    api_key: String,
    db: Option<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    reasoning_effort: String,
    sync_token: Option<String>,
) -> Result<()> {
    let state = AppState {
        db,
        semantic,
        api_key,
        default_reasoning_effort: reasoning_effort,
        sync_token: sync_token.clone(),
        sync_semaphore: Arc::new(tokio::sync::Semaphore::new(SYNC_MAX_CONCURRENT)),
        project_locks: Arc::new(ProjectLocks::new()),
    };

    let app = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    println!("Server listening on http://{}", addr);
    if sync_token.is_some() {
        println!("Sync auth:    ENABLED (via MIRA_SYNC_TOKEN)");
    } else {
        println!("Sync auth:    DISABLED (set MIRA_SYNC_TOKEN to enable)");
    }
    println!("Sync limit:   {} concurrent requests", SYNC_MAX_CONCURRENT);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
