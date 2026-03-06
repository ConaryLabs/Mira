// crates/mira-server/src/mux/local.rs
// Local socket server for hook connections

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, RwLock, watch};
use tokio::time::Instant;

use super::state::SessionState;
use super::upstream::PendingRequests;
use crate::ipc::protocol::{IpcRequest, IpcResponse};

/// Max line size for local connections (1 MB).
const MAX_LINE_SIZE: usize = 1_048_576;

/// Serve local hook connections on mux.sock.
pub async fn serve(
    sock_path: PathBuf,
    state: Arc<RwLock<SessionState>>,
    upstream_writer: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    pending_requests: PendingRequests,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    // Clean stale socket
    let _ = std::fs::remove_file(&sock_path);
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Bind with restrictive permissions
    let old_umask = unsafe { libc::umask(0o177) };
    let bind_result = UnixListener::bind(&sock_path);
    unsafe { libc::umask(old_umask) };
    let listener = bind_result?;

    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let inactivity_timeout = Duration::from_secs(300); // 5 minutes

    loop {
        let timeout_deadline = {
            let last = last_activity.lock().await;
            *last + inactivity_timeout
        };

        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        *last_activity.lock().await = Instant::now();
                        let state = state.clone();
                        let upstream = upstream_writer.clone();
                        let pending = pending_requests.clone();
                        let shutdown = shutdown_tx.clone();
                        tokio::spawn(async move {
                            handle_local_connection(stream, state, upstream, pending, shutdown).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("[mira/mux] Accept error: {e}");
                    }
                }
            }
            _ = tokio::time::sleep_until(timeout_deadline) => {
                eprintln!("[mira/mux] Shutting down after 5 minutes of inactivity");
                break;
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    eprintln!("[mira/mux] Shutdown signal received");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn handle_local_connection(
    stream: UnixStream,
    state: Arc<RwLock<SessionState>>,
    upstream: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    pending: PendingRequests,
    shutdown: watch::Sender<bool>,
) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
        if line.len() > MAX_LINE_SIZE {
            line.clear();
            continue;
        }

        let req: IpcRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(_) => {
                line.clear();
                continue;
            }
        };

        let resp = match req.op.as_str() {
            "read_state" => handle_read_state(&req, &state).await,
            "shutdown" => {
                let resp = IpcResponse::success(req.id, serde_json::json!({"ok": true}));
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let _ = writer.write_all(format!("{json}\n").as_bytes()).await;
                let _ = writer.flush().await;
                let _ = shutdown.send(true);
                return;
            }
            _ => {
                // Proxy to upstream server
                proxy_to_upstream(&req, &upstream, &pending).await
            }
        };

        let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
        if writer
            .write_all(format!("{json}\n").as_bytes())
            .await
            .is_err()
        {
            break;
        }
        if writer.flush().await.is_err() {
            break;
        }

        line.clear();
    }
}

async fn handle_read_state(req: &IpcRequest, state: &Arc<RwLock<SessionState>>) -> IpcResponse {
    let s = state.read().await;
    let keys = req.params["keys"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut result = serde_json::Map::new();
    for key in &keys {
        match *key {
            "goals" => {
                result.insert(
                    "goals".into(),
                    serde_json::to_value(&s.goals).unwrap_or_default(),
                );
            }
            "modified_files" => {
                result.insert(
                    "modified_files".into(),
                    serde_json::to_value(&s.modified_files).unwrap_or_default(),
                );
            }
            "injection_stats" => {
                result.insert(
                    "injection_stats".into(),
                    serde_json::to_value(&s.injection_stats).unwrap_or_default(),
                );
            }
            "team_conflicts" => {
                result.insert(
                    "team_conflicts".into(),
                    serde_json::to_value(&s.team_conflicts).unwrap_or_default(),
                );
            }
            "sequence" => {
                result.insert("sequence".into(), serde_json::json!(s.sequence));
            }
            _ => {}
        }
    }
    IpcResponse::success(req.id.clone(), serde_json::Value::Object(result))
}

async fn proxy_to_upstream(
    req: &IpcRequest,
    upstream: &Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    pending: &PendingRequests,
) -> IpcResponse {
    let (tx, rx) = tokio::sync::oneshot::channel();

    // Register pending request
    {
        let mut map = pending.lock().await;
        map.insert(req.id.clone(), tx);
    }

    // Send request upstream
    {
        let mut writer = upstream.lock().await;
        let json = match serde_json::to_string(req) {
            Ok(j) => j,
            Err(e) => {
                pending.lock().await.remove(&req.id);
                return IpcResponse::error(req.id.clone(), format!("serialize error: {e}"));
            }
        };
        if writer
            .write_all(format!("{json}\n").as_bytes())
            .await
            .is_err()
        {
            pending.lock().await.remove(&req.id);
            return IpcResponse::error(req.id.clone(), "upstream write failed");
        }
        if writer.flush().await.is_err() {
            pending.lock().await.remove(&req.id);
            return IpcResponse::error(req.id.clone(), "upstream flush failed");
        }
    }

    // Wait for response with timeout
    match tokio::time::timeout(Duration::from_secs(30), rx).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(_)) => IpcResponse::error(req.id.clone(), "upstream connection closed"),
        Err(_) => {
            pending.lock().await.remove(&req.id);
            IpcResponse::error(req.id.clone(), "upstream timeout")
        }
    }
}
