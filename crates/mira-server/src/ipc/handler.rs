// crates/mira-server/src/ipc/handler.rs
// Per-connection handler for IPC requests

use super::protocol::{InjectionStatsSnapshot, IpcPushEvent, IpcRequest, IpcResponse, SessionStateSnapshot};
use crate::mcp::MiraServer;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// Maximum size of a single IPC request line (1 MB).
/// Prevents OOM from malicious or buggy clients sending unbounded data.
const MAX_LINE_SIZE: usize = 1_048_576;

/// Returns a per-operation timeout. Slow ops (LLM-dependent, multi-query) get 30s,
/// medium ops (search/recall) get 10s, fast ops (simple lookups/writes) get 5s.
fn op_timeout(op: &str) -> Duration {
    match op {
        "get_user_prompt_context"
        | "close_session"
        | "get_startup_context"
        | "get_resume_context" => Duration::from_secs(30),
        "get_active_goals" | "snapshot_tasks" => Duration::from_secs(10),
        "generate_bundle" => Duration::from_secs(4),
        "get_project_map" => Duration::from_secs(2),
        "search_for_subagent" => Duration::from_secs(3),
        _ => Duration::from_secs(5),
    }
}

/// Read a single NDJSON line from the reader with bounded size.
/// Returns Ok(Some(line)) on success, Ok(None) on EOF, Err on read error or too-large.
async fn read_request_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> std::io::Result<Option<String>> {
    let mut buf = String::new();
    loop {
        let available = match reader.fill_buf().await {
            Ok([]) => return Ok(None), // EOF
            Ok(b) => b,
            Err(e) => return Err(e),
        };
        let newline_pos = available.iter().position(|&b| b == b'\n');
        let end = newline_pos.map(|p| p + 1).unwrap_or(available.len());
        if buf.len() + end > MAX_LINE_SIZE {
            reader.consume(end);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("request too large (max {} bytes)", MAX_LINE_SIZE),
            ));
        }
        buf.push_str(&String::from_utf8_lossy(&available[..end]));
        reader.consume(end);
        if newline_pos.is_some() {
            let trimmed = buf.trim().to_string();
            return if trimmed.is_empty() {
                buf.clear();
                continue;
            } else {
                Ok(Some(trimmed))
            };
        }
    }
}

/// Handle a single IPC connection: loop reading request lines until EOF.
///
/// Hooks typically need 2-3 operations per invocation (e.g., resolve_project
/// then log_behavior), so we support multiple requests per connection.
/// The client closes the connection when done (sends EOF).
///
/// Generic over the stream type to support different transports (Unix sockets,
/// Named Pipes, etc.) — any `AsyncRead + AsyncWrite` works.
pub async fn handle_connection<S>(stream: S, server: MiraServer)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut writer = writer;

    loop {
        let line = match read_request_line(&mut reader).await {
            Ok(Some(line)) => line,
            Ok(None) => break, // EOF
            Err(e) => {
                let resp = IpcResponse::error(String::new(), format!("read error: {e}"));
                let _ = write_response(&mut writer, &resp).await;
                break;
            }
        };

        // Parse request
        let req: IpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = IpcResponse::error(String::new(), format!("parse error: {e}"));
                let _ = write_response(&mut writer, &resp).await;
                continue;
            }
        };

        // Handle subscribe: switch to persistent mode
        if req.op == "subscribe" {
            let session_id = req.params["session_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();

            if session_id.is_empty() {
                let resp = IpcResponse::error(req.id, "session_id required for subscribe");
                let _ = write_response(&mut writer, &resp).await;
                continue;
            }

            // Build and send initial snapshot
            let snapshot = build_session_snapshot(&server, &session_id).await;
            let resp = IpcResponse::success(
                req.id,
                serde_json::to_value(&snapshot).unwrap_or_default(),
            );
            if write_response(&mut writer, &resp).await.is_err() {
                break;
            }

            // Create channel and subscribe
            let (tx, mut rx) = tokio::sync::mpsc::channel::<IpcPushEvent>(64);
            server.channels.subscribe(&session_id, tx).await;

            // Enter persistent mode
            handle_persistent_connection(&mut reader, &mut writer, &server, &mut rx).await;

            server.channels.unsubscribe(&session_id).await;
            return; // Connection done after persistent mode
        }

        let id = req.id.clone();

        // Dispatch with per-op timeout
        let resp =
            match tokio::time::timeout(op_timeout(&req.op), dispatch(&req.op, req.params, &server))
                .await
            {
                Ok(Ok(result)) => IpcResponse::success(id, result),
                Ok(Err(e)) => IpcResponse::error(id, e.to_string()),
                Err(_) => IpcResponse::error(id, "timeout"),
            };

        if write_response(&mut writer, &resp).await.is_err() {
            break; // Client disconnected
        }
    }
}

/// Handle a persistent subscription connection.
/// Multiplexes push events (server -> client) and interleaved request-response queries.
async fn handle_persistent_connection<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    server: &MiraServer,
    rx: &mut tokio::sync::mpsc::Receiver<IpcPushEvent>,
) where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    loop {
        tokio::select! {
            // Push events from server -> client
            Some(event) = rx.recv() => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if writer.write_all(format!("{json}\n").as_bytes()).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
            // Interleaved requests from client -> server
            result = read_request_line(reader) => {
                match result {
                    Ok(Some(line)) => {
                        let req: IpcRequest = match serde_json::from_str(&line) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };
                        let id = req.id.clone();
                        let timeout = op_timeout(&req.op);
                        let result = tokio::time::timeout(
                            timeout,
                            dispatch(&req.op, req.params, server),
                        ).await;
                        let resp = match result {
                            Ok(Ok(val)) => IpcResponse::success(id, val),
                            Ok(Err(e)) => IpcResponse::error(id, e.to_string()),
                            Err(_) => IpcResponse::error(id, "timeout"),
                        };
                        if write_response(writer, &resp).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // EOF
                    Err(_) => break,    // Read error
                }
            }
        }
    }
}

/// Build an initial state snapshot for a new subscription.
async fn build_session_snapshot(
    _server: &MiraServer,
    _session_id: &str,
) -> SessionStateSnapshot {
    // Phase 1: return empty snapshot. Phase 2 will populate from DB.
    SessionStateSnapshot {
        sequence: 0,
        goals: vec![],
        injection_stats: InjectionStatsSnapshot::default(),
        modified_files: vec![],
        team_conflicts: vec![],
    }
}

async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    resp: &IpcResponse,
) -> std::io::Result<()> {
    let mut json = serde_json::to_string(resp)
        .unwrap_or_else(|_| r#"{"id":"","ok":false,"error":"serialize error"}"#.to_string());
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    writer.flush().await
}

async fn dispatch(
    op: &str,
    params: serde_json::Value,
    server: &MiraServer,
) -> anyhow::Result<serde_json::Value> {
    match op {
        "resolve_project" => super::ops::resolve_project(server, params).await,
        "log_behavior" => super::ops::log_behavior(server, params).await,
        "store_observation" => super::ops::store_observation(server, params).await,
        "get_active_goals" => super::ops::get_active_goals(server, params).await,
        "store_error_pattern" => super::ops::store_error_pattern(server, params).await,
        "lookup_resolved_pattern" => super::ops::lookup_resolved_pattern(server, params).await,
        "count_session_failures" => super::ops::count_session_failures(server, params).await,
        "resolve_error_patterns" => super::ops::resolve_error_patterns(server, params).await,
        "get_team_membership" => super::ops::get_team_membership(server, params).await,
        "record_file_ownership" => super::ops::record_file_ownership(server, params).await,
        "get_file_conflicts" => super::ops::get_file_conflicts(server, params).await,
        "auto_link_milestone" => super::ops::auto_link_milestone(server, params).await,
        "save_compaction_context" => super::ops::save_compaction_context(server, params).await,
        // Phase 3: Session lifecycle & stop
        "register_session" => super::ops::register_session(server, params).await,
        "register_team_session" => super::ops::register_team_session(server, params).await,
        "get_startup_context" => super::ops::get_startup_context(server, params).await,
        "get_resume_context" => super::ops::get_resume_context(server, params).await,
        "close_session" => super::ops::close_session(server, params).await,
        "snapshot_tasks" => super::ops::snapshot_tasks(server, params).await,
        "deactivate_team_session" => super::ops::deactivate_team_session(server, params).await,
        "generate_bundle" => super::ops::generate_bundle(server, params).await,
        "get_project_map" => super::ops::get_project_map(server, params).await,
        "search_for_subagent" => super::ops::search_for_subagent(server, params).await,
        // Phase 4: UserPromptSubmit
        "get_user_prompt_context" => super::ops::get_user_prompt_context(server, params).await,
        _ => anyhow::bail!("unknown op: {op}"),
    }
}
