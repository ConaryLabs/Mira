# Leptos Studio Implementation Plan

**Goal:** Add a Rust-native web frontend to Mira using Leptos, achieving the full vision from the design doc (Ghost Mode, Diff Viewer, Terminal Mirror, Queue & Sync).

**Scope:** UI only - Scout/Solve AI pipeline deferred to future phase.

**Date:** 2025-12-31

---

## Architecture Decision

**Integrated model:** `mira web` runs HTTP server with MCP available via internal calls. Single process, shared database connection.

**Workspace structure:**
```
/home/peter/Mira/
├── Cargo.toml              # Workspace root (already exists)
├── crates/
│   ├── mira-types/         # Shared types (native + WASM)
│   ├── mira-server/        # Current src/ (MCP + HTTP + DB)
│   └── mira-app/           # WASM frontend (Leptos components)
├── assets/                 # CSS, icons
└── style/                  # Tailwind config
```

---

## Phase 1: Workspace Restructure

**Files to modify:**
- `/home/peter/Mira/Cargo.toml` - Convert to workspace root
- Create `crates/mira-types/Cargo.toml`
- Create `crates/mira-server/Cargo.toml`
- Create `crates/mira-app/Cargo.toml`
- Move `src/` to `crates/mira-server/src/`

**mira-types contents:**
```rust
// Shared between server and WASM
pub struct MemoryFact { ... }
pub struct Symbol { ... }
pub struct Task { ... }
pub struct Goal { ... }
pub struct ProjectContext { ... }

// WebSocket events
pub enum WsEvent {
    Thinking { content: String, phase: ThinkingPhase },
    ToolStart { tool_name: String, arguments: Value },
    ToolResult { tool_name: String, result: String, success: bool },
    DiffPreview { file_path: String, diff: UnifiedDiff },
    FileChange { file_path: String, change_type: ChangeType },
    SessionUpdate { ... },
    Error { ... },
}

// API request/response types
pub struct RememberRequest { ... }
pub struct RecallRequest { ... }
// etc.
```

---

## Phase 2: Web Server Layer

**Add to `crates/mira-server/src/`:**
```
web/
├── mod.rs          # Router setup
├── state.rs        # AppState with broadcast channel
├── api.rs          # REST endpoints (reuse MCP tool logic)
├── ws.rs           # WebSocket handler for Ghost Mode
└── pages.rs        # Leptos SSR pages
```

**New CLI command in main.rs:**
```rust
Commands::Web {
    #[arg(short, long, default_value = "3000")]
    port: u16,
}
```

**Router structure:**
```rust
Router::new()
    // API (REST)
    .route("/api/memories", get(list).post(create))
    .route("/api/recall", post(recall))
    .route("/api/symbols/:path", get(symbols))
    .route("/api/search/code", post(semantic_search))
    .route("/api/tasks", get(list).post(create))
    .route("/api/goals", get(list).post(create))
    .route("/api/index", post(trigger_index))

    // WebSocket
    .route("/ws", get(ws_handler))

    // Leptos SSR
    .route("/", get(home))
    .route("/ghost", get(ghost_mode))

    // Static
    .nest_service("/pkg", ServeDir::new("target/site/pkg"))
```

---

## Phase 3: WebSocket Protocol

**Event broadcasting:**
```rust
pub struct AppState {
    pub db: Arc<Database>,           // Holds sqlite-vec connection
    pub embeddings: Option<Arc<Embeddings>>,
    pub ws_tx: broadcast::Sender<WsEvent>,
}

impl AppState {
    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }
}
```

**Important:** `Database::open()` already loads the sqlite-vec extension. The `AppState` holds `Arc<Database>` so WebSocket handlers can query `vec_memory` and `vec_code` tables for semantic search (needed for Scout phase later).

**Client reconnection (Queue & Sync):**
- Store events in `events` table with sequence ID
- Client sends `{ "sync": last_event_id }` on reconnect
- Server replays missed events

---

## Phase 4: Leptos Components (mira-app)

**Component hierarchy:**
```
App
├── Layout (nav, sidebar)
├── HomePage
├── GhostModePage
│   ├── ThinkingPanel (accordion, collapsible reasoning stream)
│   ├── ToolCallTimeline (tool executions with timing)
│   ├── DiffViewer (side-by-side with syntax highlighting)
│   └── TerminalMirror (shell output display)
├── MemoriesPage
├── CodePage (symbols, search)
└── TasksPage
```

**Ghost Mode accordion:**
- Collapsed: `"Analyzing dependency graph..."` (summary)
- Expanded: Raw reasoning stream (monospace)
- Sticky insights pinned to chat log

**Diff viewer:**
- Unified diff format
- Syntax highlighting (tree-sitter via WASM or highlight.js)
- Approve/Edit/Reject buttons

---

## Phase 5: Single Binary Build

**Using cargo-leptos + rust-embed:**

```toml
# Leptos.toml
[package]
name = "mira"
bin-package = "mira-server"
lib-package = "mira-app"
site-root = "target/site"

# Tailwind auto-compilation during watch
tailwind-input-file = "style/input.css"
tailwind-config-file = "tailwind.config.js"
```

**Build command:**
```bash
cargo leptos build --release
# Result: target/release/mira (single binary with embedded WASM)
```

---

## Implementation Order

### Step 1: Workspace setup
- [ ] Create `crates/` directory structure
- [ ] Move `src/` to `crates/mira-server/src/`
- [ ] Create `mira-types` with extracted types
- [ ] Update import paths
- [ ] Verify `cargo build` works

### Step 2: Basic web server
- [ ] Add `web/mod.rs` with axum router
- [ ] Add `Commands::Web` to CLI
- [ ] Implement `/api/memories` endpoint (test with curl)
- [ ] Verify database sharing works

### Step 3: WebSocket infrastructure
- [ ] Add `web/ws.rs` with broadcast channel
- [ ] Define `WsEvent` enum in mira-types
- [ ] Test with websocat

### Step 4: Leptos SSR scaffold
- [ ] Add leptos dependencies
- [ ] Create basic `HomePage` component
- [ ] Serve via axum integration

### Step 5: WASM frontend (mira-app)
- [ ] Set up mira-app crate
- [ ] Implement `GhostModePage` with WebSocket
- [ ] Add `ThinkingPanel` accordion
- [ ] Add `ToolCallTimeline`

### Step 6: Diff Viewer
- [ ] Implement `UnifiedDiff` type
- [ ] Create `DiffViewer` component
- [ ] Add syntax highlighting

### Step 7: Terminal Mirror
- [ ] Add shell output event type
- [ ] Create `TerminalMirror` component

### Step 8: Queue & Sync (resilience)
- [ ] Add `events` table to schema
- [ ] Implement event journaling
- [ ] Add reconnection protocol

### Step 9: Single binary packaging
- [ ] Configure cargo-leptos
- [ ] Set up rust-embed for assets
- [ ] Test single binary deployment

---

## Key Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Workspace members, leptos deps |
| `src/main.rs` → `crates/mira-server/src/main.rs` | Add Web command |
| `src/db.rs` → `crates/mira-server/src/db.rs` | Extract types to mira-types |
| `src/mcp/mod.rs` → `crates/mira-server/src/mcp/mod.rs` | Extract request types |
| NEW `crates/mira-types/src/lib.rs` | Shared types |
| NEW `crates/mira-server/src/web/mod.rs` | HTTP router |
| NEW `crates/mira-server/src/web/ws.rs` | WebSocket handler |
| NEW `crates/mira-app/src/lib.rs` | Leptos app root |
| NEW `crates/mira-app/src/components/ghost_mode.rs` | Ghost Mode UI |

---

## Dependencies to Add

**mira-server:**
```toml
leptos = { version = "0.7", features = ["ssr"] }
leptos_axum = "0.7"
rust-embed = "8.0"
```

**mira-app:**
```toml
leptos = { version = "0.7", features = ["csr", "hydrate"] }
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = ["WebSocket", "MessageEvent"] }
```

**mira-types:**
```toml
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
# No native-only deps - must compile to WASM
```

---

## Context: Original Design Doc

This plan implements the UI layer from the "Mira Studio" design doc (v0.2, December 2025). Key concepts:

1. **Ghost Mode** - Real-time visualization of agent reasoning with accordion UI
2. **Dependency-Aware Pipeline** - Cartographer (AST) → Scout (Gemini) → Engineer (DeepSeek) [deferred]
3. **Jail & Allowlist Security** - Capabilities-based command execution
4. **Queue & Sync** - Browser disconnect resilience with event journaling
5. **Single Binary** - Local-first, zero-dependency distribution

The Scout/Solve AI pipeline is deferred to a future phase. This plan focuses on the Leptos frontend infrastructure.

---

## Open Questions for Review

1. **Leptos version**: Plan uses 0.7 - confirm this is stable enough or use 0.6?
2. ~~**Styling**: Tailwind vs plain CSS?~~ **RESOLVED:** Tailwind with cargo-leptos auto-compilation
3. **Tree-sitter in WASM**: For syntax highlighting in diff viewer - worth the complexity or use highlight.js?
4. **SSR vs CSR**: Plan uses hybrid (SSR for initial load, hydration for interactivity). Pure CSR simpler?

## Gemini Pro Review (2025-12-31)

**Incorporated feedback:**
- Added `tailwind-input-file` config to Leptos.toml for auto-compilation during watch
- Documented that `AppState.db` holds sqlite-vec connection for future Scout semantic queries

---

*Plan created: 2025-12-31*
*Review requested: Gemini Pro*
