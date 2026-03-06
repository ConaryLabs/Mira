<!-- docs/plans/2026-03-05-realtime-hook-data-plan.md -->
# Real-Time Hook Data Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace one-shot IPC with persistent subscription connections and a session agent (mux) so hooks get real-time state updates instead of per-request DB reads.

**Architecture:** The MCP server gains a session channel registry that pushes state updates over persistent connections. A lightweight mux process (`mira mux`) holds the persistent connection and exposes a local `mux.sock` for hooks. Hooks try `mux.sock` first, fall back to direct `mira.sock`. Four-phase rollout: infrastructure, read hooks migrate, write hooks publish, simplify.

**Tech Stack:** Rust, tokio, Unix sockets (NDJSON protocol), serde_json

**Design doc:** `docs/plans/2026-03-05-realtime-hook-data-design.md`

---

## Phase 1: Server-Side Subscribe Protocol

### Task 1: Add subscription message types to IPC protocol

**Files:**
- Modify: `crates/mira-server/src/ipc/protocol.rs`
- Test: `crates/mira-server/src/ipc/tests.rs`

**Step 1: Write the failing test**

Add to `crates/mira-server/src/ipc/tests.rs`:

```rust
#[test]
fn test_subscribe_request_serialization() {
    let req = IpcRequest {
        op: "subscribe".to_string(),
        id: "test-1".to_string(),
        params: serde_json::json!({ "session_id": "sess-abc" }),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("subscribe"));
    assert!(json.contains("sess-abc"));
}

#[test]
fn test_push_event_serialization() {
    let event = IpcPushEvent {
        event_type: "goal_updated".to_string(),
        sequence: 1,
        data: serde_json::json!({ "goal_id": 5, "progress": 80 }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: IpcPushEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.event_type, "goal_updated");
    assert_eq!(parsed.sequence, 1);
}

#[test]
fn test_session_state_snapshot_serialization() {
    let snapshot = SessionStateSnapshot {
        sequence: 0,
        goals: vec![],
        injection_stats: InjectionStatsSnapshot { total_injections: 5, total_chars: 1200 },
        modified_files: vec!["src/main.rs".to_string()],
        team_conflicts: vec![],
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    let parsed: SessionStateSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.injection_stats.total_injections, 5);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_subscribe_request -- --nocapture`
Expected: FAIL -- `IpcPushEvent` and `SessionStateSnapshot` not defined

**Step 3: Write minimal implementation**

Add to `crates/mira-server/src/ipc/protocol.rs`:

```rust
/// Server-pushed event over a persistent subscription connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPushEvent {
    pub event_type: String,
    pub sequence: u64,
    pub data: serde_json::Value,
}

/// Full state snapshot sent on initial subscribe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateSnapshot {
    pub sequence: u64,
    pub goals: Vec<GoalSnapshot>,
    pub injection_stats: InjectionStatsSnapshot,
    pub modified_files: Vec<String>,
    pub team_conflicts: Vec<FileConflictSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSnapshot {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionStatsSnapshot {
    pub total_injections: u64,
    pub total_chars: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConflictSnapshot {
    pub file_path: String,
    pub other_member_name: String,
    pub operation: String,
}

/// Wire message: either a response (request-response) or a push event (subscription).
/// The mux and persistent connections receive both types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcMessage {
    Response(IpcResponse),
    Push(IpcPushEvent),
    Snapshot(SessionStateSnapshot),
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_subscribe_request test_push_event test_session_state_snapshot -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/mira-server/src/ipc/protocol.rs crates/mira-server/src/ipc/tests.rs
git commit -m "feat: add subscription protocol types (IpcPushEvent, SessionStateSnapshot)"
```

---

### Task 2: Add session channel registry to MiraServer

**Files:**
- Create: `crates/mira-server/src/ipc/channels.rs`
- Modify: `crates/mira-server/src/ipc/mod.rs` (add `pub mod channels;`)
- Test: `crates/mira-server/src/ipc/tests.rs`

**Step 1: Write the failing test**

Add to `crates/mira-server/src/ipc/tests.rs`:

```rust
#[tokio::test]
async fn test_channel_registry_subscribe_unsubscribe() {
    let registry = SessionChannelRegistry::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<IpcPushEvent>(32);

    registry.subscribe("sess-1", tx).await;
    assert_eq!(registry.subscriber_count().await, 1);

    registry.unsubscribe("sess-1").await;
    assert_eq!(registry.subscriber_count().await, 0);
}

#[tokio::test]
async fn test_channel_registry_publish() {
    let registry = SessionChannelRegistry::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<IpcPushEvent>(32);

    registry.subscribe("sess-1", tx).await;

    let event = IpcPushEvent {
        event_type: "goal_updated".to_string(),
        sequence: 1,
        data: serde_json::json!({ "goal_id": 5 }),
    };
    let delivered = registry.publish("sess-1", event.clone()).await;
    assert!(delivered);

    let received = rx.recv().await.unwrap();
    assert_eq!(received.event_type, "goal_updated");
}

#[tokio::test]
async fn test_channel_registry_publish_drops_on_full_buffer() {
    let registry = SessionChannelRegistry::new();
    // Buffer size 1 -- second event should be dropped (non-critical)
    let (tx, mut rx) = tokio::sync::mpsc::channel::<IpcPushEvent>(1);

    registry.subscribe("sess-1", tx).await;

    let critical = IpcPushEvent {
        event_type: "goal_updated".to_string(),
        sequence: 1,
        data: serde_json::json!({}),
    };
    let non_critical = IpcPushEvent {
        event_type: "injection_stats".to_string(),
        sequence: 2,
        data: serde_json::json!({}),
    };

    // Fill the buffer
    registry.publish("sess-1", critical).await;
    // This should be dropped (non-critical, buffer full)
    let delivered = registry.publish("sess-1", non_critical).await;
    assert!(!delivered);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_channel_registry -- --nocapture`
Expected: FAIL -- `SessionChannelRegistry` not defined

**Step 3: Write minimal implementation**

Create `crates/mira-server/src/ipc/channels.rs`:

```rust
// crates/mira-server/src/ipc/channels.rs
use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};

use super::protocol::IpcPushEvent;

/// Critical event types that must not be dropped under backpressure.
const CRITICAL_EVENTS: &[&str] = &[
    "goal_updated",
    "milestone_completed",
    "file_conflict",
    "session_event",
];

fn is_critical(event_type: &str) -> bool {
    CRITICAL_EVENTS.contains(&event_type)
}

/// Registry of active session subscriptions.
/// The MCP server holds one of these and publishes events to it.
pub struct SessionChannelRegistry {
    channels: RwLock<HashMap<String, SessionChannel>>,
}

struct SessionChannel {
    tx: mpsc::Sender<IpcPushEvent>,
    sequence: u64,
}

impl SessionChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    pub async fn subscribe(&self, session_id: &str, tx: mpsc::Sender<IpcPushEvent>) {
        let mut channels = self.channels.write().await;
        channels.insert(
            session_id.to_string(),
            SessionChannel { tx, sequence: 0 },
        );
    }

    pub async fn unsubscribe(&self, session_id: &str) {
        let mut channels = self.channels.write().await;
        channels.remove(session_id);
    }

    /// Publish an event to a session's subscriber.
    /// Returns true if delivered, false if dropped or no subscriber.
    /// Critical events block until delivered; non-critical are dropped if buffer full.
    pub async fn publish(&self, session_id: &str, mut event: IpcPushEvent) -> bool {
        let mut channels = self.channels.write().await;
        let Some(channel) = channels.get_mut(session_id) else {
            return false;
        };

        channel.sequence += 1;
        event.sequence = channel.sequence;

        if is_critical(&event.event_type) {
            // Critical: block until space available (or channel closed)
            channel.tx.send(event).await.is_ok()
        } else {
            // Non-critical: drop if buffer full
            channel.tx.try_send(event).is_ok()
        }
    }

    /// Remove channels where the receiver has been dropped.
    pub async fn cleanup_dead(&self) {
        let mut channels = self.channels.write().await;
        channels.retain(|_, ch| !ch.tx.is_closed());
    }

    pub async fn subscriber_count(&self) -> usize {
        self.channels.read().await.len()
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_channel_registry -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/mira-server/src/ipc/channels.rs crates/mira-server/src/ipc/mod.rs crates/mira-server/src/ipc/tests.rs
git commit -m "feat: add SessionChannelRegistry for persistent subscription channels"
```

---

### Task 3: Wire registry into MiraServer and add subscribe handler

**Files:**
- Modify: `crates/mira-server/src/server.rs` (or wherever `MiraServer` is defined)
- Modify: `crates/mira-server/src/ipc/handler.rs`
- Modify: `crates/mira-server/src/ipc/mod.rs`
- Test: `crates/mira-server/src/ipc/tests.rs`

**Step 1: Find where MiraServer is defined**

Search for `pub struct MiraServer` -- it likely holds `pool: Arc<DatabasePool>` and `code_pool: Arc<CodePool>`. Add a `channels: Arc<SessionChannelRegistry>` field.

**Step 2: Add registry field to MiraServer**

```rust
use crate::ipc::channels::SessionChannelRegistry;

pub struct MiraServer {
    pub pool: Arc<DatabasePool>,
    pub code_pool: Arc<CodePool>,
    pub channels: Arc<SessionChannelRegistry>,  // NEW
}
```

Update construction site(s) to initialize `channels: Arc::new(SessionChannelRegistry::new())`.

**Step 3: Handle subscribe in connection handler**

In `crates/mira-server/src/ipc/handler.rs`, modify `handle_connection` to detect `subscribe` op and switch to persistent mode:

```rust
// In the request loop, after parsing the request:
if req.op == "subscribe" {
    let session_id = req.params["session_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    if session_id.is_empty() {
        let resp = IpcResponse::error(req.id, "session_id required");
        write_response(&mut writer, &resp).await?;
        continue;
    }

    // Send initial snapshot
    let snapshot = build_session_snapshot(&server, &session_id).await;
    let resp = IpcResponse::success(req.id, serde_json::to_value(&snapshot)?);
    write_response(&mut writer, &resp).await?;

    // Switch to persistent mode: create channel, listen for events + interleaved requests
    let (tx, mut rx) = tokio::sync::mpsc::channel::<IpcPushEvent>(64);
    server.channels.subscribe(&session_id, tx).await;

    // Persistent loop: multiplex push events and incoming requests
    handle_persistent_connection(&mut reader, &mut writer, &server, &session_id, &mut rx).await;

    server.channels.unsubscribe(&session_id).await;
    break; // Connection done
}
```

**Step 4: Implement persistent connection handler**

Add to `crates/mira-server/src/ipc/handler.rs`:

```rust
async fn handle_persistent_connection<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
    server: &MiraServer,
    session_id: &str,
    rx: &mut tokio::sync::mpsc::Receiver<IpcPushEvent>,
) where
    R: tokio::io::AsyncRead + Unpin + Send,
    W: tokio::io::AsyncWrite + Unpin + Send,
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
            line = read_request_line(reader) => {
                match line {
                    Ok(Some(request_str)) => {
                        let req: IpcRequest = match serde_json::from_str(&request_str) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };
                        let timeout = op_timeout(&req.op);
                        let result = tokio::time::timeout(
                            timeout,
                            dispatch(&req.op, req.params.clone(), server),
                        ).await;
                        let resp = match result {
                            Ok(Ok(val)) => IpcResponse::success(req.id, val),
                            Ok(Err(e)) => IpcResponse::error(req.id, e.to_string()),
                            Err(_) => IpcResponse::error(req.id, "timeout"),
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
```

**Step 5: Implement `build_session_snapshot`**

Add to `crates/mira-server/src/ipc/handler.rs` (or a new helper file):

```rust
async fn build_session_snapshot(
    server: &MiraServer,
    session_id: &str,
) -> SessionStateSnapshot {
    let pool = server.pool.clone();
    let sid = session_id.to_string();

    // Best-effort: if any query fails, return defaults
    let snapshot = pool.interact(move |conn| {
        let goals = crate::db::tasks::get_active_goals_for_snapshot_sync(conn)
            .unwrap_or_default();
        let injection_stats = crate::db::injection::get_injection_stats_for_session_sync(conn, &sid)
            .unwrap_or_default();
        let modified_files = crate::hooks::get_session_modified_files_sync(conn, &sid)
            .unwrap_or_default();

        SessionStateSnapshot {
            sequence: 0,
            goals,
            injection_stats,
            modified_files,
            team_conflicts: vec![],
        }
    }).await.unwrap_or_else(|_| SessionStateSnapshot {
        sequence: 0,
        goals: vec![],
        injection_stats: InjectionStatsSnapshot { total_injections: 0, total_chars: 0 },
        modified_files: vec![],
        team_conflicts: vec![],
    });

    snapshot
}
```

Note: You will need to add `get_active_goals_for_snapshot_sync` and adapt `get_injection_stats_for_session_sync` to return `InjectionStatsSnapshot`. Follow existing patterns in `db/tasks.rs` and `db/injection.rs`.

**Step 6: Extract `read_request_line` helper**

Refactor the bounded line reading from `handle_connection` into a reusable `read_request_line` function so both transient and persistent handlers can use it.

**Step 7: Run tests**

Run: `cargo test`
Expected: PASS -- existing tests unaffected, subscribe path tested

**Step 8: Commit**

```bash
git add -A
git commit -m "feat: wire SessionChannelRegistry into MiraServer, add subscribe handler with persistent multiplexed connection"
```

---

### Task 4: Add publish calls at DB write points

**Files:**
- Modify: `crates/mira-server/src/ipc/ops.rs`
- Test: `crates/mira-server/src/ipc/tests.rs`

**Step 1: Identify write operations that should publish**

From the design doc, these IPC ops should publish after their DB write:

| Op | Event Type | Critical? |
|----|-----------|-----------|
| `log_behavior` (file_access) | `file_modified` | Yes |
| `record_file_ownership` | `file_conflict` | Yes |
| `auto_link_milestone` | `milestone_completed` | Yes |
| Goal tool updates (via MCP) | `goal_updated` | Yes |
| `register_session` | `session_event` | Yes |
| `close_session` | `session_event` | Yes |
| `log_behavior` (tool_use) | `tool_used` | No |
| Injection recording | `injection_stats` | No |

**Step 2: Add publish helper to ops**

At the top of `crates/mira-server/src/ipc/ops.rs`:

```rust
/// Fire-and-forget publish to session channel. Never fails the parent operation.
async fn publish_event(server: &MiraServer, session_id: &str, event_type: &str, data: serde_json::Value) {
    let event = IpcPushEvent {
        event_type: event_type.to_string(),
        sequence: 0, // Registry assigns real sequence
        data,
    };
    server.channels.publish(session_id, event).await;
}
```

**Step 3: Add publish calls after DB writes**

For each operation listed above, add a `publish_event` call after the successful DB write. Example for `log_behavior`:

```rust
// After the existing DB insert:
if event_type == "file_access" {
    publish_event(
        server,
        &session_id,
        "file_modified",
        serde_json::json!({ "file_path": event_data["file_path"] }),
    ).await;
}
```

Follow the same pattern for each operation. The publish is fire-and-forget -- if no subscriber exists, it's a no-op.

**Step 4: Write test for publish-on-write**

```rust
#[tokio::test]
async fn test_log_behavior_publishes_file_modified_event() {
    let (server, _tmp) = test_server().await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    server.channels.subscribe("sess-1", tx).await;

    // Register session first
    ops::register_session(&server, serde_json::json!({
        "session_id": "sess-1",
        "cwd": "/tmp/test",
        "source": "startup"
    })).await.unwrap();

    // Log a file access
    ops::log_behavior(&server, serde_json::json!({
        "session_id": "sess-1",
        "project_id": 1,
        "event_type": "file_access",
        "event_data": { "file_path": "src/main.rs", "operation": "Edit" }
    })).await.unwrap();

    // Should receive a push event
    let event = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        rx.recv(),
    ).await.unwrap().unwrap();
    assert_eq!(event.event_type, "file_modified");
}
```

**Step 5: Run tests**

Run: `cargo test`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/mira-server/src/ipc/ops.rs crates/mira-server/src/ipc/tests.rs
git commit -m "feat: publish push events to session channels at DB write points"
```

---

## Phase 2: Session Agent (mux)

### Task 5: Add `mira mux` CLI subcommand

**Files:**
- Modify: `crates/mira-server/src/cli/mod.rs` (or wherever CLI is defined)
- Create: `crates/mira-server/src/mux/mod.rs`
- Modify: `crates/mira-server/src/main.rs`
- Modify: `crates/mira-server/src/ipc/mod.rs` (add `pub mod mux;` or keep mux separate)

**Step 1: Add CLI subcommand**

Find the clap `Command` or `Subcommand` enum. Add:

```rust
/// Run session agent (mux) for real-time hook data
Mux {
    /// Session ID to subscribe to
    #[arg(long)]
    session_id: String,
},
```

**Step 2: Create mux module skeleton**

Create `crates/mira-server/src/mux/mod.rs`:

```rust
// crates/mira-server/src/mux/mod.rs
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::ipc::protocol::{
    IpcRequest, IpcResponse, IpcPushEvent, SessionStateSnapshot,
};

mod state;
mod upstream;
mod local;

pub use state::SessionState;

/// Run the session agent mux process.
pub async fn run(session_id: String) -> anyhow::Result<()> {
    let state = Arc::new(RwLock::new(SessionState::default()));

    // 1. Connect upstream and subscribe
    let (upstream_writer, mut event_rx) = upstream::connect_and_subscribe(&session_id, state.clone()).await?;

    // 2. Bind local mux.sock
    let mux_sock_path = mux_socket_path(&session_id);
    let upstream = Arc::new(tokio::sync::Mutex::new(upstream_writer));

    // 3. Write PID file
    write_pid_file(&session_id)?;

    // 4. Serve local connections + relay upstream events
    local::serve(mux_sock_path, state, upstream, session_id).await
}

pub fn mux_socket_path(session_id: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".mira")
        .join("sessions")
        .join(session_id)
        .join("mux.sock")
}

fn write_pid_file(session_id: &str) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let pid_path = PathBuf::from(home)
        .join(".mira")
        .join("sessions")
        .join(session_id)
        .join("mux.pid");
    std::fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}
```

**Step 3: Wire into main.rs**

```rust
Command::Mux { session_id } => {
    crate::mux::run(session_id).await?;
}
```

**Step 4: Commit skeleton**

```bash
git add -A
git commit -m "feat: add mira mux CLI subcommand and module skeleton"
```

---

### Task 6: Implement mux upstream connection

**Files:**
- Create: `crates/mira-server/src/mux/upstream.rs`
- Create: `crates/mira-server/src/mux/state.rs`

**Step 1: Implement SessionState**

Create `crates/mira-server/src/mux/state.rs`:

```rust
// crates/mira-server/src/mux/state.rs
use crate::ipc::protocol::{
    GoalSnapshot, InjectionStatsSnapshot, FileConflictSnapshot,
    SessionStateSnapshot, IpcPushEvent,
};

/// Cached session state maintained by the mux from server push events.
#[derive(Debug, Default, Clone)]
pub struct SessionState {
    pub sequence: u64,
    pub goals: Vec<GoalSnapshot>,
    pub injection_stats: InjectionStatsSnapshot,
    pub modified_files: Vec<String>,
    pub team_conflicts: Vec<FileConflictSnapshot>,
}

impl SessionState {
    /// Initialize from server snapshot.
    pub fn from_snapshot(snapshot: SessionStateSnapshot) -> Self {
        Self {
            sequence: snapshot.sequence,
            goals: snapshot.goals,
            injection_stats: snapshot.injection_stats,
            modified_files: snapshot.modified_files,
            team_conflicts: snapshot.team_conflicts,
        }
    }

    /// Apply an incremental push event.
    pub fn apply_event(&mut self, event: &IpcPushEvent) {
        self.sequence = event.sequence;
        match event.event_type.as_str() {
            "goal_updated" => {
                if let Ok(goal) = serde_json::from_value::<GoalSnapshot>(event.data.clone()) {
                    if let Some(existing) = self.goals.iter_mut().find(|g| g.id == goal.id) {
                        *existing = goal;
                    } else {
                        self.goals.push(goal);
                    }
                }
            }
            "file_modified" => {
                if let Some(path) = event.data["file_path"].as_str() {
                    if !self.modified_files.contains(&path.to_string()) {
                        self.modified_files.push(path.to_string());
                    }
                }
            }
            "file_conflict" => {
                if let Ok(conflict) = serde_json::from_value::<FileConflictSnapshot>(event.data.clone()) {
                    self.team_conflicts.push(conflict);
                }
            }
            "injection_stats" => {
                if let Some(total) = event.data["total_injections"].as_u64() {
                    self.injection_stats.total_injections = total;
                }
                if let Some(chars) = event.data["total_chars"].as_u64() {
                    self.injection_stats.total_chars = chars;
                }
            }
            _ => {} // Unknown events ignored
        }
    }
}
```

**Step 2: Implement upstream connection**

Create `crates/mira-server/src/mux/upstream.rs`:

```rust
// crates/mira-server/src/mux/upstream.rs
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::RwLock;

use crate::ipc::protocol::{
    IpcRequest, IpcResponse, IpcPushEvent, SessionStateSnapshot,
};
use super::state::SessionState;

/// Connect to mira.sock, send subscribe, receive snapshot, return writer + event stream.
pub async fn connect_and_subscribe(
    session_id: &str,
    state: Arc<RwLock<SessionState>>,
) -> anyhow::Result<(
    tokio::io::WriteHalf<UnixStream>,
    tokio::sync::mpsc::Receiver<IpcPushEvent>,
)> {
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
    write_half.write_all(format!("{json}\n").as_bytes()).await?;
    write_half.flush().await?;

    // Read snapshot response
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let resp: IpcResponse = serde_json::from_str(line.trim())?;

    if !resp.ok {
        anyhow::bail!("subscribe failed: {}", resp.error.unwrap_or_default());
    }

    let snapshot: SessionStateSnapshot = serde_json::from_value(
        resp.result.unwrap_or_default()
    )?;

    {
        let mut s = state.write().await;
        *s = SessionState::from_snapshot(snapshot);
    }

    // Spawn reader task: parse events, update state, forward to channel
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut reader = reader;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // Could be a push event or a response to an interleaved query
                    if let Ok(event) = serde_json::from_str::<IpcPushEvent>(line.trim()) {
                        {
                            let mut s = state_clone.write().await;
                            s.apply_event(&event);
                        }
                        let _ = tx.send(event).await;
                    }
                    // IpcResponse lines are handled by the query proxy (Task 7)
                }
                Err(_) => break,
            }
        }
    });

    Ok((write_half, rx))
}
```

**Step 3: Run tests**

Run: `cargo check`
Expected: PASS (compiles)

**Step 4: Commit**

```bash
git add crates/mira-server/src/mux/state.rs crates/mira-server/src/mux/upstream.rs
git commit -m "feat: implement mux upstream connection with subscribe and state tracking"
```

---

### Task 7: Implement mux local socket server

**Files:**
- Create: `crates/mira-server/src/mux/local.rs`

**Step 1: Implement local server**

Create `crates/mira-server/src/mux/local.rs`:

```rust
// crates/mira-server/src/mux/local.rs
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;

use crate::ipc::protocol::{IpcRequest, IpcResponse, IpcPushEvent};
use super::state::SessionState;

/// Serve local hook connections on mux.sock.
pub async fn serve(
    sock_path: PathBuf,
    state: Arc<RwLock<SessionState>>,
    upstream_writer: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    session_id: String,
) -> anyhow::Result<()> {
    // Clean stale socket
    let _ = std::fs::remove_file(&sock_path);
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    // Set permissions (owner-only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))?;
    }

    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let inactivity_timeout = Duration::from_secs(300); // 5 minutes

    loop {
        tokio::select! {
            Ok((stream, _)) = listener.accept() => {
                *last_activity.lock().await = Instant::now();
                let state = state.clone();
                let upstream = upstream_writer.clone();
                tokio::spawn(async move {
                    handle_local_connection(stream, state, upstream).await;
                });
            }
            _ = tokio::time::sleep_until(*last_activity.lock().await + inactivity_timeout) => {
                eprintln!("[mira/mux] Shutting down after 5 minutes of inactivity");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&sock_path);
    Ok(())
}

async fn handle_local_connection(
    stream: tokio::net::UnixStream,
    state: Arc<RwLock<SessionState>>,
    upstream: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
        let req: IpcRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(_) => {
                line.clear();
                continue;
            }
        };

        let resp = match req.op.as_str() {
            "read_state" => {
                // Read from local cache -- instant
                let s = state.read().await;
                let keys = req.params["keys"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                let mut result = serde_json::Map::new();
                for key in &keys {
                    match *key {
                        "goals" => {
                            result.insert("goals".into(), serde_json::to_value(&s.goals).unwrap_or_default());
                        }
                        "modified_files" => {
                            result.insert("modified_files".into(), serde_json::to_value(&s.modified_files).unwrap_or_default());
                        }
                        "injection_stats" => {
                            result.insert("injection_stats".into(), serde_json::to_value(&s.injection_stats).unwrap_or_default());
                        }
                        "team_conflicts" => {
                            result.insert("team_conflicts".into(), serde_json::to_value(&s.team_conflicts).unwrap_or_default());
                        }
                        "sequence" => {
                            result.insert("sequence".into(), serde_json::json!(s.sequence));
                        }
                        _ => {}
                    }
                }
                IpcResponse::success(req.id, serde_json::Value::Object(result))
            }
            _ => {
                // Proxy to upstream server
                match proxy_to_upstream(&req, &upstream).await {
                    Ok(resp) => resp,
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
        };

        let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
        if writer.write_all(format!("{json}\n").as_bytes()).await.is_err() {
            break;
        }
        if writer.flush().await.is_err() {
            break;
        }

        line.clear();
    }
}

async fn proxy_to_upstream(
    req: &IpcRequest,
    upstream: &Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
) -> anyhow::Result<IpcResponse> {
    // Note: This is simplified. A production version needs response correlation
    // since the upstream connection is multiplexed (push events + responses).
    // For now, send the request and rely on the upstream reader task to
    // route responses back. This will be refined in Task 8.
    let mut writer = upstream.lock().await;
    let json = serde_json::to_string(req)?;
    writer.write_all(format!("{json}\n").as_bytes()).await?;
    writer.flush().await?;

    // TODO: Task 8 will add response correlation via pending_requests map
    Ok(IpcResponse::error(req.id.clone(), "proxy not yet implemented"))
}
```

**Step 2: Run tests**

Run: `cargo check`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/mira-server/src/mux/local.rs
git commit -m "feat: implement mux local socket server with read_state and proxy stubs"
```

---

### Task 8: Implement request-response correlation for proxied queries

**Files:**
- Modify: `crates/mira-server/src/mux/upstream.rs`
- Modify: `crates/mira-server/src/mux/local.rs`

The upstream connection is multiplexed: it receives both push events and responses to proxied queries. We need to correlate responses to pending requests by ID.

**Step 1: Add pending requests map to upstream**

In `upstream.rs`, add a shared `pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>>` that the reader task checks. When it reads a line that parses as `IpcResponse`, it looks up the ID in the map and sends the response to the waiting oneshot. When it parses as `IpcPushEvent`, it updates state and forwards to the event channel.

**Step 2: Update proxy_to_upstream in local.rs**

```rust
async fn proxy_to_upstream(
    req: &IpcRequest,
    upstream: &Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    pending: &Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>>,
) -> anyhow::Result<IpcResponse> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    {
        let mut map = pending.lock().await;
        map.insert(req.id.clone(), tx);
    }

    {
        let mut writer = upstream.lock().await;
        let json = serde_json::to_string(req)?;
        writer.write_all(format!("{json}\n").as_bytes()).await?;
        writer.flush().await?;
    }

    // Wait for response with timeout
    match tokio::time::timeout(Duration::from_secs(30), rx).await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(_)) => anyhow::bail!("upstream closed"),
        Err(_) => {
            pending.lock().await.remove(&req.id);
            anyhow::bail!("upstream timeout")
        }
    }
}
```

**Step 3: Update upstream reader task**

```rust
// In the reader loop:
let trimmed = line.trim();
if let Ok(resp) = serde_json::from_str::<IpcResponse>(trimmed) {
    // Check if this is a response to a pending request
    let mut map = pending_requests.lock().await;
    if let Some(tx) = map.remove(&resp.id) {
        let _ = tx.send(resp);
    }
} else if let Ok(event) = serde_json::from_str::<IpcPushEvent>(trimmed) {
    let mut s = state_clone.write().await;
    s.apply_event(&event);
    let _ = tx.send(event).await;
}
```

**Step 4: Run tests**

Run: `cargo check`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/mira-server/src/mux/upstream.rs crates/mira-server/src/mux/local.rs
git commit -m "feat: add request-response correlation for proxied queries through mux"
```

---

### Task 9: Add mux spawning to SessionStart hook

**Files:**
- Modify: `crates/mira-server/src/hooks/session/mod.rs`

**Step 1: Add mux spawn helper**

```rust
/// Spawn the mux process for this session if not already running.
fn spawn_mux(session_id: &str) {
    let mux_sock = crate::mux::mux_socket_path(session_id);
    if mux_sock.exists() {
        // Check if the socket is live
        if std::os::unix::net::UnixStream::connect(&mux_sock).is_ok() {
            return; // Already running
        }
        // Stale socket -- remove it
        let _ = std::fs::remove_file(&mux_sock);
    }

    // Spawn detached mux process
    let mira_bin = std::env::current_exe().unwrap_or_else(|_| "mira".into());
    match std::process::Command::new(mira_bin)
        .args(["mux", "--session-id", session_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => {
            eprintln!("[Mira/mux] Spawned session agent for {session_id}");
        }
        Err(e) => {
            eprintln!("[Mira/mux] Failed to spawn session agent: {e}");
        }
    }
}
```

**Step 2: Call from SessionStart**

Near the end of the SessionStart handler, after session registration:

```rust
spawn_mux(&session_id);
```

**Step 3: Add shutdown to Stop hook**

In `crates/mira-server/src/hooks/stop.rs`, add mux shutdown before `clear_session_identity`:

```rust
// Send shutdown to mux if running
let mux_sock = crate::mux::mux_socket_path(&session_id);
if mux_sock.exists() {
    if let Ok(stream) = tokio::net::UnixStream::connect(&mux_sock).await {
        let req = IpcRequest {
            op: "shutdown".to_string(),
            id: "stop".to_string(),
            params: serde_json::Value::Null,
        };
        // Best-effort shutdown
        let _ = /* send req */;
    }
}
```

Also add `"shutdown"` handling to the mux's local connection handler (break the serve loop).

**Step 4: Run tests**

Run: `cargo test`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/mira-server/src/hooks/session/mod.rs crates/mira-server/src/hooks/stop.rs
git commit -m "feat: spawn mux on SessionStart, shutdown on Stop"
```

---

## Phase 3: Hook Migration

### Task 10: Update HookClient to try mux.sock first

**Files:**
- Modify: `crates/mira-server/src/ipc/client/mod.rs`

**Step 1: Update connect() to check mux.sock**

In `HookClient::connect()`, before trying `mira.sock`:

```rust
// Try mux.sock first (if session_id available)
if let Some(session_id) = Self::read_session_id() {
    let mux_sock = crate::mux::mux_socket_path(&session_id);
    if let Ok(stream) = tokio::time::timeout(
        Duration::from_millis(50), // Even faster than mira.sock -- it's local
        UnixStream::connect(&mux_sock),
    ).await {
        if let Ok(stream) = stream {
            return Self {
                inner: Backend::Ipc(IpcStream::new(/* ... */)),
                session_id: Some(session_id),
                via_mux: true,
            };
        }
    }
}
// Existing mira.sock fallback...
```

**Step 2: Add read_state shortcut**

Add a method to HookClient for reading cached state from the mux:

```rust
/// Read cached state from mux. Returns None if not connected via mux.
pub async fn read_cached_state(&mut self, keys: &[&str]) -> Option<serde_json::Value> {
    if !self.via_mux {
        return None;
    }
    self.call("read_state", serde_json::json!({ "keys": keys })).await.ok()
}
```

**Step 3: Run tests**

Run: `cargo test`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/mira-server/src/ipc/client/mod.rs
git commit -m "feat: HookClient tries mux.sock first with read_cached_state shortcut"
```

---

### Task 11: Migrate UserPromptSubmit to use mux cache

**Files:**
- Modify: `crates/mira-server/src/hooks/user_prompt.rs`

**Step 1: Use cached goals from mux when available**

In the UserPromptSubmit handler, before calling `get_user_prompt_context()`:

```rust
// Try fast path: read cached state from mux
if let Some(cached) = client.read_cached_state(&["goals", "modified_files", "team_conflicts"]).await {
    // Use cached data for context building instead of full DB round-trip
    // Only fall through to get_user_prompt_context() for reactive search (requires live query)
}
```

This is a targeted optimization -- the reactive context (semantic search based on user message) still needs a live query, but goals, modified files, and team conflicts come from cache.

**Step 2: Run tests**

Run: `cargo test`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/mira-server/src/hooks/user_prompt.rs
git commit -m "feat: UserPromptSubmit reads cached goals and files from mux"
```

---

### Task 12: Migrate SubagentStart to use mux cache

**Files:**
- Modify: `crates/mira-server/src/hooks/subagent.rs`

**Step 1: Use cached project map from mux**

For narrow subagents, read `modified_files` from cache. For full subagents, read `goals` from cache. The `search_for_subagent` and `generate_bundle` calls still proxy through to the server (query-shaped, not cacheable).

**Step 2: Run tests**

Run: `cargo test`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/mira-server/src/hooks/subagent.rs
git commit -m "feat: SubagentStart reads cached goals and project state from mux"
```

---

## Phase 4: Integration Testing

### Task 13: End-to-end integration test

**Files:**
- Create: `crates/mira-server/src/mux/tests.rs`

**Step 1: Write integration test**

```rust
#[tokio::test]
async fn test_mux_end_to_end() {
    // 1. Start a test MiraServer
    // 2. Start IPC listener on a temp socket
    // 3. Start mux process pointing at that socket
    // 4. Connect a mock hook to mux.sock
    // 5. Send read_state -- verify empty initial state
    // 6. Trigger a DB write via IPC (log_behavior with file_access)
    // 7. Wait briefly for push propagation
    // 8. Send read_state again -- verify file appears in modified_files
    // 9. Send a proxied query (search_for_subagent) -- verify response
    // 10. Send shutdown -- verify mux exits cleanly
}
```

**Step 2: Run tests**

Run: `cargo test test_mux_end_to_end -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/mira-server/src/mux/tests.rs
git commit -m "test: end-to-end integration test for mux subscribe-push-read cycle"
```

---

### Task 14: Update documentation

**Files:**
- Modify: `CLAUDE.md` -- update SubagentStart hook description
- Modify: `.claude/rules/sub-agents.md` -- mention mux
- Modify: `CHANGELOG.md` -- add entry for next version

**Step 1: Update CLAUDE.md hook table**

Change SubagentStart description from:
> Injects active goals for subagent context

To:
> Injects goals, project map, and search hints for subagent context

**Step 2: Commit**

```bash
git add CLAUDE.md .claude/rules/sub-agents.md
git commit -m "docs: update hook descriptions for real-time session channels"
```

---

## Task Dependency Graph

```
Task 1 (protocol types)
  └→ Task 2 (channel registry)
       └→ Task 3 (wire into server + subscribe handler)
            └→ Task 4 (publish at write points)
                 └→ Task 5 (CLI subcommand)
                      └→ Task 6 (upstream connection)
                      └→ Task 7 (local socket server)
                           └→ Task 8 (request correlation)
                                └→ Task 9 (spawn/shutdown lifecycle)
                                     └→ Task 10 (HookClient mux discovery)
                                          └→ Task 11 (UserPromptSubmit migration)
                                          └→ Task 12 (SubagentStart migration)
                                               └→ Task 13 (integration test)
                                                    └→ Task 14 (docs)
```
