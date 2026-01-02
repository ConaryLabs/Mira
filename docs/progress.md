# Mira Studio Progress

## Overview

Mira Studio is a Rust-native web frontend for Mira, built with Leptos (WASM). It provides a visual interface for Ghost Mode (real-time agent reasoning visualization), memory management, code intelligence, and task tracking.

## Completed (2025-12-31)

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

### Phase 6: Diff Viewer & MCP Bridge
- [x] Syntax highlighting with highlight.js (CDN)
- [x] Language detection from file extension
- [x] MCP → WebSocket bridge (`broadcaster.rs`)
- [x] `/api/broadcast` endpoint for event injection
- [x] Tool calls stream to Ghost Mode in real-time
- [x] Test Diff button for UI verification
- [x] `.env` file loading (`~/.mira/.env` and project root)
- [ ] Approve/Edit/Reject buttons for diffs (deferred)

### Phase 8: Session-Aware Event History
- [x] `tool_history` table with session_id for event persistence
- [x] `session_history` MCP tool (current, list_sessions, get_history)
- [x] Shared session_id between MCP and web server
- [x] Event replay on Ghost Mode connect (last 50 events)
- [x] WebSocket sync protocol for reconnection
- [x] Exponential backoff reconnection (1s → 30s)

### Phase 9: Single Binary Packaging
- [x] rust-embed for assets (`web/embedded.rs`)
- [x] Embed WASM in server binary (24MB self-contained)
- [x] Updated build script with correct build order

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
│   │       ├── broadcaster.rs # MCP → WebSocket bridge
│   │       ├── web/        # Web server layer
│   │       │   ├── mod.rs  # Router
│   │       │   ├── api.rs  # REST endpoints + /api/broadcast
│   │       │   ├── ws.rs   # WebSocket handler
│   │       │   ├── embedded.rs # rust-embed assets (single binary)
│   │       │   └── state.rs# AppState
│   │       └── ...
│   └── mira-app/           # WASM frontend
│       └── src/lib.rs      # Leptos components + highlight.js bindings
├── assets/
│   ├── style.css           # Terminal theme
│   └── index.html          # WASM shell + highlight.js CDN
├── pkg/                    # Built WASM output
└── build-studio.sh         # Build script
```

### Event Flow (Ghost Mode)
```
Claude Code → mira serve (MCP) → broadcaster.rs → POST /api/broadcast
                                                          ↓
Browser ← WebSocket ← ws.rs ← AppState.broadcast() ←──────┘
```

## Running

```bash
# Build everything (creates single 24MB binary)
./build-studio.sh

# Or manually (order matters - WASM must be built first):
wasm-pack build --target web crates/mira-app --out-dir ../../pkg
cargo build --release -p mira-server

# Run web server (no external files needed)
./target/release/mira web --port 3000
```

The binary is self-contained - assets and WASM are embedded via rust-embed.
You can copy the binary anywhere and run it.

### Phase 7: Terminal Mirror Enhancement
- [x] ANSI color parsing (16 + 256 color support, bold/dim/italic/underline)
- [x] Scrollback buffer (1000 lines with auto-scroll to bottom)

### Phase 10: cargo-leptos Integration
- [x] Leptos.toml configuration for cargo-leptos
- [x] Updated build-studio.sh with --leptos and --watch options
- [x] Hot reloading support via `cargo leptos watch`

### Phase 11: Session Context on Startup
- [x] session_start shows last 3 sessions with timestamps
- [x] Tool call count and top tools per session
- [x] Session summary display (when available)
- [x] Database path in startup response
- [x] Pointer to session_history tool for detailed inspection

### Phase 12: Persona Overlay System
- [x] Stackable persona system for DeepSeek chat
- [x] Base persona (OG Mira personality) - always active
- [x] Project overlay - auto-applies when project is set
- [x] Session overlay - ephemeral, manual via API
- [x] Database methods: get/set_base_persona, get/set/clear_project_persona
- [x] New module: `persona.rs` with DEFAULT_BASE_PERSONA
- [x] API endpoints: GET /api/persona, POST /api/persona/session
- [x] KV cache efficient (832/849 tokens cached on first request)

## Remaining Work

### Deferred: Scout/Solve AI Pipeline
- [ ] Scout mode (exploration/planning)
- [ ] Solve mode (implementation)
- [ ] AI-driven workflow orchestration
