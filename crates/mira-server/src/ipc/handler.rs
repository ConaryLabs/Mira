// crates/mira-server/src/ipc/handler.rs
// Per-connection handler for IPC requests

use super::protocol::{IpcRequest, IpcResponse};
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
        | "get_resume_context"
        | "distill_team_session" => Duration::from_secs(30),
        "recall_memories"
        | "get_active_goals"
        | "snapshot_tasks"
        | "write_claude_local_md"
        | "write_auto_memory" => Duration::from_secs(10),
        _ => Duration::from_secs(5),
    }
}

/// Handle a single IPC connection: loop reading request lines until EOF.
///
/// Hooks typically need 2-3 operations per invocation (e.g., resolve_project
/// then recall_memories), so we support multiple requests per connection.
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
        // Bounded line read: uses fill_buf/consume to reject lines exceeding
        // MAX_LINE_SIZE BEFORE allocating unbounded memory. Plain read_line
        // would buffer the entire line first, risking OOM on malicious input.
        let mut buf = String::new();
        let mut eof = false;
        let mut too_large = false;
        loop {
            let available = match reader.fill_buf().await {
                Ok([]) => {
                    eof = true;
                    break;
                }
                Ok(b) => b,
                Err(e) => {
                    let resp = IpcResponse::error(String::new(), format!("read error: {e}"));
                    let _ = write_response(&mut writer, &resp).await;
                    return;
                }
            };
            let newline_pos = available.iter().position(|&b| b == b'\n');
            let end = newline_pos.map(|p| p + 1).unwrap_or(available.len());
            if buf.len() + end > MAX_LINE_SIZE {
                too_large = true;
                // Drain the rest of this line so the connection stays usable
                let consume_len = end;
                reader.consume(consume_len);
                break;
            }
            // Safe: IPC sends JSON (valid UTF-8). Invalid bytes → error on parse.
            buf.push_str(&String::from_utf8_lossy(&available[..end]));
            reader.consume(end);
            if newline_pos.is_some() {
                break;
            }
        }
        if eof {
            break;
        }
        if too_large {
            let resp = IpcResponse::error(
                String::new(),
                format!("request too large (max {} bytes)", MAX_LINE_SIZE),
            );
            let _ = write_response(&mut writer, &resp).await;
            break;
        }

        let buf = buf.trim();
        if buf.is_empty() {
            continue;
        }

        // Parse request
        let req: IpcRequest = match serde_json::from_str(buf) {
            Ok(r) => r,
            Err(e) => {
                let resp = IpcResponse::error(String::new(), format!("parse error: {e}"));
                let _ = write_response(&mut writer, &resp).await;
                continue;
            }
        };

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
        "recall_memories" => super::ops::recall_memories(server, params).await,
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
        "write_claude_local_md" => super::ops::write_claude_local_md(server, params).await,
        "deactivate_team_session" => super::ops::deactivate_team_session(server, params).await,
        "write_auto_memory" => super::ops::write_auto_memory(server, params).await,
        "distill_team_session" => super::ops::distill_team_session(server, params).await,
        // Phase 4: UserPromptSubmit
        "get_user_prompt_context" => super::ops::get_user_prompt_context(server, params).await,
        // Memory staleness
        "mark_memories_stale" => super::ops::mark_memories_stale(server, params).await,
        _ => anyhow::bail!("unknown op: {op}"),
    }
}
