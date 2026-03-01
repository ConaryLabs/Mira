// crates/mira-server/src/ipc/tests.rs
// IPC tests — duplex-based (all platforms) + Unix socket integration tests

use crate::config::ApiKeys;
use crate::db::pool::{CodePool, DatabasePool, MainPool};
use crate::db::test_support::{setup_test_pool, setup_test_pool_with_project};
use crate::ipc::handler;
use crate::ipc::protocol::{IpcRequest, IpcResponse};
use crate::mcp::MiraServer;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers (all platforms)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a MiraServer for testing.
async fn create_test_server(pool: Arc<DatabasePool>) -> MiraServer {
    let code_pool = CodePool::new(Arc::new(
        DatabasePool::open_code_db_in_memory()
            .await
            .expect("failed to open code pool"),
    ));
    MiraServer::from_api_keys(
        MainPool::new(pool),
        code_pool,
        None,
        &ApiKeys::default(),
        false,
    )
}

/// Spawn a handler on an in-memory duplex stream. Returns the client half
/// (split into reader/writer) for sending requests. Works on all platforms.
async fn spawn_duplex_handler(
    pool: Arc<DatabasePool>,
) -> (
    BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<()>,
) {
    let server = create_test_server(pool).await;
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);

    let handle = tokio::spawn(async move {
        handler::handle_connection(server_stream, server).await;
    });

    let (read, write) = tokio::io::split(client_stream);
    (BufReader::new(read), write, handle)
}

/// Send an IpcRequest over a generic reader/writer and parse the response.
async fn send_request<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    req: &IpcRequest,
) -> IpcResponse
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = serde_json::to_string(req).unwrap();
    line.push('\n');
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();

    let mut buf = String::new();
    reader.read_line(&mut buf).await.unwrap();
    serde_json::from_str(&buf).expect("failed to parse IpcResponse")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Transport-agnostic tests (run on all platforms via duplex streams)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_duplex_unknown_op() {
    let pool = setup_test_pool().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "nonexistent_operation".into(),
        id: "unknown-1".into(),
        params: json!({}),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(!resp.ok, "unknown op should fail");
    assert_eq!(resp.id, "unknown-1");
    assert!(
        resp.error
            .as_ref()
            .map(|e| e.contains("unknown op"))
            .unwrap_or(false),
        "error should mention unknown op, got: {:?}",
        resp.error
    );

    handle.abort();
}

#[tokio::test]
async fn test_duplex_malformed_request() {
    let pool = setup_test_pool().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    // Send invalid JSON
    writer.write_all(b"this is not json\n").await.unwrap();
    writer.flush().await.unwrap();

    let mut buf = String::new();
    reader.read_line(&mut buf).await.unwrap();

    let resp: IpcResponse = serde_json::from_str(&buf).expect("should get a valid error response");
    assert!(!resp.ok, "malformed request should fail");
    assert!(
        resp.error
            .as_ref()
            .map(|e| e.contains("parse error"))
            .unwrap_or(false),
        "error should mention parse error, got: {:?}",
        resp.error
    );

    handle.abort();
}

#[tokio::test]
async fn test_duplex_multi_request() {
    let (pool, _project_id) = setup_test_pool_with_project().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    // First request
    let req1 = IpcRequest {
        op: "resolve_project".into(),
        id: "req-1".into(),
        params: json!({"cwd": "/test/path"}),
    };
    let resp1 = send_request(&mut reader, &mut writer, &req1).await;
    assert!(resp1.ok, "first request should succeed");
    assert_eq!(resp1.id, "req-1");

    // Second request on same connection
    let req2 = IpcRequest {
        op: "resolve_project".into(),
        id: "req-2".into(),
        params: json!({"cwd": "/test/path"}),
    };
    let resp2 = send_request(&mut reader, &mut writer, &req2).await;
    assert!(resp2.ok, "second request should succeed");
    assert_eq!(resp2.id, "req-2");

    let pid1 = resp1.result.as_ref().unwrap()["project_id"].as_i64();
    let pid2 = resp2.result.as_ref().unwrap()["project_id"].as_i64();
    assert_eq!(pid1, pid2, "same cwd should resolve to same project");

    handle.abort();
}

#[tokio::test]
async fn test_duplex_resolve_project() {
    let (pool, _project_id) = setup_test_pool_with_project().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "resolve_project".into(),
        id: "resolve-1".into(),
        params: json!({"cwd": "/test/path"}),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "resolve_project should succeed");
    let result = resp.result.as_ref().unwrap();
    assert!(result["project_id"].as_i64().unwrap() > 0);
    assert_eq!(result["path"].as_str().unwrap(), "/test/path");

    handle.abort();
}

#[tokio::test]
async fn test_duplex_log_behavior() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();

    let pid = project_id;
    pool.interact(move |conn| {
        crate::db::create_session_sync(conn, "test-session-duplex", Some(pid)).map_err(Into::into)
    })
    .await
    .unwrap();

    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "log_behavior".into(),
        id: "log-1".into(),
        params: json!({
            "session_id": "test-session-duplex",
            "project_id": project_id,
            "event_type": "tool_use",
            "event_data": {"tool_name": "Read"},
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "log_behavior should succeed");

    // Give it a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let count: i64 = pool_verify
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                conn.query_row(
                    "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = 'test-session-duplex'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0),
            )
        })
        .await
        .unwrap();
    assert!(count > 0, "log_behavior should have inserted a row");

    handle.abort();
}

#[tokio::test]
async fn test_duplex_log_behavior_empty_session_id() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "log_behavior".into(),
        id: "empty-sid".into(),
        params: json!({
            "session_id": "",
            "project_id": project_id,
            "event_type": "tool_use",
            "event_data": {"tool_name": "Read"},
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "empty session_id should succeed (early return)");

    let count: i64 = pool_verify
        .interact(|conn| {
            Ok::<_, anyhow::Error>(
                conn.query_row("SELECT COUNT(*) FROM session_behavior_log", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0),
            )
        })
        .await
        .unwrap();
    assert_eq!(count, 0, "no rows should be logged for empty session_id");

    handle.abort();
}

#[tokio::test]
async fn test_duplex_snapshot_tasks_too_many() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let big_tasks: Vec<serde_json::Value> = (0..10_001)
        .map(|i| {
            json!({
                "id": format!("task-{i}"),
                "subject": format!("task {i}"),
                "status": "pending",
            })
        })
        .collect();

    let req = IpcRequest {
        op: "snapshot_tasks".into(),
        id: "too-many".into(),
        params: json!({
            "project_id": project_id,
            "list_id": "test-list",
            "tasks": big_tasks,
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(!resp.ok, "10001 tasks should be rejected");
    assert!(
        resp.error
            .as_ref()
            .map(|e| e.contains("too many tasks"))
            .unwrap_or(false),
        "error should mention 'too many tasks', got: {:?}",
        resp.error
    );

    handle.abort();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Bundle generation tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a test server with a pre-seeded code index.
async fn create_test_server_with_code(
    pool: Arc<DatabasePool>,
    code_pool: Arc<DatabasePool>,
) -> MiraServer {
    MiraServer::from_api_keys(
        MainPool::new(pool),
        CodePool::new(code_pool),
        None,
        &ApiKeys::default(),
        false,
    )
}

/// Seed the code database with test modules and symbols.
async fn seed_code_index(code_pool: &DatabasePool, project_id: i64) {
    code_pool
        .interact(move |conn| {
            // Insert a module
            conn.execute(
                "INSERT INTO codebase_modules (project_id, module_id, name, path, purpose, exports, symbol_count, line_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    project_id,
                    "src/hooks/",
                    "hooks",
                    "src/hooks/",
                    "Hook handlers for Claude Code events",
                    r#"["run_start","run_stop"]"#,
                    3,
                    200
                ],
            )?;
            conn.execute(
                "INSERT INTO codebase_modules (project_id, module_id, name, path, purpose, exports, symbol_count, line_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    project_id,
                    "src/hooks/subagent",
                    "subagent",
                    "src/hooks/subagent.rs",
                    "SubagentStart and SubagentStop hook handlers",
                    r#"["run_start","run_stop"]"#,
                    5,
                    400
                ],
            )?;
            // Insert symbols
            conn.execute(
                "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, signature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    project_id,
                    "src/hooks/subagent.rs",
                    "run_start",
                    "function",
                    100,
                    "pub async fn run_start() -> Result<()>"
                ],
            )?;
            conn.execute(
                "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, signature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    project_id,
                    "src/hooks/subagent.rs",
                    "extract_scopes_from_prompt",
                    "function",
                    460,
                    "fn extract_scopes_from_prompt(prompt: &str) -> Vec<String>"
                ],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .expect("failed to seed code index");
}

/// Spawn a duplex handler with a pre-seeded code index.
async fn spawn_duplex_handler_with_code(
    pool: Arc<DatabasePool>,
    code_pool: Arc<DatabasePool>,
) -> (
    BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<()>,
) {
    let server = create_test_server_with_code(pool, code_pool).await;
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
    let handle = tokio::spawn(async move {
        handler::handle_connection(server_stream, server).await;
    });
    let (read, write) = tokio::io::split(client_stream);
    (BufReader::new(read), write, handle)
}

#[tokio::test]
async fn test_duplex_generate_bundle_with_data() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let code_pool = Arc::new(
        DatabasePool::open_code_db_in_memory()
            .await
            .expect("failed to open code pool"),
    );
    seed_code_index(&code_pool, project_id).await;

    let (mut reader, mut writer, handle) = spawn_duplex_handler_with_code(pool, code_pool).await;

    let req = IpcRequest {
        op: "generate_bundle".into(),
        id: "bundle-1".into(),
        params: json!({
            "project_id": project_id,
            "scope": "src/hooks/",
            "budget": 3000,
            "depth": "overview",
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "generate_bundle should succeed: {:?}", resp.error);

    let result = resp.result.as_ref().unwrap();
    assert!(
        !result["empty"].as_bool().unwrap_or(true),
        "should not be empty"
    );
    assert!(
        result["modules"].as_u64().unwrap() >= 2,
        "should find 2 modules"
    );
    assert!(
        result["symbols"].as_u64().unwrap() >= 2,
        "should find 2 symbols"
    );

    let content = result["content"].as_str().unwrap();
    assert!(
        content.contains("hooks"),
        "bundle should mention module name"
    );
    assert!(
        content.contains("run_start"),
        "bundle should include symbol names"
    );

    handle.abort();
}

#[tokio::test]
async fn test_duplex_generate_bundle_empty_scope() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let code_pool = Arc::new(
        DatabasePool::open_code_db_in_memory()
            .await
            .expect("failed to open code pool"),
    );

    let (mut reader, mut writer, handle) = spawn_duplex_handler_with_code(pool, code_pool).await;

    // Query a scope that has no indexed data
    let req = IpcRequest {
        op: "generate_bundle".into(),
        id: "bundle-empty".into(),
        params: json!({
            "project_id": project_id,
            "scope": "nonexistent/path/",
            "budget": 3000,
            "depth": "overview",
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "generate_bundle with empty result should succeed");

    let result = resp.result.as_ref().unwrap();
    assert!(
        result["empty"].as_bool().unwrap_or(false),
        "should be empty for nonexistent scope"
    );

    handle.abort();
}

#[tokio::test]
async fn test_duplex_generate_bundle_missing_params() {
    let (pool, _project_id) = setup_test_pool_with_project().await;
    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    // Missing project_id
    let req = IpcRequest {
        op: "generate_bundle".into(),
        id: "bundle-bad".into(),
        params: json!({"scope": "src/"}),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(!resp.ok, "should fail without project_id");

    // Missing scope
    let req2 = IpcRequest {
        op: "generate_bundle".into(),
        id: "bundle-bad-2".into(),
        params: json!({"project_id": 1}),
    };
    let resp2 = send_request(&mut reader, &mut writer, &req2).await;
    assert!(!resp2.ok, "should fail without scope");

    handle.abort();
}

// ═══════════════════════════════════════════════════════════════════════════════
// save_compaction_context tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_save_compaction_context_basic() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();

    // Create a session so the FK constraint is satisfied
    pool.interact(move |conn| {
        crate::db::create_session_sync(conn, "ctx-basic", Some(project_id)).map_err(Into::into)
    })
    .await
    .unwrap();

    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "save_compaction_context".into(),
        id: "ctx-1".into(),
        params: json!({
            "session_id": "ctx-basic",
            "context": {
                "decisions": ["use SQLite for storage"],
                "active_work": ["implementing compaction"],
                "issues": [],
                "pending_tasks": [],
                "user_intent": "add compaction support"
            }
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(
        resp.ok,
        "save_compaction_context should succeed: {:?}",
        resp.error
    );

    // Verify the snapshot was stored correctly
    let snapshot_str: String = pool_verify
        .interact(|conn| {
            conn.query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = 'ctx-basic'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();
    let cc = &snapshot["compaction_context"];
    assert_eq!(
        cc["decisions"].as_array().unwrap(),
        &vec![json!("use SQLite for storage")],
    );
    assert_eq!(
        cc["active_work"].as_array().unwrap(),
        &vec![json!("implementing compaction")],
    );
    assert_eq!(
        cc["user_intent"].as_str().unwrap(),
        "add compaction support",
    );

    handle.abort();
}

#[tokio::test]
async fn test_save_compaction_context_merges_on_second_call() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();

    pool.interact(move |conn| {
        crate::db::create_session_sync(conn, "ctx-merge", Some(project_id)).map_err(Into::into)
    })
    .await
    .unwrap();

    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    // First call -- seed with initial context
    let req1 = IpcRequest {
        op: "save_compaction_context".into(),
        id: "merge-1".into(),
        params: json!({
            "session_id": "ctx-merge",
            "context": {
                "decisions": ["decision A"],
                "active_work": ["work A"],
                "issues": ["issue A"],
                "pending_tasks": [],
                "user_intent": "original intent"
            }
        }),
    };
    let resp1 = send_request(&mut reader, &mut writer, &req1).await;
    assert!(resp1.ok, "first save should succeed: {:?}", resp1.error);

    // Second call -- should merge, not overwrite
    let req2 = IpcRequest {
        op: "save_compaction_context".into(),
        id: "merge-2".into(),
        params: json!({
            "session_id": "ctx-merge",
            "context": {
                "decisions": ["decision B"],
                "active_work": ["work B"],
                "issues": [],
                "pending_tasks": ["task B"],
                "user_intent": "new intent"
            }
        }),
    };
    let resp2 = send_request(&mut reader, &mut writer, &req2).await;
    assert!(resp2.ok, "second save should succeed: {:?}", resp2.error);

    // Verify merge behavior
    let snapshot_str: String = pool_verify
        .interact(|conn| {
            conn.query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = 'ctx-merge'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();
    let cc = &snapshot["compaction_context"];

    // Decisions should contain both A and B (merged, not overwritten)
    let decisions: Vec<&str> = cc["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(decisions.contains(&"decision A"), "should keep decision A");
    assert!(decisions.contains(&"decision B"), "should add decision B");

    // active_work should contain both
    let active_work: Vec<&str> = cc["active_work"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(active_work.contains(&"work A"), "should keep work A");
    assert!(active_work.contains(&"work B"), "should add work B");

    // issues: first had "issue A", second had none -- merged result keeps "issue A"
    let issues: Vec<&str> = cc["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        issues.contains(&"issue A"),
        "should keep issue A from first call"
    );

    // pending_tasks: first had none, second had "task B"
    let tasks: Vec<&str> = cc["pending_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        tasks.contains(&"task B"),
        "should add task B from second call"
    );

    // user_intent: merge keeps the FIRST one (original intent)
    assert_eq!(
        cc["user_intent"].as_str().unwrap(),
        "original intent",
        "user_intent should preserve the first/original value, not overwrite"
    );

    handle.abort();
}

#[tokio::test]
async fn test_save_compaction_context_preserves_other_snapshot_fields() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();

    pool.interact(move |conn| {
        crate::db::create_session_sync(conn, "ctx-preserve", Some(project_id)).map_err(Into::into)
    })
    .await
    .unwrap();

    // Pre-seed a snapshot with other fields (tool_count, custom_field)
    pool_verify
        .interact(|conn| {
            let snapshot = json!({
                "tool_count": 42,
                "custom_field": "should survive",
                "nested": {"key": "value"}
            });
            conn.execute(
                "INSERT INTO session_snapshots (session_id, snapshot, created_at)
                 VALUES ('ctx-preserve', ?1, datetime('now'))",
                rusqlite::params![serde_json::to_string(&snapshot).unwrap()],
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await
        .unwrap();

    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool_verify.clone()).await;

    let req = IpcRequest {
        op: "save_compaction_context".into(),
        id: "preserve-1".into(),
        params: json!({
            "session_id": "ctx-preserve",
            "context": {
                "decisions": ["keep other fields"],
                "active_work": [],
                "issues": [],
                "pending_tasks": []
            }
        }),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(
        resp.ok,
        "save_compaction_context should succeed: {:?}",
        resp.error
    );

    // Verify the pre-existing fields survived
    let snapshot_str: String = pool_verify
        .interact(|conn| {
            conn.query_row(
                "SELECT snapshot FROM session_snapshots WHERE session_id = 'ctx-preserve'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_str).unwrap();

    // Original fields must still be present
    assert_eq!(
        snapshot["tool_count"].as_i64().unwrap(),
        42,
        "tool_count should survive compaction context save"
    );
    assert_eq!(
        snapshot["custom_field"].as_str().unwrap(),
        "should survive",
        "custom_field should survive compaction context save"
    );
    assert_eq!(
        snapshot["nested"]["key"].as_str().unwrap(),
        "value",
        "nested fields should survive compaction context save"
    );

    // And the compaction_context should be present too
    let cc = &snapshot["compaction_context"];
    assert_eq!(
        cc["decisions"].as_array().unwrap(),
        &vec![json!("keep other fields")],
    );

    handle.abort();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unix socket integration tests (Unix only — tests real transport)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(unix)]
mod unix_tests {
    use super::*;
    use crate::ipc::client::HookClient;
    use std::path::PathBuf;
    use tokio::net::{UnixListener, UnixStream};

    /// Spawn a test IPC server on a temporary Unix socket.
    async fn spawn_test_server(pool: Arc<DatabasePool>) -> (PathBuf, tokio::task::JoinHandle<()>) {
        let sock_path =
            std::env::temp_dir().join(format!("mira-test-{}.sock", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path).expect("failed to bind test socket");
        let server = create_test_server(pool).await;

        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let srv = server.clone();
                tokio::spawn(async move {
                    handler::handle_connection(stream, srv).await;
                });
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        (sock_path, handle)
    }

    #[tokio::test]
    async fn test_ipc_connect() {
        let pool = setup_test_pool().await;
        let (sock_path, handle) = spawn_test_server(pool).await;

        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let client = HookClient::from_stream(stream);
        assert!(client.is_ipc());

        handle.abort();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn test_resolve_project() {
        let (pool, _project_id) = setup_test_pool_with_project().await;
        let (sock_path, handle) = spawn_test_server(pool).await;

        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let mut client = HookClient::from_stream(stream);

        let result = client.resolve_project(Some("/test/path"), None).await;
        assert!(result.is_some(), "resolve_project should return a result");
        let (id, path) = result.unwrap();
        assert!(id > 0);
        assert_eq!(path, "/test/path");

        handle.abort();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn test_concurrent_connections() {
        let (pool, project_id) = setup_test_pool_with_project().await;
        let (sock_path, handle) = spawn_test_server(pool).await;

        let make_client = |idx: usize, path: PathBuf| async move {
            let stream = UnixStream::connect(&path).await.unwrap();
            let (read, write) = tokio::io::split(stream);
            let mut reader = BufReader::new(read);
            let mut writer = write;

            let req = IpcRequest {
                op: "get_active_goals".into(),
                id: format!("concurrent-{idx}"),
                params: json!({"project_id": project_id, "limit": 5}),
            };
            send_request(&mut reader, &mut writer, &req).await
        };

        let (r0, r1, r2) = tokio::join!(
            make_client(0, sock_path.clone()),
            make_client(1, sock_path.clone()),
            make_client(2, sock_path.clone()),
        );

        for (i, resp) in [&r0, &r1, &r2].iter().enumerate() {
            assert!(
                resp.ok,
                "concurrent request {i} should succeed: {:?}",
                resp.error
            );
            assert_eq!(resp.id, format!("concurrent-{i}"));
        }

        for resp in [&r0, &r1, &r2] {
            let result = resp.result.as_ref().unwrap();
            assert!(
                result.get("goals").is_some(),
                "response should contain goals key"
            );
        }

        handle.abort();
        let _ = std::fs::remove_file(&sock_path);
    }
}
