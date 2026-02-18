// crates/mira-server/src/ipc/tests.rs
// IPC tests — duplex-based (all platforms) + Unix socket integration tests

use crate::config::ApiKeys;
use crate::db::pool::DatabasePool;
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
    let code_pool = Arc::new(
        DatabasePool::open_code_db_in_memory()
            .await
            .expect("failed to open code pool"),
    );
    MiraServer::from_api_keys(pool, code_pool, None, &ApiKeys::default(), false)
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
async fn send_request<R, W>(reader: &mut BufReader<R>, writer: &mut W, req: &IpcRequest) -> IpcResponse
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
async fn test_duplex_recall_memories() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    // Store a confirmed memory
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

    let (mut reader, mut writer, handle) = spawn_duplex_handler(pool).await;

    let req = IpcRequest {
        op: "recall_memories".into(),
        id: "recall-1".into(),
        params: json!({"project_id": project_id, "query": "builder pattern"}),
    };
    let resp = send_request(&mut reader, &mut writer, &req).await;
    assert!(resp.ok, "recall_memories should succeed");
    let memories = resp.result.as_ref().unwrap()["memories"]
        .as_array()
        .unwrap();
    assert!(
        !memories.is_empty(),
        "recall_memories should return at least one result"
    );

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

        let result = client.resolve_project(Some("/test/path")).await;
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
