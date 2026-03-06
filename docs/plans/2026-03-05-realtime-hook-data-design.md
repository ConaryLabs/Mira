<!-- docs/plans/2026-03-05-realtime-hook-data-design.md -->
# Real-Time Hook Data via Server-Side Session Channels

## Problem

Hooks currently use one-shot IPC connections: connect to `mira.sock`, send request, receive response, disconnect. Every hook invocation pays connection overhead, and there is no way for the server to push state updates to hooks proactively. This makes assist counts stale, subagent context slow to materialize, and hook-to-hook data flow indirect (always round-tripping through the DB).

## Decision

Replace one-shot IPC with **server-side session channels** and a **session agent (mux) process** per session. The MCP server gains persistent subscription connections with push semantics. A lightweight mux process holds the persistent connection and exposes a local socket for hooks to read cached state instantly.

### Alternatives Considered

**Thin mux (cache + proxy):** Mux holds a persistent connection and caches pushed state, proxies queries. Simple but puts no subscription logic in the server -- the mux would need to understand what to cache. Business logic stays in the server but cache invalidation is harder.

**Smart mux (materialized views):** Mux pre-builds response payloads from event streams. Fastest reads but duplicates business logic between mux and server. Version skew risk.

**Server-side sessions (chosen):** Server manages subscriptions and pushes structured state updates. Mux is a thin relay with local cache. All logic stays in the server. Most invasive but cleanest long-term architecture.

## Architecture

```
Hook --> mux.sock --> Session Agent ==persistent==> mira.sock (MCP Server)
                        (cache)        subscribe
                                       <-- snapshot
                                       <-- event
                                       <-- event
                        query -------> request -->
                        result <------- response <--
```

### Protocol Evolution

The IPC protocol supports two connection modes on the same `mira.sock`:

**Transient mode (unchanged):** Connect, send request, receive response, disconnect. All 27 existing operations work as-is. Zero breaking changes.

**Persistent mode:** Client sends `subscribe(session_id)`. Server keeps the connection open and:
- Immediately sends a full state snapshot (assists, goals, file modifications, team state, injection stats)
- Pushes incremental updates as state changes
- Accepts interleaved request-response queries on the same connection

### Server-Side Session Channels

**SessionChannel** -- one per subscribed session:
- Holds the persistent `tokio::net::UnixStream` writer half
- Monotonic sequence number per pushed event
- Tracks subscribed event types (initially: all)

**Push triggers** -- at each DB write point, the server publishes to the session channel:
- `record_injection` -> injection stats update
- `log_behavior` -> file_modified / tool_used events
- `update_goal` / `complete_milestone` -> goal state
- `record_file_ownership` -> team conflict updates
- `register_session` -> session metadata

**Backpressure:** If the session agent's write buffer is full, drop non-critical events (injection stats, behavior log). Never drop goal or file conflict updates. If the connection dies, remove the channel. Next reconnect gets a fresh snapshot.

**Concurrency:** Persistent connections get dedicated `tokio::spawn` tasks. They do NOT consume the 16-slot semaphore reserved for transient connections.

### Session Agent (mux process)

A mode of the `mira` binary: `mira mux --session <id>`. Does three things:

**1. Upstream persistent connection.** Connects to `mira.sock`, sends `subscribe(session_id)`, receives snapshot, listens for events. Maintains a `SessionState` struct in memory.

**2. Local socket.** Exposes `~/.mira/sessions/{session_id}/mux.sock`. Two request types:
- `read_state(keys)` -- instant response from cache, no upstream round-trip
- `query(op, params)` -- proxied through persistent connection to server

**3. Lifecycle:**
- Spawned lazily by the first hook that looks for `mux.sock` and doesn't find it. Fork+detach, PID written to `~/.mira/sessions/{session_id}/mux.pid`.
- Self-terminates on: shutdown command from Stop hook, upstream reconnect fails 3 times, or 5 minutes of inactivity.
- Crash recovery: hooks finding stale `mux.sock` (connect fails) remove it and spawn a new mux. Fresh snapshot on re-subscribe.

**Hook discovery:** Try `mux.sock` first. If unavailable, SessionStart spawns the mux; other hooks fall back to direct `mira.sock`. Existing direct-IPC fallback is the safety net.

## Migration Plan

### Phase 1: Infrastructure

Add subscribe protocol to server, build session agent binary, wire SessionStart to spawn it. All hooks continue using direct IPC. Validates persistent connection and push mechanics in isolation.

### Phase 2: Read-Heavy Hooks Migrate

Update `HookClient::connect()` to try `mux.sock` first, fall back to `mira.sock`. Hook code unchanged -- same API, but reads hit mux cache. Priority hooks:
- **UserPromptSubmit** -- reads goals, tasks, behavior, observations every prompt
- **PreToolUse** -- reads file modification state
- **SubagentStart** -- reads project map from cache, proxies search queries

### Phase 3: Write Hooks Publish

PostToolUse, Stop, and other state-writing hooks write through the mux, so the server pushes updates to the session channel immediately.

### Phase 4: Simplify

Remove direct DB fallback from migrated hooks. Direct `mira.sock` fallback stays for mux-down scenarios. Hook code gets significantly simpler.

**Key property:** At every phase, the system works if the mux isn't running. Hooks fall back to current behavior. No big-bang cutover.

## Pushed State Categories

| Category | Push trigger | Critical? |
|----------|-------------|-----------|
| Assist count | `record_injection` | No |
| Active goals | `update_goal`, `complete_milestone` | Yes |
| File modifications | `log_behavior(file_access)` | Yes |
| Team conflicts | `record_file_ownership`, `get_file_conflicts` | Yes |
| Injection stats | `record_injection` | No |
| Tool usage | `log_behavior(tool_use)` | No |
| Session events | `register_session`, `close_session` | Yes |

Non-critical events can be dropped under backpressure. Critical events are never dropped -- if delivery fails, the connection is considered dead and the channel is removed.
