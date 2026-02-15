// crates/mira-server/src/ipc/tests.rs
// Integration tests for Unix socket IPC between hooks and server

use crate::config::ApiKeys;
use crate::db::pool::DatabasePool;
use crate::db::test_support::{setup_test_pool, setup_test_pool_with_project};
use crate::ipc::client::HookClient;
use crate::ipc::handler;
use crate::ipc::protocol::{IpcRequest, IpcResponse};
use crate::mcp::MiraServer;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// Spawn a test IPC server on a temporary Unix socket.
/// Returns the socket path (caller must clean up) and a handle to the server task.
async fn spawn_test_server(pool: Arc<DatabasePool>) -> (PathBuf, tokio::task::JoinHandle<()>) {
    let sock_path = std::env::temp_dir().join(format!("mira-test-{}.sock", uuid::Uuid::new_v4()));

    // Clean up stale socket if it exists
    let _ = std::fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path).expect("failed to bind test socket");

    let code_pool = Arc::new(
        DatabasePool::open_code_db_in_memory()
            .await
            .expect("failed to open code pool"),
    );
    let server = MiraServer::from_api_keys(pool, code_pool, None, &ApiKeys::default(), false);

    let handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let srv = server.clone();
                    tokio::spawn(async move {
                        handler::handle_connection(stream, srv).await;
                    });
                }
                Err(_) => break,
            }
        }
    });

    // Give the listener a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    (sock_path, handle)
}

/// Send an IpcRequest and read back the response using a split stream.
async fn send_request(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    req: &IpcRequest,
) -> IpcResponse {
    let mut line = serde_json::to_string(req).unwrap();
    line.push('\n');
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();

    let mut buf = String::new();
    reader.read_line(&mut buf).await.unwrap();
    serde_json::from_str(&buf).expect("failed to parse IpcResponse")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

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

    // resolve_project with the known CWD
    let result = client.resolve_project(Some("/test/path")).await;
    assert!(result.is_some(), "resolve_project should return a result");
    let (id, path) = result.unwrap();
    assert!(id > 0);
    assert_eq!(path, "/test/path");

    handle.abort();
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_recall_memories() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    // Store a memory fact directly in the pool and mark it as confirmed
    // (recall_memories only returns confirmed memories)
    let pid = project_id;
    pool.interact(move |conn| {
        let id = crate::db::test_support::store_memory_helper(
            conn,
            Some(pid),
            None,
            "Always use the builder pattern for config structs",
            "decision",
            Some("patterns"),
            0.9,
        )?;
        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed' WHERE id = ?",
            [id],
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .unwrap();

    let (sock_path, handle) = spawn_test_server(pool).await;
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let mut client = HookClient::from_stream(stream);

    let memories = client.recall_memories(project_id, "builder pattern").await;
    // recall_memories uses keyword fallback (no embeddings), should find our memory
    assert!(
        !memories.is_empty(),
        "recall_memories should return at least one result"
    );
    let joined = memories.join(" ");
    assert!(
        joined.contains("builder pattern"),
        "result should contain the stored memory content, got: {joined}"
    );

    handle.abort();
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_log_behavior() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();

    // Create a session first (log_behavior needs a valid session)
    let pid = project_id;
    pool.interact(move |conn| {
        crate::db::create_session_sync(conn, "test-session-log", Some(pid)).map_err(Into::into)
    })
    .await
    .unwrap();

    let (sock_path, handle) = spawn_test_server(pool).await;
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let mut client = HookClient::from_stream(stream);

    // Fire-and-forget — should not error
    client
        .log_behavior(
            "test-session-log",
            project_id,
            "tool_use",
            json!({"tool_name": "Read"}),
        )
        .await;

    // Give it a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify the behavior was logged
    let count: i64 = pool_verify
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                conn.query_row(
                    "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = 'test-session-log'",
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
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_malformed_request() {
    let pool = setup_test_pool().await;
    let (sock_path, handle) = spawn_test_server(pool).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);

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
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_multi_request_connection() {
    let (pool, _project_id) = setup_test_pool_with_project().await;
    let (sock_path, handle) = spawn_test_server(pool).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);

    // First request: resolve_project
    let req1 = IpcRequest {
        op: "resolve_project".into(),
        id: "req-1".into(),
        params: json!({"cwd": "/test/path"}),
    };
    let resp1 = send_request(&mut reader, &mut writer, &req1).await;
    assert!(resp1.ok, "first request should succeed");
    assert_eq!(resp1.id, "req-1");

    // Second request on same connection: resolve_project again
    let req2 = IpcRequest {
        op: "resolve_project".into(),
        id: "req-2".into(),
        params: json!({"cwd": "/test/path"}),
    };
    let resp2 = send_request(&mut reader, &mut writer, &req2).await;
    assert!(resp2.ok, "second request should succeed");
    assert_eq!(resp2.id, "req-2");

    // Both should return the same project
    let pid1 = resp1.result.as_ref().unwrap()["project_id"].as_i64();
    let pid2 = resp2.result.as_ref().unwrap()["project_id"].as_i64();
    assert_eq!(pid1, pid2, "same cwd should resolve to same project");

    handle.abort();
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_concurrent_connections() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let (sock_path, handle) = spawn_test_server(pool).await;

    // Open 3 separate connections and send requests concurrently.
    // The project already exists (setup_test_pool_with_project), so concurrent
    // resolve_project calls only read — no SQLite write contention.
    let make_client = |idx: usize, path: PathBuf| async move {
        let stream = UnixStream::connect(&path).await.unwrap();
        let (read, mut writer) = stream.into_split();
        let mut reader = BufReader::new(read);

        // Use get_active_goals — a read-only op that won't trigger writes
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

    // All should return a goals array (possibly empty)
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

#[tokio::test]
async fn test_unknown_op() {
    let pool = setup_test_pool().await;
    let (sock_path, handle) = spawn_test_server(pool).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);

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
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_log_behavior_empty_session_id() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let pool_verify = pool.clone();
    let (sock_path, handle) = spawn_test_server(pool).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);

    // Send log_behavior with empty session_id — should succeed but be a no-op
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

    // Also test with missing session_id entirely (defaults to "")
    let req2 = IpcRequest {
        op: "log_behavior".into(),
        id: "missing-sid".into(),
        params: json!({
            "project_id": project_id,
            "event_type": "tool_use",
            "event_data": {},
        }),
    };
    let resp2 = send_request(&mut reader, &mut writer, &req2).await;
    assert!(resp2.ok, "missing session_id should succeed (early return)");

    // Verify nothing was logged
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
    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_snapshot_tasks_too_many() {
    let (pool, project_id) = setup_test_pool_with_project().await;
    let (sock_path, handle) = spawn_test_server(pool).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);

    // Build an array with 10_001 tasks
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
    let _ = std::fs::remove_file(&sock_path);
}
