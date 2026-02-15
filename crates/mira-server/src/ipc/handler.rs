// crates/mira-server/src/ipc/handler.rs
// Per-connection handler for IPC requests

use super::protocol::{IpcRequest, IpcResponse};
use crate::mcp::MiraServer;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
pub async fn handle_connection(stream: tokio::net::UnixStream, server: MiraServer) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    loop {
        let mut buf = String::new();
        match reader.read_line(&mut buf).await {
            Ok(0) => break, // EOF — client closed connection
            Ok(_) => {}
            Err(e) => {
                let resp = IpcResponse::error(String::new(), format!("read error: {e}"));
                let _ = write_response(&mut writer, &resp).await;
                break;
            }
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

async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
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
        "get_permission_rules" => super::ops::get_permission_rules(server, params).await,
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
        _ => anyhow::bail!("unknown op: {op}"),
    }
}
