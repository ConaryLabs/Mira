# Mira Studio Progress

## Overview

Mira Studio is a Rust-native web frontend for Mira, built with Leptos (WASM). It provides a visual interface for Ghost Mode (real-time agent reasoning visualization), memory management, code intelligence, and task tracking.

## Completed (2024-12-31)

### Phase 1: Workspace Restructure
- [x] Created `crates/` directory structure
- [x] Moved `src/` to `crates/mira-server/src/`
- [x] Created `mira-types` crate with shared types (native + WASM compatible)
- [x] Created `mira-app` crate for WASM frontend
- [x] Updated root `Cargo.toml` for workspace

### Phase 2: Web Server Layer
- [x] Added `web/mod.rs` with axum router
- [x] Added `web/state.rs` with AppState (broadcast channel for WebSocket)
- [x] Added `web/api.rs` with REST endpoints
- [x] Added `Commands::Web` to CLI (`mira web --port 3000`)
- [x] Static file serving for assets and pkg directories

### Phase 3: WebSocket Infrastructure
- [x] Added `web/ws.rs` with WebSocket handler
- [x] Defined `WsEvent` enum in mira-types for Ghost Mode streaming
- [x] Broadcast channel for real-time event distribution

### Phase 4: Leptos SSR Scaffold
- [x] Added Leptos dependencies (0.8.x)
- [x] Created `web/components/mod.rs` with server-side components
- [x] Created `assets/style.css` with terminal theme

### Phase 5: WASM Frontend (mira-app)
- [x] Set up Leptos CSR (client-side rendering) app
- [x] Implemented all page components:
  - `HomePage` - Dashboard with server health check
  - `GhostModePage` - Real-time agent visualization
  - `MemoriesPage` - Semantic memory search
  - `CodePage` - Semantic code search
  - `TasksPage` - Goals and task management
- [x] Ghost Mode components:
  - `ThinkingPanel` - Accordion for agent reasoning stream
  - `ToolTimeline` - Tool call execution tracking
  - `DiffViewer` - Unified diff display
  - `TerminalMirror` - Shell output display
- [x] WebSocket connection for live events
- [x] REST API integration for data fetching
- [x] Connection status indicator

### Build System
- [x] `wasm-pack` integration for WASM builds
- [x] Created `build-studio.sh` script
- [x] Server auto-detects WASM files and serves appropriate HTML

## Architecture

```
/home/peter/Mira/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── mira-types/         # Shared types (native + WASM)
│   │   └── src/lib.rs      # MemoryFact, WsEvent, Task, Goal, etc.
│   ├── mira-server/        # HTTP server + MCP
│   │   └── src/
│   │       ├── main.rs     # CLI entry (serve, web, connect)
│   │       ├── web/        # Web server layer
│   │       │   ├── mod.rs  # Router
│   │       │   ├── api.rs  # REST endpoints
│   │       │   ├── ws.rs   # WebSocket handler
│   │       │   └── state.rs# AppState
│   │       └── ...
│   └── mira-app/           # WASM frontend
│       └── src/lib.rs      # Leptos components
├── assets/
│   └── style.css           # Terminal theme
├── pkg/                    # Built WASM output
└── build-studio.sh         # Build script
```

## Running

```bash
# Build everything
./build-studio.sh

# Or manually:
wasm-pack build --target web crates/mira-app --out-dir ../../pkg
cargo build --release

# Run web server
./target/release/mira web --port 3000
```

## Remaining Work

### Phase 6: Diff Viewer Enhancement
- [ ] Syntax highlighting (tree-sitter or highlight.js)
- [ ] Approve/Edit/Reject buttons for diffs

### Phase 7: Terminal Mirror Enhancement
- [ ] ANSI color parsing
- [ ] Scrollback buffer

### Phase 8: Queue & Sync (Resilience)
- [ ] Add `events` table to schema for journaling
- [ ] Client reconnection with event replay
- [ ] Sync protocol (`{ "sync": last_event_id }`)

### Phase 9: Single Binary Packaging
- [ ] rust-embed for assets
- [ ] Embed WASM in server binary
- [ ] cargo-leptos integration (optional)

### Deferred: Scout/Solve AI Pipeline
- [ ] Scout mode (exploration/planning)
- [ ] Solve mode (implementation)
- [ ] AI-driven workflow orchestration
