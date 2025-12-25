# Mira Studio

**Web-based Chat Interface for Mira**

Mira Studio is a SvelteKit frontend that provides a modern chat interface to the Mira daemon. It communicates with GPT-5.2 via Mira's HTTP API and renders structured streaming responses with rich tool call visualization.

## Quick Start

```bash
cd studio
npm install
npm run dev
```

Open http://localhost:5173. Ensure the Mira daemon is running (`systemctl --user status mira`).

## Architecture

```
Studio (SvelteKit)           Mira Daemon (Rust)
┌─────────────────┐         ┌─────────────────┐
│                 │  SSE    │                 │
│   ChatPanel     │◄───────►│  /api/chat      │
│   TerminalView  │         │                 │
│   BlockRenderer │         │  GPT-5.2        │
│                 │         │  Tool execution │
└─────────────────┘         └─────────────────┘
```

### Key Design Decisions

1. **No Frontend Parsing** - The backend sends structured `MessageBlock` types. No markdown parsing in the frontend. This ensures consistency and better streaming performance.

2. **SSE Streaming** - Real-time streaming via Server-Sent Events with typed events (`text_delta`, `tool_call_start`, `tool_call_result`, `code_block`, etc.)

3. **Svelte 5 Runes** - Uses `$state`, `$derived`, and `$effect` for reactive state management.

4. **Terminal Aesthetic** - Monospace fonts, dark theme, terminal-inspired UI.

## Directory Structure

```
studio/src/lib/
├── api/
│   └── client.ts           # API client, SSE streaming, type definitions
├── components/
│   ├── layout/
│   │   ├── AppShell.svelte         # Main layout container
│   │   ├── NavRail.svelte          # Left 48px icon rail
│   │   └── ContextDrawer.svelte    # Right tabbed panel (360px)
│   ├── chat/
│   │   ├── BlockRenderer.svelte    # Switch on block.type
│   │   ├── TextRenderer.svelte     # Markdown-ish text rendering
│   │   └── ToolCallInline.svelte   # Inline tool call display
│   ├── terminal/
│   │   ├── TerminalView.svelte     # Message list + streaming
│   │   └── TerminalPrompt.svelte   # Chat input
│   ├── timeline/
│   │   ├── TimelineTab.svelte      # Tool activity feed
│   │   └── TimelineCard.svelte     # Expandable tool call card
│   ├── workspace/
│   │   ├── WorkspaceTab.svelte     # Artifacts panel
│   │   └── ArtifactCard.svelte     # File preview card
│   └── content/
│       ├── CodeBlock.svelte        # Syntax highlighted code
│       ├── CouncilView.svelte      # Multi-model responses
│       └── DiffView.svelte         # File diff display
├── stores/
│   ├── layout.svelte.ts       # Panel state, localStorage persistence
│   ├── settings.ts            # Project path, model, theme
│   ├── streamState.svelte.ts  # Streaming state machine
│   ├── toolActivity.svelte.ts # Tool call tracking
│   └── artifacts.svelte.ts    # File artifacts tracking
└── types/
    └── content.ts             # CouncilResponses, ProviderInfo
```

## Layout

### Desktop (≥1024px)

```
┌──────┬────────────────────────┬─────────────┐
│ Rail │         Chat           │  Context    │
│ 48px │       (flex-1)         │  Drawer     │
│      │                        │  (360px)    │
│ [⚙]  │  > user message        │             │
│ [?]  │  │ assistant response  │ [Timeline]  │
│      │  │   ✓ read_file       │ [Workspace] │
│      │  │   ✓ bash            │             │
└──────┴────────────────────────┴─────────────┘
```

### Mobile (<768px)

- No left rail
- Hamburger menu header
- Context drawer as bottom sheet (70vh)
- Panel toggle button in header

## Components

### BlockRenderer

Renders message blocks by type. No parsing needed - backend sends structured blocks.

```svelte
{#if block.type === 'text'}
  <TextRenderer text={block.content} />
{:else if block.type === 'code_block'}
  <CodeBlock language={block.language} code={block.code} />
{:else if block.type === 'tool_call'}
  <ToolCallInline name={block.name} arguments={block.arguments} result={block.result} />
{:else if block.type === 'council'}
  <CouncilView responses={toCouncilResponses(block)} />
{/if}
```

### ToolCallInline

Compact inline tool call display. Shows:
- Status indicator (spinner, checkmark, X)
- Tool name and summary
- Duration
- Expand to see arguments, output, diff

Features:
- Alt+click opens Timeline panel
- Category color coding (file=cyan, shell=yellow, memory=purple, web=blue, git=orange, mira=accent)

### TimelineTab

Live feed of all tool executions. Features:
- Filter by category (file, shell, memory, web, git, mira)
- Filter by status (running, done, error)
- Active tool count badge
- "Jump to chat" button for bidirectional linking

### WorkspaceTab

Tracks files read, written, and modified during the session. Features:
- Filter pills (All, Modified, Created, Read)
- Artifact cards with preview
- Click to open full viewer
- Links to source tool call

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd/Ctrl + /` | Focus chat input |
| `Cmd/Ctrl + \` | Toggle context drawer |
| `Cmd/Ctrl + 1` | Switch to Timeline tab |
| `Cmd/Ctrl + 2` | Switch to Workspace tab |
| `Cmd/Ctrl + ,` | Toggle settings |
| `Escape` | Cancel streaming or close drawer |

## Stores

### layoutStore

Manages panel visibility and dimensions with localStorage persistence.

```typescript
layoutStore.toggleDrawer()          // Toggle right panel
layoutStore.setDrawerTab('timeline') // Switch tab
layoutStore.setDrawerWidth(400)     // Resize (280-600px)
layoutStore.toggleSettings()        // Toggle settings modal
```

### toolActivityStore

Tracks all tool calls during streaming.

```typescript
toolActivityStore.toolStarted(event)   // SSE tool_call_start
toolActivityStore.toolCompleted(event) // SSE tool_call_result
toolActivityStore.filteredCalls        // Get filtered list
toolActivityStore.activeCount          // Running tool count
toolActivityStore.scrollToChat(callId) // Jump to chat
```

### artifactStore

Tracks files and artifacts created during the session.

```typescript
artifactStore.processToolResult(event) // Process tool results
artifactStore.artifacts                 // All artifacts
artifactStore.grouped                   // Grouped by action
artifactStore.counts                    // Count by action
```

### streamState

State machine for streaming. States: `idle` → `streaming` → `idle`.

```typescript
streamState.startStream(messageId)  // Returns AbortController
streamState.updateStream(blocks, usage)
streamState.completeStream()
streamState.cancelStream()
streamState.isLoading              // Is streaming?
streamState.canCancel              // Can cancel?
streamState.streamingMessage       // Current message
```

## Event Types

SSE events from `/api/chat`:

```typescript
type ChatEvent =
  | { type: 'text_delta'; content: string; message_id: string; seq: number }
  | { type: 'code_block'; language: string; code: string; filename?: string }
  | { type: 'tool_call_start'; call_id: string; name: string; arguments: object;
      message_id: string; seq: number; ts_ms: number;
      summary: string; category: ToolCategory }
  | { type: 'tool_call_result'; call_id: string; name: string; success: boolean;
      output: string; duration_ms: number; truncated: boolean; total_bytes: number;
      diff?: DiffInfo; exit_code?: number; stderr?: string }
  | { type: 'usage'; input_tokens: number; output_tokens: number;
      cached_tokens: number; reasoning_tokens: number }
  | { type: 'done' }
  | { type: 'error'; message: string }
```

Tool categories: `file`, `shell`, `memory`, `web`, `git`, `mira`, `other`

## Styling

Uses CSS custom properties for theming:

```css
--term-bg           /* Background */
--term-bg-secondary /* Card/panel background */
--term-text         /* Primary text */
--term-text-dim     /* Secondary text */
--term-accent       /* Accent color (orange) */
--term-accent-faded /* Accent with transparency */
--term-border       /* Border color */
--term-success      /* Green */
--term-error        /* Red */
--term-warning      /* Yellow */
--font-mono         /* Monospace font */
```

## Building

```bash
npm run build
```

Output goes to `build/`. The build is static and can be served from any web server.

## Configuration

Settings stored in localStorage:
- `mira-settings`: Project path, model, theme, reasoning effort
- `mira-layout`: Drawer state, width, active tab
- `mira-expansion-state`: Which code blocks are expanded

## API Endpoint

Studio expects Mira daemon at `http://localhost:3199`:

- `GET /health` - Health check
- `GET /api/status` - Status with model info
- `GET /api/messages` - Message history
- `POST /api/chat` - SSE streaming chat endpoint

## Development

```bash
npm run dev      # Start dev server
npm run build    # Production build
npm run preview  # Preview production build
```

## Tech Stack

- **Framework**: SvelteKit 2.0
- **Language**: TypeScript
- **Styling**: Tailwind CSS + custom CSS
- **Build**: Vite
- **State**: Svelte 5 runes ($state, $derived, $effect)
