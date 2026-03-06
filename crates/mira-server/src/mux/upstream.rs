// crates/mira-server/src/mux/upstream.rs
// Upstream persistent connection to the MCP server

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, RwLock, oneshot, watch};

use super::state::SessionState;
use crate::ipc::protocol::{IpcPushEvent, IpcRequest, IpcResponse, SessionStateSnapshot};

/// Pending request map: request ID -> oneshot sender for the response.
pub type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>>;

/// Max line size for upstream reads (1 MB).
const MAX_LINE_SIZE: usize = 1_048_576;

/// Connect to mira.sock, send subscribe, receive snapshot, spawn reader task.
/// Returns the write half (for proxying queries) and the pending requests map.
pub async fn connect_and_subscribe(
    session_id: &str,
    state: Arc<RwLock<SessionState>>,
    shutdown_tx: watch::Sender<bool>,
) -> anyhow::Result<(tokio::io::WriteHalf<UnixStream>, PendingRequests)> {
    let socket_path = crate::ipc::socket_path();
    let stream = UnixStream::connect(&socket_path).await?;
    let (read_half, mut write_half) = tokio::io::split(stream);

    // Send subscribe request
    let req = IpcRequest {
        op: "subscribe".to_string(),
        id: uuid::Uuid::new_v4().to_string(),
        params: serde_json::json!({ "session_id": session_id }),
    };
    let json = serde_json::to_string(&req)?;
    write_half
        .write_all(format!("{json}\n").as_bytes())
        .await?;
    write_half.flush().await?;

    // Read snapshot response
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let resp: IpcResponse = serde_json::from_str(line.trim())?;

    if !resp.ok {
        anyhow::bail!(
            "subscribe failed: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
    }

    let snapshot: SessionStateSnapshot =
        serde_json::from_value(resp.result.unwrap_or_default())?;

    {
        let mut s = state.write().await;
        *s = SessionState::from_snapshot(snapshot);
    }

    // Shared pending requests map for request-response correlation
    let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
    let pending_clone = pending.clone();

    // Spawn reader task: route push events to state, responses to pending map.
    // Signals shutdown when upstream connection dies.
    tokio::spawn(async move {
        let mut reader = reader;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF - server closed connection
                Ok(_) => {
                    if line.len() > MAX_LINE_SIZE {
                        continue;
                    }
                    let trimmed = line.trim();
                    // Try as IpcResponse first (has "id" and "ok" fields)
                    if let Ok(resp) = serde_json::from_str::<IpcResponse>(trimmed) {
                        let mut map = pending_clone.lock().await;
                        if let Some(tx) = map.remove(&resp.id) {
                            let _ = tx.send(resp);
                        }
                    } else if let Ok(event) = serde_json::from_str::<IpcPushEvent>(trimmed) {
                        let mut s = state.write().await;
                        s.apply_event(&event);
                    }
                    // Ignore unparseable lines
                }
                Err(_) => break, // Read error
            }
        }
        eprintln!("[mira/mux] Upstream connection closed, signaling shutdown");
        let _ = shutdown_tx.send(true);
    });

    Ok((write_half, pending))
}
