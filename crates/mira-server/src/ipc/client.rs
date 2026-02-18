// crates/mira-server/src/ipc/client.rs
// IPC client for hooks — connects to MCP server via Unix socket, falls back to direct DB

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

    // ═══════════════════════════════════════════════════════════════════════
    // Project & Memory
    // ═══════════════════════════════════════════════════════════════════════

    /// Resolve the active project, returning (project_id, project_path).
    pub async fn resolve_project(&mut self, cwd: Option<&str>) -> Option<(i64, String)> {
        if self.is_ipc() {
            let params = match cwd {
                Some(c) => json!({"cwd": c}),
                None => json!({}),
            };
            if let Ok(result) = self.call("resolve_project", params).await {
                let project_id = result.get("project_id")?.as_i64()?;
                let path = result.get("path")?.as_str()?.to_string();
                return Some((project_id, path));
            }
            // IPC failed — call() may have switched to Direct, fall through
        }
        if let Backend::Direct { pool } = &self.inner {
            let (id, path) = crate::hooks::resolve_project(pool).await;
            return Some((id?, path?));
        }
        None
    }

    /// Recall relevant memories for a project and query string.
    pub async fn recall_memories(&mut self, project_id: i64, query: &str) -> Vec<String> {
        if self.is_ipc() {
            let params = json!({"project_id": project_id, "query": query});
            if let Ok(result) = self.call("recall_memories", params).await {
                return result
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::recall::recall_memories(pool, project_id, query).await;
        }
        Vec::new()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Behavior Tracking
    // ═══════════════════════════════════════════════════════════════════════

    /// Log a behavior event. Fire-and-forget — errors are logged but not propagated.
    pub async fn log_behavior(
        &mut self,
        session_id: &str,
        project_id: i64,
        event_type: &str,
        event_data: serde_json::Value,
    ) {
        if session_id.is_empty() {
            return;
        }
        if self.is_ipc() {
            let params = json!({
                "session_id": session_id,
                "project_id": project_id,
                "event_type": event_type,
                "event_data": event_data,
            });
            if self.call("log_behavior", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let event_type = event_type.to_string();
            pool.try_interact("log_behavior", move |conn| {
                let mut tracker = crate::proactive::behavior::BehaviorTracker::for_session(
                    conn, session_id, project_id,
                );
                let et = match event_type.as_str() {
                    "tool_failure" => crate::proactive::EventType::ToolFailure,
                    "goal_update" => crate::proactive::EventType::GoalUpdate,
                    _ => crate::proactive::EventType::ToolUse,
                };
                if let Err(e) = tracker.log_event(conn, et, event_data) {
                    tracing::debug!("Failed to log behavior: {e}");
                }
                Ok(())
            })
            .await;
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Observations
    // ═══════════════════════════════════════════════════════════════════════

    /// Store an observation. Fire-and-forget.
    #[allow(clippy::too_many_arguments)]
    pub async fn store_observation(
        &mut self,
        project_id: Option<i64>,
        content: &str,
        observation_type: &str,
        category: Option<&str>,
        confidence: f64,
        source: &str,
        scope: &str,
        expires_at: Option<&str>,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "content": content,
                "observation_type": observation_type,
                "category": category,
                "confidence": confidence,
                "source": source,
                "scope": scope,
                "expires_at": expires_at,
            });
            if self.call("store_observation", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let content = content.to_string();
            let observation_type = observation_type.to_string();
            let category = category.map(String::from);
            let source = source.to_string();
            let scope = scope.to_string();
            let expires_at = expires_at.map(String::from);
            pool.try_interact("store_observation", move |conn| {
                crate::db::store_observation_sync(
                    conn,
                    crate::db::StoreObservationParams {
                        project_id,
                        key: None,
                        content: &content,
                        observation_type: &observation_type,
                        category: category.as_deref(),
                        confidence,
                        source: &source,
                        session_id: None,
                        team_id: None,
                        scope: &scope,
                        expires_at: expires_at.as_deref(),
                    },
                )?;
                Ok(())
            })
            .await;
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Goals
    // ═══════════════════════════════════════════════════════════════════════

    /// Get formatted active goals for a project.
    pub async fn get_active_goals(&mut self, project_id: i64, limit: usize) -> Vec<String> {
        if self.is_ipc() {
            let params = json!({"project_id": project_id, "limit": limit});
            if let Ok(result) = self.call("get_active_goals", params).await {
                return result
                    .get("goals")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::format_active_goals(pool, project_id, limit).await;
        }
        Vec::new()
    }

    /// Auto-link a completed task to goal milestones. Fire-and-forget.
    pub async fn auto_link_milestone(
        &mut self,
        project_id: i64,
        task_subject: &str,
        task_description: Option<&str>,
        session_id: Option<&str>,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "task_subject": task_subject,
                "task_description": task_description,
                "session_id": session_id,
            });
            if self.call("auto_link_milestone", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let task_subject = task_subject.to_string();
            let task_description = task_description.map(String::from);
            let session_id = session_id.map(String::from);
            pool.try_interact("auto_link_milestone", move |conn| {
                crate::hooks::task_completed::auto_link_milestone(
                    conn,
                    project_id,
                    &task_subject,
                    task_description.as_deref(),
                    session_id.as_deref(),
                )
            })
            .await;
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Error Patterns
    // ═══════════════════════════════════════════════════════════════════════

    /// Store or update an error pattern. Fire-and-forget.
    pub async fn store_error_pattern(
        &mut self,
        project_id: i64,
        tool_name: &str,
        fingerprint: &str,
        template: &str,
        sample: &str,
        session_id: &str,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "tool_name": tool_name,
                "fingerprint": fingerprint,
                "template": template,
                "sample": sample,
                "session_id": session_id,
            });
            if self.call("store_error_pattern", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let tool_name = tool_name.to_string();
            let fingerprint = fingerprint.to_string();
            let template = template.to_string();
            let sample = sample.to_string();
            let session_id = session_id.to_string();
            pool.try_interact("store_error_pattern", move |conn| {
                crate::db::store_error_pattern_sync(
                    conn,
                    crate::db::StoreErrorPatternParams {
                        project_id,
                        tool_name: &tool_name,
                        error_fingerprint: &fingerprint,
                        error_template: &template,
                        raw_error_sample: &sample,
                        session_id: &session_id,
                    },
                )?;
                Ok(())
            })
            .await;
        }
    }

    /// Look up a resolved error pattern. Returns the fix description if found.
    pub async fn lookup_resolved_pattern(
        &mut self,
        project_id: i64,
        tool_name: &str,
        fingerprint: &str,
    ) -> Option<String> {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "tool_name": tool_name,
                "fingerprint": fingerprint,
            });
            if let Ok(result) = self.call("lookup_resolved_pattern", params).await {
                if result
                    .get("found")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return result
                        .get("fix_description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                return None;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let tool_name = tool_name.to_string();
            let fingerprint = fingerprint.to_string();
            let pattern = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::lookup_resolved_pattern_sync(
                        conn,
                        project_id,
                        &tool_name,
                        &fingerprint,
                    ))
                })
                .await
                .ok()
                .flatten()?;
            return Some(pattern.fix_description);
        }
        None
    }

    /// Count how many times a tool has failed in the current session.
    pub async fn count_session_failures(&mut self, session_id: &str, tool_name: &str) -> i64 {
        if self.is_ipc() {
            let params = json!({"session_id": session_id, "tool_name": tool_name});
            if let Ok(result) = self.call("count_session_failures", params).await {
                return result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let tool_name = tool_name.to_string();
            return pool
                .interact(move |conn| {
                    let count = conn
                        .query_row(
                            "SELECT COUNT(*) FROM session_behavior_log
                             WHERE session_id = ? AND event_type = 'tool_failure'
                               AND json_extract(event_data, '$.tool_name') = ?",
                            rusqlite::params![session_id, tool_name],
                            |row| row.get::<_, i64>(0),
                        )
                        .unwrap_or(0);
                    Ok::<_, anyhow::Error>(count)
                })
                .await
                .unwrap_or(0);
        }
        0
    }

    /// Resolve error patterns after a successful tool use.
    /// Returns true if a pattern was resolved.
    pub async fn resolve_error_patterns(
        &mut self,
        project_id: i64,
        session_id: &str,
        tool_name: &str,
    ) -> bool {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "session_id": session_id,
                "tool_name": tool_name,
            });
            if let Ok(result) = self.call("resolve_error_patterns", params).await {
                return result
                    .get("resolved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let tool_name = tool_name.to_string();
            return pool
                .interact(move |conn| {
                    let candidates = crate::db::get_unresolved_patterns_for_tool_sync(
                        conn,
                        project_id,
                        &tool_name,
                        &session_id,
                    );

                    let mut best: Option<(String, i64, i64)> = None;
                    for (_id, fingerprint) in &candidates {
                        let row: Option<(i64, i64)> = conn
                            .query_row(
                                "SELECT COUNT(*), COALESCE(MAX(sequence_position), 0)
                                 FROM session_behavior_log
                                 WHERE session_id = ? AND project_id = ?
                                   AND event_type = 'tool_failure'
                                   AND json_extract(event_data, '$.error_fingerprint') = ?",
                                rusqlite::params![&session_id, project_id, fingerprint],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .ok();

                        if let Some((count, max_seq)) = row
                            && count >= 3
                        {
                            let dominated = match &best {
                                None => true,
                                Some((_, _, best_seq)) => max_seq > *best_seq,
                            };
                            if dominated {
                                best = Some((fingerprint.clone(), count, max_seq));
                            }
                        }
                    }

                    if let Some((fingerprint, session_fp_count, _)) = best {
                        let _ = crate::db::resolve_error_pattern_sync(
                            conn,
                            project_id,
                            &tool_name,
                            &fingerprint,
                            &session_id,
                            &format!(
                                "Tool '{}' succeeded after {} session failures of this pattern",
                                tool_name, session_fp_count
                            ),
                        );
                        return Ok::<_, anyhow::Error>(true);
                    }
                    Ok(false)
                })
                .await
                .unwrap_or(false);
        }
        false
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Team Operations
    // ═══════════════════════════════════════════════════════════════════════

    /// Get team membership for a session.
    /// Returns (team_id, team_name, member_name, role) if found.
    pub async fn get_team_membership(&mut self, session_id: &str) -> Option<TeamMembershipInfo> {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if let Ok(result) = self.call("get_team_membership", params).await {
                if result
                    .get("found")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Some(TeamMembershipInfo {
                        team_id: result.get("team_id")?.as_i64()?,
                        team_name: result.get("team_name")?.as_str()?.to_string(),
                        member_name: result.get("member_name")?.as_str()?.to_string(),
                        role: result.get("role")?.as_str()?.to_string(),
                    });
                }
                return None;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let membership = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::get_team_membership_for_session_sync(
                        conn,
                        &session_id,
                    ))
                })
                .await
                .ok()
                .flatten()?;
            return Some(TeamMembershipInfo {
                team_id: membership.team_id,
                team_name: membership.team_name,
                member_name: membership.member_name,
                role: membership.role,
            });
        }
        None
    }

    /// Record file ownership for team conflict detection. Fire-and-forget.
    pub async fn record_file_ownership(
        &mut self,
        team_id: i64,
        session_id: &str,
        member_name: &str,
        file_path: &str,
        tool_name: &str,
    ) {
        if self.is_ipc() {
            let params = json!({
                "team_id": team_id,
                "session_id": session_id,
                "member_name": member_name,
                "file_path": file_path,
                "tool_name": tool_name,
            });
            if self.call("record_file_ownership", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let member_name = member_name.to_string();
            let file_path = file_path.to_string();
            let tool_name = tool_name.to_string();
            pool.try_interact("record_file_ownership", move |conn| {
                crate::db::record_file_ownership_sync(
                    conn,
                    team_id,
                    &session_id,
                    &member_name,
                    &file_path,
                    &tool_name,
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(())
            })
            .await;
        }
    }

    /// Get file conflicts for a team session.
    pub async fn get_file_conflicts(
        &mut self,
        team_id: i64,
        session_id: &str,
    ) -> Vec<FileConflictInfo> {
        if self.is_ipc() {
            let params = json!({"team_id": team_id, "session_id": session_id});
            if let Ok(result) = self.call("get_file_conflicts", params).await {
                return result
                    .get("conflicts")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| {
                                Some(FileConflictInfo {
                                    file_path: c.get("file_path")?.as_str()?.to_string(),
                                    other_member_name: c
                                        .get("other_member_name")?
                                        .as_str()?
                                        .to_string(),
                                    operation: c.get("operation")?.as_str()?.to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let conflicts = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::get_file_conflicts_sync(
                        conn,
                        team_id,
                        &session_id,
                    ))
                })
                .await
                .unwrap_or_default();
            return conflicts
                .into_iter()
                .map(|c| FileConflictInfo {
                    file_path: c.file_path,
                    other_member_name: c.other_member_name,
                    operation: c.operation,
                })
                .collect();
        }
        Vec::new()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Compaction
    // ═══════════════════════════════════════════════════════════════════════

    /// Save compaction context to session_snapshots. Fire-and-forget.
    pub async fn save_compaction_context(&mut self, session_id: &str, context: serde_json::Value) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id, "context": context});
            if self.call("save_compaction_context", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            pool.try_interact("save_compaction_context", move |conn| {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                        [&session_id],
                        |row| row.get::<_, String>(0),
                    )
                    .ok();

                let mut snapshot = if let Some(ref json_str) = existing {
                    serde_json::from_str::<serde_json::Value>(json_str)
                        .unwrap_or_else(|_| json!({}))
                } else {
                    json!({})
                };

                snapshot["compaction_context"] = context;

                let snapshot_str =
                    serde_json::to_string(&snapshot).map_err(|e| anyhow::anyhow!("{e}"))?;

                conn.execute(
                    "INSERT INTO session_snapshots (session_id, snapshot, created_at)
                     VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(session_id) DO UPDATE SET snapshot = ?2, created_at = datetime('now')",
                    rusqlite::params![session_id, snapshot_str],
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;

                Ok(())
            })
            .await;
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Session Lifecycle
    // ═══════════════════════════════════════════════════════════════════════

    /// Register a session in the database. Returns project_id.
    pub async fn register_session(
        &mut self,
        session_id: &str,
        cwd: &str,
        source: &str,
    ) -> Option<i64> {
        if self.is_ipc() {
            let params = json!({
                "session_id": session_id,
                "cwd": cwd,
                "source": source,
            });
            if let Ok(result) = self.call("register_session", params).await {
                return result.get("project_id")?.as_i64();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let cwd = cwd.to_string();
            let source = source.to_string();
            return pool
                .run(move |conn| {
                    let (project_id, _) = crate::db::get_or_create_project_sync(conn, &cwd, None)?;
                    crate::db::create_session_ext_sync(
                        conn,
                        &session_id,
                        Some(project_id),
                        Some(&source),
                        None,
                    )?;
                    conn.execute(
                        "INSERT INTO session_behavior_log (session_id, event_type, event_data) \
                         VALUES (?1, 'session_start', ?2)",
                        rusqlite::params![session_id, source],
                    )
                    .ok();
                    Ok::<_, rusqlite::Error>(project_id)
                })
                .await
                .ok();
        }
        None
    }

    /// Register a team session. Returns team_id.
    #[allow(clippy::too_many_arguments)]
    pub async fn register_team_session(
        &mut self,
        team_name: &str,
        config_path: &str,
        member_name: &str,
        role: &str,
        agent_type: Option<&str>,
        session_id: &str,
        cwd: Option<&str>,
    ) -> Option<i64> {
        if self.is_ipc() {
            let params = json!({
                "team_name": team_name,
                "config_path": config_path,
                "member_name": member_name,
                "role": role,
                "agent_type": agent_type,
                "session_id": session_id,
                "cwd": cwd,
            });
            if let Ok(result) = self.call("register_team_session", params).await {
                return result.get("team_id")?.as_i64();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let team_name = team_name.to_string();
            let config_path = config_path.to_string();
            let member_name = member_name.to_string();
            let role = role.to_string();
            let agent_type = agent_type.map(String::from);
            let session_id = session_id.to_string();
            let cwd = cwd.map(String::from);
            return pool
                .interact(move |conn| {
                    let project_id: Option<i64> = if let Some(ref cwd_path) = cwd {
                        crate::db::get_or_create_project_sync(conn, cwd_path, None)
                            .ok()
                            .map(|(id, _)| id)
                    } else {
                        None
                    };
                    let tid = crate::db::get_or_create_team_sync(
                        conn,
                        &team_name,
                        project_id,
                        &config_path,
                    )?;
                    crate::db::register_team_session_sync(
                        conn,
                        tid,
                        &session_id,
                        &member_name,
                        &role,
                        agent_type.as_deref(),
                    )?;
                    Ok::<_, anyhow::Error>(tid)
                })
                .await
                .ok();
        }
        None
    }

    /// Get startup context for a fresh session.
    pub async fn get_startup_context(&mut self, cwd: Option<&str>) -> Option<String> {
        if self.is_ipc() {
            let params = json!({"cwd": cwd});
            if let Ok(result) = self.call("get_startup_context", params).await {
                return result.get("context")?.as_str().map(String::from);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::session::build_startup_context(cwd, Some(pool.clone())).await;
        }
        None
    }

    /// Get resume context for a resumed session.
    pub async fn get_resume_context(
        &mut self,
        cwd: Option<&str>,
        session_id: Option<&str>,
    ) -> Option<String> {
        if self.is_ipc() {
            let params = json!({"cwd": cwd, "session_id": session_id});
            if let Ok(result) = self.call("get_resume_context", params).await {
                return result.get("context")?.as_str().map(String::from);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::session::build_resume_context(
                cwd,
                session_id,
                Some(pool.clone()),
            )
            .await;
        }
        None
    }

    /// Close a session: build summary, save snapshot, update status. Fire-and-forget.
    pub async fn close_session(&mut self, session_id: &str) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if self.call("close_session", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            pool.try_interact_warn("session close", move |conn| {
                if let Err(e) = crate::db::set_server_state_sync(
                    conn,
                    "last_stop_time",
                    &chrono::Utc::now().to_rfc3339(),
                ) {
                    tracing::warn!("[mira] Failed to save server state: {e}");
                }
                let summary = if !session_id.is_empty() {
                    crate::hooks::stop::build_session_summary(conn, &session_id)
                } else {
                    None
                };
                if !session_id.is_empty() {
                    if let Err(e) = crate::hooks::stop::save_session_snapshot(conn, &session_id) {
                        tracing::warn!("[mira] Session snapshot failed: {}", e);
                    }
                    if let Err(e) =
                        crate::db::close_session_sync(conn, &session_id, summary.as_deref())
                    {
                        tracing::warn!("[mira] Failed to close session: {e}");
                    }
                    tracing::debug!(
                        "[mira] Closed session {}",
                        crate::utils::truncate_at_boundary(&session_id, 8)
                    );
                }
                Ok(())
            })
            .await;
        }
    }

    /// Snapshot native Claude Code tasks. Fire-and-forget.
    pub async fn snapshot_tasks(
        &mut self,
        project_id: i64,
        list_id: &str,
        session_id: Option<&str>,
        tasks: &[crate::tasks::NativeTask],
        is_session_end: bool,
    ) {
        let (completed, remaining) = tasks.iter().fold((0usize, 0usize), |(c, r), t| {
            if t.status == "completed" {
                (c + 1, r)
            } else {
                (c, r + 1)
            }
        });

        let mut ipc_ok = false;
        if self.is_ipc() {
            let tasks_json = match serde_json::to_value(tasks) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("[mira] Failed to serialize tasks: {}", e);
                    return;
                }
            };
            let params = json!({
                "project_id": project_id,
                "list_id": list_id,
                "session_id": session_id,
                "tasks": tasks_json,
            });
            if let Ok(v) = self.call("snapshot_tasks", params).await {
                let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let label = if is_session_end { "SessionEnd" } else { "Stop" };
                tracing::debug!(
                    "[mira] {} snapshot: {} tasks ({} completed, {} remaining)",
                    label,
                    count,
                    completed,
                    remaining,
                );
                ipc_ok = true;
            }
            // fall through to Direct
        }
        if !ipc_ok && let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let list_id = list_id.to_string();
            let session_id = session_id.map(String::from);
            let tasks = tasks.to_vec();
            match pool
                .interact(move |conn| {
                    let count = crate::db::session_tasks::snapshot_native_tasks_sync(
                        conn,
                        project_id,
                        &list_id,
                        session_id.as_deref(),
                        &tasks,
                    )?;
                    Ok::<_, anyhow::Error>(count)
                })
                .await
            {
                Ok(count) => {
                    let label = if is_session_end { "SessionEnd" } else { "Stop" };
                    tracing::debug!(
                        "[mira] {} snapshot: {} tasks ({} completed, {} remaining)",
                        label,
                        count,
                        completed,
                        remaining,
                    );
                }
                Err(e) => {
                    tracing::warn!("[mira] Task snapshot failed: {}", e);
                }
            }
        }
    }

    /// Auto-export memories to CLAUDE.local.md. Fire-and-forget.
    pub async fn write_claude_local_md(&mut self, project_id: i64) {
        if self.is_ipc() {
            let params = json!({"project_id": project_id});
            if let Ok(v) = self.call("write_claude_local_md", params).await {
                let count = v.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
                if count > 0 {
                    tracing::debug!("[mira] Auto-exported {} memories to CLAUDE.local.md", count);
                }
                return;
            }
            // fall through to Direct
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            pool.try_interact_warn("CLAUDE.local.md export", move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).unwrap_or_else(|e| {
                    tracing::warn!("Failed to get last active project: {e}");
                    None
                });
                if let Some(project_path) = path {
                    match crate::tools::core::claude_local::write_claude_local_md_sync(
                        conn,
                        project_id,
                        &project_path,
                    ) {
                        Ok(count) if count > 0 => {
                            tracing::debug!(
                                "[mira] Auto-exported {} memories to CLAUDE.local.md",
                                count
                            );
                        }
                        Err(e) => {
                            tracing::warn!("[mira] CLAUDE.local.md export failed: {}", e);
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
            .await;
        }
    }

    /// Deactivate a team session. Fire-and-forget.
    pub async fn deactivate_team_session(&mut self, session_id: &str) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if self.call("deactivate_team_session", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            if let Err(e) = pool
                .run(move |conn| crate::db::deactivate_team_session_sync(conn, &session_id))
                .await
            {
                tracing::warn!("[mira] Failed to deactivate team session: {}", e);
            }
        }
    }

    /// Export memories to MEMORY.mira.md. Fire-and-forget.
    pub async fn write_auto_memory(&mut self, project_id: i64, project_path: &str) {
        if self.is_ipc() {
            let params = json!({"project_id": project_id, "project_path": project_path});
            if let Ok(v) = self.call("write_auto_memory", params).await {
                let count = v.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
                if count > 0 {
                    tracing::debug!("[mira] Auto-exported {} memories to MEMORY.mira.md", count);
                }
                return;
            }
            // fall through to Direct
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let project_path = project_path.to_string();
            pool.try_interact_warn("auto memory export", move |conn| {
                if crate::tools::core::claude_local::auto_memory_dir_exists(&project_path) {
                    match crate::tools::core::claude_local::write_auto_memory_sync(
                        conn,
                        project_id,
                        &project_path,
                    ) {
                        Ok(count) if count > 0 => {
                            tracing::debug!(
                                "[mira] Auto-exported {} memories to MEMORY.mira.md",
                                count
                            );
                        }
                        Err(e) => {
                            tracing::warn!("[mira] Auto memory export failed: {}", e);
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
            .await;
        }
    }

    /// Distill team session knowledge. Returns (distilled, findings_count, team_name).
    pub async fn distill_team_session(
        &mut self,
        team_id: i64,
        project_id: Option<i64>,
    ) -> (bool, usize, String) {
        if self.is_ipc() {
            let params = json!({"team_id": team_id, "project_id": project_id});
            match self.call("distill_team_session", params).await {
                Ok(v) => {
                    let distilled = v
                        .get("distilled")
                        .and_then(|d| d.as_bool())
                        .unwrap_or(false);
                    let count = v
                        .get("findings_count")
                        .and_then(|c| c.as_u64())
                        .unwrap_or(0) as usize;
                    let name = v
                        .get("team_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    return (distilled, count, name);
                }
                Err(e) => {
                    tracing::warn!("[mira] Knowledge distillation via IPC failed: {}", e);
                    // fall through to Direct
                }
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return match crate::background::knowledge_distillation::distill_team_session(
                pool, team_id, project_id,
            )
            .await
            {
                Ok(Some(result)) => (true, result.findings.len(), result.team_name),
                Ok(None) => (false, 0, String::new()),
                Err(e) => {
                    tracing::warn!("[mira] Knowledge distillation failed: {}", e);
                    (false, 0, String::new())
                }
            };
        }
        (false, 0, String::new())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // UserPromptSubmit
    // ═══════════════════════════════════════════════════════════════════════

    /// Get all context needed by the UserPromptSubmit hook in a single call.
    /// Only available via IPC (returns None in Direct mode — caller handles fallback).
    pub async fn get_user_prompt_context(
        &mut self,
        message: &str,
        session_id: &str,
    ) -> Option<UserPromptContextResult> {
        if !self.is_ipc() {
            return None;
        }
        let params = json!({"message": message, "session_id": session_id});
        let v = self.call("get_user_prompt_context", params).await.ok()?;

        Some(UserPromptContextResult {
            project_id: v.get("project_id").and_then(|v| v.as_i64()),
            project_path: v
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(String::from),
            reactive_context: v
                .get("reactive_context")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reactive_sources: v
                .get("reactive_sources")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            reactive_from_cache: v
                .get("reactive_from_cache")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            reactive_summary: v
                .get("reactive_summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reactive_skip_reason: v
                .get("reactive_skip_reason")
                .and_then(|v| v.as_str())
                .map(String::from),
            proactive_context: v
                .get("proactive_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            team_context: v
                .get("team_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            cross_project_context: v
                .get("cross_project_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            config_max_chars: v
                .get("config_max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(3000) as usize,
        })
    }
}

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
    pub proactive_context: Option<String>,
    pub team_context: Option<String>,
    pub cross_project_context: Option<String>,
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
