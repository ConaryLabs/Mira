// crates/mira-server/src/ipc/client/mod.rs
// IPC client for hooks — connects to MCP server via Unix socket, falls back to direct DB

mod goal_ops;
mod session_ops;
mod state_ops;
mod team_ops;

use crate::db::pool::DatabasePool;
use crate::ipc::protocol::{IpcRequest, IpcResponse};
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

// ═══════════════════════════════════════════════════════════════════════
// IPC Transport
// ═══════════════════════════════════════════════════════════════════════

/// Transport-agnostic IPC stream for hook-to-server communication.
///
/// Wraps any `AsyncRead + AsyncWrite` stream with the NDJSON protocol
/// used by the IPC handler. Platform-specific transports (Unix sockets,
/// Named Pipes) are abstracted behind boxed reader/writer halves.
pub(crate) struct IpcStream {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
}

impl IpcStream {
    /// Create an IPC stream from any split reader/writer pair.
    pub fn new<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
        }
    }

    /// Send a request and read the raw response line using the NDJSON protocol.
    ///
    /// Returns the raw JSON response string. The caller is responsible for
    /// parsing it into an IpcResponse and handling fallback logic.
    async fn send_raw(&mut self, req: &IpcRequest) -> std::io::Result<String> {
        let mut line = serde_json::to_string(req).map_err(std::io::Error::other)?;
        line.push('\n');

        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;

        let mut buf = String::new();
        let n = self.reader.read_line(&mut buf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "server closed connection",
            ));
        }
        Ok(buf)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// HookClient
// ═══════════════════════════════════════════════════════════════════════

pub struct HookClient {
    inner: Backend,
}

enum Backend {
    Ipc(IpcStream),
    Direct {
        pool: Arc<DatabasePool>,
    },
    /// Both IPC and direct DB are unavailable; methods return defaults.
    Unavailable,
}

impl HookClient {
    /// Connect to the MCP server via platform-native IPC.
    /// Tries Unix socket (Unix) or Named Pipe (Windows), then falls back
    /// to direct DB access if the server is unavailable.
    pub async fn connect() -> Self {
        // On Unix, try the IPC socket first
        #[cfg(unix)]
        {
            use std::time::Duration;
            let sock = super::socket_path();
            if let Ok(Ok(stream)) = tokio::time::timeout(
                Duration::from_millis(100),
                tokio::net::UnixStream::connect(&sock),
            )
            .await
            {
                let (read, write) = tokio::io::split(stream);
                tracing::debug!("[mira] IPC: connected via socket");
                return Self {
                    inner: Backend::Ipc(IpcStream::new(read, write)),
                };
            }
        }

        // On Windows, try Named Pipe
        #[cfg(windows)]
        {
            use std::time::Duration;
            use tokio::net::windows::named_pipe::ClientOptions;

            let name = super::pipe_name();
            // Try to open the pipe with a brief retry for ERROR_PIPE_BUSY
            let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
            loop {
                match ClientOptions::new().open(&name) {
                    Ok(pipe) => {
                        let (read, write) = tokio::io::split(pipe);
                        tracing::debug!("[mira] IPC: connected via named pipe");
                        return Self {
                            inner: Backend::Ipc(IpcStream::new(read, write)),
                        };
                    }
                    Err(e) if e.raw_os_error() == Some(231) => {
                        // ERROR_PIPE_BUSY (231) — server exists but all instances busy
                        if tokio::time::Instant::now() >= deadline {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(_) => break, // Pipe doesn't exist or other error
                }
            }
        }

        // IPC unavailable — try direct DB
        let db_path = crate::hooks::get_db_path();
        match DatabasePool::open_hook(&db_path).await {
            Ok(pool) => {
                tracing::debug!("[mira] IPC: connected via direct DB");
                Self {
                    inner: Backend::Direct {
                        pool: Arc::new(pool),
                    },
                }
            }
            Err(e) => {
                tracing::warn!("[mira] IPC: both socket and database unavailable: {e}");
                Self {
                    inner: Backend::Unavailable,
                }
            }
        }
    }

    /// Create a HookClient wrapping an existing pool (for tests).
    #[cfg(test)]
    pub fn from_pool(pool: Arc<DatabasePool>) -> Self {
        Self {
            inner: Backend::Direct { pool },
        }
    }

    /// Create a HookClient from a pre-connected UnixStream (for IPC integration tests).
    #[cfg(all(test, unix))]
    pub fn from_stream(stream: tokio::net::UnixStream) -> Self {
        let (read, write) = tokio::io::split(stream);
        Self {
            inner: Backend::Ipc(IpcStream::new(read, write)),
        }
    }

    pub fn is_ipc(&self) -> bool {
        matches!(self.inner, Backend::Ipc(_))
    }

    /// Returns true when both IPC and direct DB are unavailable.
    /// Hooks can use this to inject a status advisory.
    pub fn is_unavailable(&self) -> bool {
        matches!(self.inner, Backend::Unavailable)
    }

    /// Get the direct DB pool (only available in Direct mode).
    /// Used by hooks that need pool access for operations not yet in IPC.
    pub fn pool(&self) -> Option<&Arc<DatabasePool>> {
        match &self.inner {
            Backend::Direct { pool } => Some(pool),
            _ => None,
        }
    }

    /// Send a request over the IPC stream and read the response.
    ///
    /// On I/O errors or server-level protocol errors (overloaded, timeout),
    /// automatically switches to direct DB via `fallback_to_direct()`.
    /// Methods use a "try IPC, fall through to Direct" pattern so the
    /// current call retries via Direct immediately.
    async fn call(&mut self, op: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let Backend::Ipc(stream) = &mut self.inner else {
            anyhow::bail!("call() is only available on IPC backend");
        };

        let req = IpcRequest {
            op: op.to_string(),
            id: uuid::Uuid::new_v4().to_string(),
            params,
        };

        // send_raw borrows stream (and thus self.inner). The result is an
        // owned String, so the borrow is released before fallback_to_direct().
        let io_result = stream.send_raw(&req).await;

        match io_result {
            Ok(buf) => {
                let resp: IpcResponse = serde_json::from_str(&buf)?;
                if resp.ok {
                    Ok(resp.result.unwrap_or(serde_json::Value::Null))
                } else {
                    let err_msg = resp.error.unwrap_or_else(|| "unknown IPC error".into());
                    // Server-level errors mean the connection is being closed
                    if err_msg.contains("overloaded") || err_msg.contains("timeout") {
                        tracing::warn!(
                            "[mira] IPC server error ({err_msg}), switching to direct DB"
                        );
                        self.fallback_to_direct().await;
                    }
                    anyhow::bail!(err_msg)
                }
            }
            Err(e) => {
                tracing::warn!("[mira] IPC connection error ({e}), switching to direct DB");
                self.fallback_to_direct().await;
                anyhow::bail!("IPC failed: {e}")
            }
        }
    }

    /// Switch from broken IPC to direct DB for all subsequent calls.
    async fn fallback_to_direct(&mut self) {
        let db_path = crate::hooks::get_db_path();
        match DatabasePool::open_hook(&db_path).await {
            Ok(pool) => {
                self.inner = Backend::Direct {
                    pool: Arc::new(pool),
                };
                tracing::debug!("[mira] Switched to direct DB fallback");
            }
            Err(e) => {
                tracing::warn!("[mira] Direct DB fallback also failed: {e}");
            }
        }
    }

    /// Resolve the active project, returning (project_id, project_path).
    /// When `session_id` is provided, per-session cwd files are checked first.
    pub async fn resolve_project(
        &mut self,
        cwd: Option<&str>,
        session_id: Option<&str>,
    ) -> Option<(i64, String)> {
        if self.is_ipc() {
            let mut params = json!({});
            if let Some(c) = cwd {
                params["cwd"] = json!(c);
            }
            if let Some(s) = session_id {
                params["session_id"] = json!(s);
            }
            if let Ok(result) = self.call("resolve_project", params).await {
                let project_id = result.get("project_id")?.as_i64()?;
                let path = result.get("path")?.as_str()?.to_string();
                return Some((project_id, path));
            }
            // IPC failed — call() may have switched to Direct, fall through
        }
        if let Backend::Direct { pool } = &self.inner {
            let (id, path, _name) = crate::hooks::resolve_project(pool, session_id).await;
            return Some((id?, path?));
        }
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// Result from the composite `get_user_prompt_context` IPC call.
#[derive(Debug)]
pub struct UserPromptContextResult {
    pub project_id: Option<i64>,
    pub project_path: Option<String>,
    pub reactive_context: String,
    pub reactive_sources: Vec<String>,
    pub reactive_from_cache: bool,
    pub reactive_summary: String,
    pub reactive_skip_reason: Option<String>,
    pub team_context: Option<String>,
    pub config_max_chars: usize,
}

/// Team membership info returned by `get_team_membership`.
#[derive(Debug, Clone)]
pub struct TeamMembershipInfo {
    pub team_id: i64,
    pub team_name: String,
    pub member_name: String,
    pub role: String,
}

/// File conflict info returned by `get_file_conflicts`.
#[derive(Debug, Clone)]
pub struct FileConflictInfo {
    pub file_path: String,
    pub other_member_name: String,
    pub operation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_unavailable_returns_true_for_unavailable_backend() {
        let client = HookClient {
            inner: Backend::Unavailable,
        };
        assert!(client.is_unavailable(), "Unavailable backend must return true");
        assert!(!client.is_ipc(), "Unavailable backend must not report as IPC");
    }

    #[tokio::test]
    async fn is_unavailable_returns_false_for_direct_backend() {
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("test.db");
        let pool = Arc::new(
            DatabasePool::open_hook(&db_path)
                .await
                .unwrap(),
        );
        let client = HookClient::from_pool(pool);
        assert!(!client.is_unavailable(), "Direct backend must not be unavailable");
        assert!(!client.is_ipc(), "Direct backend must not report as IPC");
    }
}
