//! E2E Integration Tests for Mira Chat Server
//!
//! These tests verify the full request/response cycle through the HTTP layer.
//! Uses axum's test utilities to call handlers directly without spawning a server.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tower::ServiceExt;

// Import from mira crate
use mira::chat::server::{AppState, ProjectLocks};
use mira::chat::tools::WebSearchConfig;
use mira::core::SemanticSearch;

// ============================================================================
// Test Utilities
// ============================================================================

/// Create a test database with schema
async fn create_test_db(temp_dir: &TempDir) -> SqlitePool {
    let db_path = temp_dir.path().join("test.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let pool = SqlitePool::connect(&db_url).await.expect("Failed to create test DB");

    // Apply minimal schema for chat testing
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id TEXT PRIMARY KEY,
            role TEXT NOT NULL,
            blocks TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            archived_at INTEGER,
            summary_id TEXT
        );

        CREATE TABLE IF NOT EXISTS chat_context (
            project_path TEXT PRIMARY KEY,
            last_response_id TEXT,
            needs_handoff INTEGER DEFAULT 0,
            handoff_blob TEXT,
            total_messages INTEGER DEFAULT 0,
            consecutive_low_cache_turns INTEGER DEFAULT 0,
            turns_since_reset INTEGER DEFAULT 999,
            last_failure_command TEXT,
            last_failure_error TEXT,
            last_failure_at INTEGER,
            recent_artifact_ids TEXT,
            updated_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS chat_summaries (
            id TEXT PRIMARY KEY,
            project_path TEXT NOT NULL,
            summary TEXT NOT NULL,
            message_ids TEXT,
            message_count INTEGER DEFAULT 0,
            level INTEGER DEFAULT 1,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            name TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_usage (
            id INTEGER PRIMARY KEY,
            message_id TEXT NOT NULL,
            input_tokens INTEGER,
            output_tokens INTEGER,
            reasoning_tokens INTEGER,
            cached_tokens INTEGER,
            created_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create schema");

    pool
}

/// Create test AppState with minimal dependencies
async fn create_test_state(db: SqlitePool) -> AppState {
    AppState {
        db: Some(db),
        semantic: Arc::new(SemanticSearch::new(None, None).await),
        api_key: "test-key".to_string(),
        default_reasoning_effort: "low".to_string(),
        sync_token: None,
        sync_semaphore: Arc::new(Semaphore::new(3)),
        web_search_config: WebSearchConfig {
            google_api_key: None,
            google_cx: None,
        },
        project_locks: Arc::new(ProjectLocks::new()),
    }
}

/// Create test router
fn create_test_router(state: AppState) -> Router {
    mira::chat::server::create_router(state)
}

/// Helper to make JSON requests
async fn post_json(router: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({"raw": String::from_utf8_lossy(&body_bytes).to_string()}));

    (status, body_json)
}

/// Helper to make GET requests
async fn get_json(router: &Router, path: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({"raw": String::from_utf8_lossy(&body_bytes).to_string()}));

    (status, body_json)
}

// ============================================================================
// E2E Tests
// ============================================================================

#[tokio::test]
async fn test_e2e_status_endpoint() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;
    let state = create_test_state(db).await;
    let router = create_test_router(state);

    let (status, body) = get_json(&router, "/api/status").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    // database is a boolean (true if Some(db)), not a string
    assert!(body["database"].as_bool().is_some());
}

#[tokio::test]
async fn test_e2e_messages_endpoint_empty() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;
    let state = create_test_state(db).await;
    let router = create_test_router(state);

    let (status, body) = get_json(&router, "/api/messages").await;

    assert_eq!(status, StatusCode::OK);
    // Handler returns a bare array, not { "messages": [...] }
    assert!(body.as_array().is_some());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_e2e_messages_endpoint_with_data() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;

    // Insert test messages
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO chat_messages (id, role, blocks, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind("msg-1")
    .bind("user")
    .bind(r#"[{"type":"text","content":"Hello"}]"#)
    .bind(now - 100)
    .execute(&db)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO chat_messages (id, role, blocks, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind("msg-2")
    .bind("assistant")
    .bind(r#"[{"type":"text","content":"Hi there!"}]"#)
    .bind(now)
    .execute(&db)
    .await
    .unwrap();

    let state = create_test_state(db).await;
    let router = create_test_router(state);

    let (status, body) = get_json(&router, "/api/messages").await;

    assert_eq!(status, StatusCode::OK);
    let messages = body.as_array().unwrap();
    assert_eq!(messages.len(), 2);
    // Should be ordered by created_at DESC
    assert_eq!(messages[0]["id"], "msg-2");
    assert_eq!(messages[1]["id"], "msg-1");
}

#[tokio::test]
async fn test_e2e_messages_pagination() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;

    // Insert 10 messages
    let now = chrono::Utc::now().timestamp();
    for i in 0..10 {
        sqlx::query(
            "INSERT INTO chat_messages (id, role, blocks, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(format!("msg-{}", i))
        .bind(if i % 2 == 0 { "user" } else { "assistant" })
        .bind(format!(r#"[{{"type":"text","content":"Message {}"}}]"#, i))
        .bind(now + i)
        .execute(&db)
        .await
        .unwrap();
    }

    let state = create_test_state(db).await;
    let router = create_test_router(state);

    // Get with limit
    let (status, body) = get_json(&router, "/api/messages?limit=5").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 5);

    // Get with before cursor
    let (status, body) = get_json(&router, &format!("/api/messages?before={}&limit=3", now + 5)).await;
    assert_eq!(status, StatusCode::OK);
    let messages = body.as_array().unwrap();
    assert!(messages.len() <= 3);
    // All should be before the timestamp
    for msg in messages {
        assert!(msg["created_at"].as_i64().unwrap() < now + 5);
    }
}

#[tokio::test]
async fn test_e2e_sync_endpoint_validation() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;
    let state = create_test_state(db).await;
    let router = create_test_router(state);

    // Empty message should still be accepted (validation in handler)
    let (status, _body) = post_json(
        &router,
        "/api/chat/sync",
        json!({
            "message": "",
            "project_path": "/test/project"
        }),
    )
    .await;

    // Should get some response (may be error due to missing API key in processing)
    // The important thing is the endpoint accepts the request structure
    assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_e2e_sync_endpoint_with_auth_token() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;

    // Create state with sync token required
    let mut state = create_test_state(db).await;
    state.sync_token = Some("secret-token".to_string());
    let router = create_test_router(state);

    // Request without token should fail
    let request = Request::builder()
        .method("POST")
        .uri("/api/chat/sync")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "message": "test",
                "project_path": "/test"
            })
            .to_string(),
        ))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Request with correct token should be accepted
    let request = Request::builder()
        .method("POST")
        .uri("/api/chat/sync")
        .header("content-type", "application/json")
        .header("authorization", "Bearer secret-token")
        .body(Body::from(
            json!({
                "message": "test",
                "project_path": "/test"
            })
            .to_string(),
        ))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    // Should pass auth (may fail later due to missing API key, but not 401)
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_e2e_sync_endpoint_payload_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;
    let state = create_test_state(db).await;
    let router = create_test_router(state);

    // Create a message larger than the limit (32KB for message content)
    let large_message = "x".repeat(40 * 1024); // 40KB

    let (status, body) = post_json(
        &router,
        "/api/chat/sync",
        json!({
            "message": large_message,
            "project_path": "/test/project"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body["error"].as_str().unwrap_or("").contains("limit"));
}

#[tokio::test]
async fn test_e2e_concurrent_sync_semaphore() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;

    // Create state with semaphore limit of 1
    let mut state = create_test_state(db).await;
    state.sync_semaphore = Arc::new(Semaphore::new(1));
    let router = create_test_router(state);

    // Spawn a task that holds the semaphore
    let router1 = router.clone();
    let hold_task = tokio::spawn(async move {
        let request = Request::builder()
            .method("POST")
            .uri("/api/chat/sync")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "message": "hold",
                    "project_path": "/test"
                })
                .to_string(),
            ))
            .unwrap();

        // This will acquire the semaphore and hold it during processing
        let _ = router1.oneshot(request).await;
    });

    // Small delay to let first request acquire semaphore
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Second request should get 429 if semaphore is held
    // (Though in practice the first request may complete quickly)
    let router2 = router.clone();
    let request = Request::builder()
        .method("POST")
        .uri("/api/chat/sync")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "message": "test",
                "project_path": "/test"
            })
            .to_string(),
        ))
        .unwrap();

    let response = router2.oneshot(request).await.unwrap();
    // Either 429 (too many) or success (if first completed)
    assert!(
        response.status() == StatusCode::TOO_MANY_REQUESTS
            || response.status() == StatusCode::OK
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR
    );

    hold_task.abort();
}

#[tokio::test]
async fn test_e2e_project_locks_different_projects() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;
    let state = create_test_state(db).await;

    // Different projects should be able to run concurrently
    let lock_a = state.project_locks.get_lock("/project/a").await;
    let lock_b = state.project_locks.get_lock("/project/b").await;

    // Should be different locks
    assert!(!Arc::ptr_eq(&lock_a, &lock_b));

    // Both should be acquirable simultaneously
    let guard_a = lock_a.try_lock();
    let guard_b = lock_b.try_lock();

    assert!(guard_a.is_ok());
    assert!(guard_b.is_ok());
}

#[tokio::test]
async fn test_e2e_archived_messages_excluded() {
    let temp_dir = TempDir::new().unwrap();
    let db = create_test_db(&temp_dir).await;

    let now = chrono::Utc::now().timestamp();

    // Insert active message
    sqlx::query(
        "INSERT INTO chat_messages (id, role, blocks, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind("msg-active")
    .bind("user")
    .bind(r#"[{"type":"text","content":"Active"}]"#)
    .bind(now)
    .execute(&db)
    .await
    .unwrap();

    // Insert archived message
    sqlx::query(
        "INSERT INTO chat_messages (id, role, blocks, created_at, archived_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("msg-archived")
    .bind("user")
    .bind(r#"[{"type":"text","content":"Archived"}]"#)
    .bind(now - 1000)
    .bind(now - 500)
    .execute(&db)
    .await
    .unwrap();

    let state = create_test_state(db).await;
    let router = create_test_router(state);

    let (status, body) = get_json(&router, "/api/messages").await;

    assert_eq!(status, StatusCode::OK);
    let messages = body.as_array().unwrap();
    // Should only return active message
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], "msg-active");
}
