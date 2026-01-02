# Tauri Migration Plan: Leptos → Svelte + Tauri Desktop App

## Overview

Migrate from monolithic Leptos/WASM frontend to Tauri desktop app with Svelte frontend.

- **Goal**: Instant hot reload during dev, single binary desktop app for distribution
- **Strategy**: Keep Axum backend as-is (frontend talks via HTTP), convert to Tauri IPC later if needed

## Why

- WASM compile times are brutal for UI iteration
- Modern JS frameworks have years of DX polish (hot reload, devtools, ecosystem)
- Tauri gives us: Rust backend + modern frontend + single binary distribution
- Best of both worlds: fast dev iteration AND easy distribution

## Phase 1: Tauri + Svelte Project Setup

- [ ] Initialize Tauri project alongside existing code
- [ ] Set up Svelte + Vite + TailwindCSS
- [ ] Configure dev server with hot reload
- [ ] Verify Tauri can spawn and communicate with mira-server
- [ ] Basic "hello world" window opening

## Phase 2: Core Layout and Routing

- [ ] Port Layout component (nav, sidebar)
- [ ] Set up SvelteKit routing for all 5 pages: /, /memories, /code, /tasks, /chat
- [ ] Port ProjectSidebar component
- [ ] Basic navigation working between pages

## Phase 3: API Client and Data Fetching

- [ ] Create TypeScript API client matching current endpoints
- [ ] Type definitions (port from mira-types or codegen)
- [ ] Fetch and display: memories, goals, tasks, projects
- [ ] Error handling and loading states

## Phase 4: Chat Page with Streaming

- [ ] Port message bubbles (user/assistant/system)
- [ ] Typing indicator, thinking indicator
- [ ] SSE streaming for responses
- [ ] Markdown rendering with syntax highlighting (marked + highlight.js or similar)
- [ ] Expandable sections for tool calls
- [ ] Chat history loading

## Phase 5: Terminal Integration

- [ ] xterm.js for proper terminal emulation
- [ ] WebSocket PTY connection
- [ ] ANSI color parsing (or let xterm handle it)
- [ ] Terminal tray with multiple instances
- [ ] Spawn/kill terminal sessions

## Phase 6: Polish and Packaging

- [ ] App icons (Linux/Mac/Windows)
- [ ] Window chrome and native feel
- [ ] System tray integration (optional)
- [ ] Tauri build configuration for all platforms
- [ ] Remove old mira-app crate
- [ ] Update build scripts and README

## Phase 7: Optional - Tauri IPC Migration

- [ ] If HTTP overhead noticeable, convert key endpoints to `#[tauri::command]`
- [ ] Direct IPC instead of localhost HTTP
- [ ] Only pursue if performance demands it

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Frontend framework | Svelte | Simpler than React, great DX, less boilerplate |
| Styling | TailwindCSS | Fast iteration, utility-first |
| Terminal | xterm.js | Industry standard, handles ANSI natively |
| Backend approach | Keep Axum initially | Less disruption, HTTP works fine |
| Types | Manual TS initially | Consider codegen from mira-types later |

## Current Frontend Structure (for reference)

Pages to port:
- `/` - Home
- `/memories` - Memory browser
- `/code` - Code search
- `/tasks` - Tasks/goals view
- `/chat` - Chat with DeepSeek

Key components:
- `Layout` - Main app shell with nav
- `ProjectSidebar` - Project switcher
- `MessageBubble` - Chat messages (user/assistant/system)
- `TypingIndicator`, `ThinkingIndicator` - Loading states
- `Markdown`, `CodeBlock` - Content rendering
- `Expandable` - Collapsible sections for tool calls
- `Terminal` - PTY terminal with tray

## API Endpoints to Support

```
GET  /api/health
GET  /api/memories
POST /api/recall
POST /api/search/code
GET  /api/goals
GET  /api/tasks
GET  /api/projects
GET  /api/project
POST /api/project/set
POST /api/chat
GET  /api/chat/history
WS   /ws (terminal PTY)
SSE  /api/chat/stream (chat streaming)
```

## Notes

- Tauri uses system webview (not Chromium) = tiny binaries (~3-10MB vs Electron's 100MB+)
- During dev: Vite dev server (instant reload) + Tauri backend
- For distribution: Single binary desktop app
- Can still keep web server for browser access if wanted
