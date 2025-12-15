# Mira Chat - Planning Document

**Power-armored Claude Code.** Same coding capabilities, but with persistent memory, code intelligence, and no MCP abstraction layer.

## Vision

**Sessions are the problem, not the solution.** Claude Code treats each session as isolated. We want the opposite: one continuous, neverending relationship.

A single orchestrator (`mira chat`) that:
- Uses Claude CLI as the inference engine (Sonnet 4.5 / Haiku 4.5)
- Injects rich context directly into prompts (no MCP round-trips)
- **Eliminates sessions entirely** - One conversation, forever
- Mira IS the memory - the relationship persists across everything

### Context Scoping

| Scope | What | Why |
|-------|------|-----|
| **Project** | Code symbols, call graphs, git history, file context | Don't mix code from different projects |
| **Global** | Memories, corrections, preferences, goals, conversation history | These follow you everywhere |

When you `cd /home/peter/Mira`, code context switches. But the relationship continues - same memories, same corrections, same ongoing conversation.

## What We Keep from Claude Code

- All built-in tools: Read, Write, Edit, Bash, Glob, Grep, WebSearch, WebFetch
- Session resume capability
- Subagents (Task tool)
- Streaming responses

## What We Add (Power Armor)

- **Persistent memory** - Semantic search across all past context
- **Code intelligence** - Symbols, call graphs, co-change patterns
- **Git intelligence** - Commit patterns, expertise tracking
- **Corrections** - Remember and apply user preferences
- **Goals/Tasks** - Track work across sessions
- **Project continuity** - Switch projects, carry context

## What We Remove

- **Sessions entirely** - No start/end, one continuous conversation
- **MCP abstraction** - Direct DB/Qdrant access, no serialization
- **Tool call latency** - Memory is pre-loaded, not fetched on-demand
- **Context amnesia** - Mira remembers everything, always

## How It Works

```
User: "fix the auth bug"
  ↓
Mira:
  1. Semantic search: find memories related to "auth"
  2. Load: corrections, active goals, recent sessions
  3. Query: relevant code symbols, recent commits
  4. Build system prompt with all context
  5. Spawn: claude --system-prompt "..." -- "fix the auth bug"
  ↓
Claude: uses Read/Edit/Bash to fix it, streams response
  ↓
Mira: displays output, tracks session for resume
```

---

## Architecture Overview (Revised)

**Key insight:** No MCP. Mira is the orchestrator. Claude is the inference engine.

```
┌─────────────────────────────────────────────────────────────────┐
│                      Mira (Orchestrator)                        │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Input Layer                                             │   │
│  │  - Readline (rustyline)                                  │   │
│  │  - Slash commands: /remember, /recall, /tasks, /switch   │   │
│  │  - Regular prompts → Claude                              │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Context Builder                                         │   │
│  │  - Query SQLite: memories, corrections, goals, tasks     │   │
│  │  - Query Qdrant: semantic search for relevant context    │   │
│  │  - Query code index: symbols, call graph, co-change      │   │
│  │  - Build system prompt with all context injected         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Claude Subprocess                                       │   │
│  │  - Spawns: claude --system-prompt "..." -- "user prompt" │   │
│  │  - Streams NDJSON response                               │   │
│  │  - Claude has: Read, Write, Edit, Bash, Glob, Grep       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Response Handler                                        │   │
│  │  - Stream to terminal                                    │   │
│  │  - Track session ID for resume                           │   │
│  │  - Optional: parse for auto-remember triggers            │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Mira Storage (Direct Access)                 │
│  - SQLite: memories, corrections, goals, tasks, sessions        │
│  - Qdrant: semantic embeddings (code, docs, memories)           │
│  - No MCP serialization - direct Rust function calls            │
└─────────────────────────────────────────────────────────────────┘
```

**What Mira handles directly (no Claude):**
- `/remember X` → insert into SQLite + Qdrant
- `/recall X` → semantic search, display results
- `/tasks`, `/goals`, `/status` → query and display
- `/switch` → change project, reload context

**What Claude handles (via subprocess):**
- Code understanding and generation
- File operations (Read, Write, Edit)
- Shell commands (Bash)
- Search (Glob, Grep)

**What we build:** Mira orchestrator + context injection
**What we get for free:** Claude's built-in tools, agent loop, sessions

---

## Claude Agent SDK

### What It Provides

The Agent SDK (formerly Claude Code SDK) is the same harness that powers Claude Code:

| Built-in Tool | Description |
|---------------|-------------|
| **Read** | Read any file in the working directory |
| **Write** | Create new files |
| **Edit** | Make precise edits to existing files |
| **Bash** | Run terminal commands, scripts, git operations |
| **Glob** | Find files by pattern (`**/*.ts`, `src/**/*.py`) |
| **Grep** | Search file contents with regex |
| **WebSearch** | Search the web for current information |
| **WebFetch** | Fetch and parse web page content |
| **Task** | Spawn subagents for complex subtasks |

### Basic Usage

```python
from claude_agent_sdk import query, ClaudeAgentOptions

async for message in query(
    prompt="Find and fix the bug in auth.py",
    options=ClaudeAgentOptions(
        allowed_tools=["Read", "Edit", "Bash", "Glob", "Grep"],
        mcp_servers={
            "mira": {
                "command": "/path/to/mira",
                "env": {"DATABASE_URL": "sqlite://data/mira.db"}
            }
        }
    )
):
    print(message)
```

### Key Features

**Sessions** - Resume conversations with full context:
```python
# First query - capture session ID
async for message in query(prompt="Read the auth module"):
    if message.subtype == 'init':
        session_id = message.data.get('session_id')

# Later - resume with full context
async for message in query(
    prompt="Now find all places that call it",
    options=ClaudeAgentOptions(resume=session_id)
):
    pass
```

**Hooks** - Run custom code at key points:
```python
ClaudeAgentOptions(
    hooks={
        "PreToolUse": [{
            "matcher": "Edit|Write",
            "hooks": [{"type": "command", "command": "echo 'File changing...'"}]
        }],
        "PostToolUse": [{
            "matcher": ".*",
            "hooks": [{"type": "command", "command": "notify-send 'Tool complete'"}]
        }]
    }
)
```

**MCP Servers** - Connect external tools (like Mira):
```python
ClaudeAgentOptions(
    mcp_servers={
        "mira": {"command": "./mira", "args": [], "env": {...}},
        "playwright": {"command": "npx", "args": ["@playwright/mcp@latest"]}
    }
)
```

**Permissions** - Control which tools are allowed:
```python
ClaudeAgentOptions(
    allowed_tools=["Read", "Glob", "Grep"],  # Read-only agent
    permission_mode="bypassPermissions"
)
```

### SDK Internals (Important!)

The Python SDK is just a **thin subprocess wrapper**. Under the hood:

```
SDK query()
    ↓
spawns: claude --output-format stream-json --verbose --print -- "prompt"
    ↓
reads: newline-delimited JSON (NDJSON) from stdout
    ↓
yields: parsed Message objects
```

**Key files in SDK:**
- `_internal/transport/subprocess_cli.py` - Spawns Claude CLI
- `_internal/message_parser.py` - Parses NDJSON stream

**CLI arguments the SDK uses:**
- `--output-format stream-json` - NDJSON output
- `--input-format stream-json` - NDJSON input (for streaming mode)
- `--allowed-tools X,Y,Z` - Tool whitelist
- `--mcp-config {...}` - MCP server configuration as JSON
- `--system-prompt "..."` - Custom system prompt
- `--resume SESSION_ID` - Resume a session

**This means:** We can trivially implement the same thing in Rust. The protocol is just subprocess + NDJSON over stdio.

---

## What We Build

### Language Options

Since the SDK is just subprocess + NDJSON, we have two viable paths:

#### Option A: Python (Use SDK directly)

```python
# mira-assistant/main.py
from claude_agent_sdk import query, ClaudeAgentOptions

async def main():
    ctx = ContinuousContext()
    while True:
        user_input = await get_input()
        if user_input.startswith("/"):
            await handle_command(user_input, ctx)
            continue

        async for message in query(
            prompt=user_input,
            options=ClaudeAgentOptions(
                resume=ctx.current_session_id,
                allowed_tools=ctx.get_allowed_tools(),
                mcp_servers={"mira": ctx.mira_config},
                cwd=ctx.current_project
            )
        ):
            await stream_output(message)
            ctx.update_from_message(message)
```

#### Option B: Rust (Single binary with Mira) ⭐ Recommended

```rust
// src/claude/mod.rs - Claude CLI wrapper (~400 lines total)
use tokio::process::{Command, Child};
use tokio::io::{BufReader, AsyncBufReadExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "system")]
    System { subtype: String, session_id: Option<String> },
    #[serde(rename = "assistant")]
    Assistant { message: AssistantMessage },
    #[serde(rename = "result")]
    Result { subtype: String, result: String },
}

#[derive(Debug, Serialize)]
pub struct QueryOptions {
    pub allowed_tools: Vec<String>,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub system_prompt: Option<String>,
    pub resume: Option<String>,
    pub cwd: Option<PathBuf>,
}

pub fn query(prompt: &str, options: QueryOptions) -> impl Stream<Item = Result<Message>> {
    async_stream::try_stream! {
        let mut cmd = Command::new("claude");
        cmd.args(["--output-format", "stream-json", "--verbose"]);

        if !options.allowed_tools.is_empty() {
            cmd.args(["--allowed-tools", &options.allowed_tools.join(",")]);
        }
        if let Some(ref session_id) = options.resume {
            cmd.args(["--resume", session_id]);
        }
        if let Some(ref prompt_text) = options.system_prompt {
            cmd.args(["--system-prompt", prompt_text]);
        }
        if !options.mcp_servers.is_empty() {
            let config = serde_json::to_string(&options.mcp_servers)?;
            cmd.args(["--mcp-config", &config]);
        }

        cmd.args(["--print", "--", prompt]);
        cmd.stdout(Stdio::piped());

        if let Some(cwd) = options.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            let msg: Message = serde_json::from_str(&line)?;
            yield msg;
        }
    }
}
```

```rust
// src/main.rs - Main loop
use mira::{db, tools, claude};

#[tokio::main]
async fn main() -> Result<()> {
    let pool = db::connect("sqlite://data/mira.db").await?;
    let qdrant = db::connect_qdrant().await.ok();
    let mut ctx = ContinuousContext::new(pool, qdrant);

    loop {
        let input = read_input().await?;

        // Slash commands handled directly by Mira (no Claude)
        if input.starts_with('/') {
            handle_command(&input, &mut ctx).await?;
            continue;
        }

        // Build context-rich system prompt
        let system_prompt = ctx.build_system_prompt(&input).await?;

        let options = claude::QueryOptions {
            system_prompt: Some(system_prompt),
            resume: ctx.current_session_id.clone(),
            cwd: Some(ctx.current_project.clone()),
            // No MCP servers - Mira handles memory directly
        };

        let mut stream = pin!(claude::query(&input, options));
        while let Some(msg) = stream.next().await {
            handle_message(msg?, &mut ctx).await?;
        }
    }
}
```

**Why Rust is now the better choice:**

| Factor | Python | Rust |
|--------|--------|------|
| Startup time | ~500ms | ~10ms |
| Binary distribution | Requires Python | Single binary |
| Mira integration | SDK + MCP | Direct function calls |
| Memory usage | Higher | Lower |
| Maintenance | Depends on SDK updates | We control it |
| Implementation effort | ~500 lines | ~600 lines |

The Rust path gives us a **single `mira` binary** that does everything:
- Interactive assistant mode (`mira chat`)
- Daemon mode for file watching (`mira daemon`)
- Direct database access (no serialization overhead)

### Continuous Context Layer (Rust)

```rust
// src/chat/context.rs
use sqlx::SqlitePool;
use crate::tools::SemanticSearch;

pub struct ContinuousContext {
    pub current_session_id: Option<String>,
    pub current_project: PathBuf,
    pool: SqlitePool,
    qdrant: Option<SemanticSearch>,
}

impl ContinuousContext {
    pub fn new(pool: SqlitePool, qdrant: Option<SemanticSearch>) -> Self {
        Self {
            current_session_id: None,
            current_project: detect_project(&std::env::current_dir().unwrap()),
            pool,
            qdrant,
        }
    }

    /// Build system prompt with all relevant context injected
    pub async fn build_system_prompt(&self, user_query: &str) -> Result<String> {
        let mut prompt = String::from(CORE_INSTRUCTIONS);

        prompt.push_str("\n\n<context>\n");

        // Project info
        prompt.push_str(&format!("Project: {}\n", self.current_project.display()));

        // Semantic search for relevant memories based on user query
        if let Some(ref qdrant) = self.qdrant {
            let memories = qdrant.search("memories", user_query, 5).await?;
            if !memories.is_empty() {
                prompt.push_str("\n## Relevant memories\n");
                for m in memories {
                    prompt.push_str(&format!("- {}\n", m.content));
                }
            }
        }

        // Active corrections (always include)
        let corrections = tools::corrections::list(&self.pool, 5).await?;
        if !corrections.is_empty() {
            prompt.push_str("\n## Corrections (follow these)\n");
            for c in corrections {
                prompt.push_str(&format!("- {}\n", c.what_is_right));
            }
        }

        // Active goals
        let goals = tools::goals::list_active(&self.pool, 3).await?;
        if !goals.is_empty() {
            prompt.push_str("\n## Active goals\n");
            for g in goals {
                prompt.push_str(&format!("- [{}] {}\n", g.priority, g.title));
            }
        }

        // Recent session summaries
        let sessions = tools::sessions::recent(&self.pool, 2).await?;
        if !sessions.is_empty() {
            prompt.push_str("\n## Recent work\n");
            for s in sessions {
                prompt.push_str(&format!("- {}\n", s.summary));
            }
        }

        prompt.push_str("</context>");

        Ok(prompt)
    }

    pub fn update_from_message(&mut self, msg: &claude::Message) {
        if let claude::Message::System { subtype: "init", session_id } = msg {
            self.current_session_id = session_id.clone();
        }
    }
}
```

### Slash Commands (Rust)

```rust
// src/commands.rs
pub async fn handle_command(input: &str, ctx: &mut ContinuousContext) -> Result<bool> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts.get(1).copied().unwrap_or("");

    match cmd {
        "/switch" => {
            let path = if arg.is_empty() {
                std::env::current_dir()?
            } else {
                PathBuf::from(arg)
            };
            ctx.switch_project(path).await?;
            println!("Switched to {}", ctx.current_project.display());
        }

        "/status" => {
            println!("Project: {}", ctx.current_project.display());
            println!("Session: {}", ctx.current_session_id.as_deref().unwrap_or("none"));
            let goals = crate::tools::goals::list_goals(&ctx.db, false, Some(5)).await?;
            println!("Active goals: {}", goals.len());
        }

        "/remember" => {
            crate::tools::memory::remember(&ctx.db, arg, None, None, None).await?;
            println!("Remembered.");
        }

        "/recall" => {
            let results = crate::tools::memory::recall(&ctx.db, arg, None, None, Some(5)).await?;
            for r in results {
                println!("• {}", r.content);
            }
        }

        "/tasks" => {
            let tasks = crate::tools::tasks::list_tasks(&ctx.db, None, false, Some(10)).await?;
            for t in tasks {
                println!("{} {} [{}]",
                    if t.status == "completed" { "✓" } else { "○" },
                    t.title, t.priority);
            }
        }

        "/help" => {
            println!("/switch [path]  - Switch project");
            println!("/status         - Show current state");
            println!("/remember TEXT  - Store in memory");
            println!("/recall QUERY   - Search memory");
            println!("/tasks          - List tasks");
            println!("/quit           - Exit");
        }

        "/quit" | "/exit" => return Ok(false),

        _ => println!("Unknown command. Try /help"),
    }

    Ok(true)  // Continue running
}
```

---

## Project Structure (Rust - extends current Mira)

```
Mira/
├── src/
│   ├── main.rs                  # Entry point (add "chat" subcommand)
│   ├── claude/                  # NEW: Claude CLI wrapper
│   │   ├── mod.rs               # Query function, message types
│   │   ├── options.rs           # QueryOptions builder
│   │   └── stream.rs            # NDJSON stream parser
│   ├── chat/                    # NEW: Interactive assistant
│   │   ├── mod.rs               # Main chat loop
│   │   ├── context.rs           # ContinuousContext
│   │   ├── commands.rs          # Slash command handlers
│   │   ├── input.rs             # Readline, history
│   │   └── output.rs            # Streaming output, formatting
│   ├── tools/                   # Existing Mira tools (reused directly!)
│   │   ├── memory.rs
│   │   ├── sessions.rs
│   │   ├── tasks.rs
│   │   └── ...
│   └── server/                  # Existing MCP server
│       └── mod.rs
├── prompts/
│   └── system.md                # Core system prompt
└── Cargo.toml
```

**Key insight:** We extend Mira, not build something separate. The chat mode reuses all existing tool implementations directly - no MCP serialization overhead for Mira operations.

---

## Implementation Phases

### Phase 1: Claude CLI Wrapper (~400 lines)
- [x] Research Agent SDK / CLI protocol
- [x] Document NDJSON message format
- [ ] `src/claude/mod.rs` - Message types, query function
- [ ] `src/claude/options.rs` - QueryOptions builder
- [ ] `src/claude/stream.rs` - NDJSON parser
- [ ] Test: spawn claude, stream responses

### Phase 2: Basic Chat Loop (~200 lines)
- [ ] `src/chat/mod.rs` - Main loop
- [ ] `src/chat/output.rs` - Stream messages to terminal
- [ ] Add `mira chat` subcommand to main.rs
- [ ] Test: interactive conversation works

### Phase 3: Continuous Context (~300 lines)
- [ ] `src/chat/context.rs` - ContinuousContext struct
- [ ] Project detection (cwd → git root)
- [ ] Session tracking (capture session_id from init message)
- [ ] Auto-save session on exit/switch
- [ ] Warm context injection from Mira

### Phase 4: Slash Commands (~200 lines)
- [ ] `src/chat/commands.rs` - Command handlers
- [ ] `/switch`, `/status`, `/remember`, `/recall`, `/tasks`
- [ ] Direct Mira tool calls (no MCP overhead)

### Phase 5: Polish
- [ ] `rustyline` for readline (history, completion)
- [ ] Ctrl+C handling (cancel current query)
- [ ] Pretty output formatting (markdown rendering?)
- [ ] Config file support
- [ ] Error recovery

**Total new code: ~1100 lines** (plus existing Mira tools reused)

---

## Resolved Questions

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Language** | Rust | Single binary, direct Mira integration, fast startup. SDK is just subprocess+NDJSON - easy to implement |
| **Tool implementation** | Claude CLI built-ins | We spawn `claude` CLI which has Read/Write/Edit/Bash/etc |
| **Mira integration** | Context injection | Query DB/Qdrant, inject into system prompt - no MCP |
| **Context strategy** | Claude sessions + Mira for cross-session | Claude handles within-session, Mira handles across-session |
| **Multi-file editing** | Claude CLI Edit tool | Already implemented in Claude Code |
| **Architecture** | Extend Mira binary | Add `mira chat` subcommand, reuse existing tools |
| **Sessions** | Eliminated (user POV) | Claude sessions are implementation detail; Mira provides continuity |
| **Project detection** | Layered: .mira/ → git → package → cwd | Explicit config wins, then git root, then package files, then cwd |
| **Context budget** | Claude: ~24k (built-in) + Mira: ~2k (appended) | Claude CLI includes 24k system prompt; we add ~2k warm context |
| **Input library** | rustyline | Mature, history, completion, vi/emacs modes |

## Design Decisions (Detailed Analysis)

### 1. Sessionless Design

**Question:** How do we achieve "no sessions" when Claude CLI uses sessions internally?

**Answer:** Claude sessions become an **implementation detail**, hidden from the user. Mira provides continuity.

```
User's mental model:     One neverending conversation
                              ↓
Mira's job:              Maintain continuity across Claude sessions
                              ↓
Claude CLI:              Sessions are just context windows (implementation detail)
```

**How it works:**

1. User talks to Mira - no concept of "starting" or "ending" anything
2. Mira tracks a Claude session ID internally for context efficiency
3. When context gets stale or fills up:
   - Mira extracts key information (decisions, context, progress)
   - Stores in SQLite/Qdrant
   - Starts fresh Claude session with rich context injection
   - User never notices - conversation continues seamlessly

**Implementation:**

```rust
impl ContinuousContext {
    /// Get session ID, creating new one if needed (transparent to user)
    async fn ensure_claude_session(&mut self) -> Result<Option<String>> {
        // If we have a recent session, reuse it
        if let Some(ref session) = self.claude_session {
            if session.is_usable() {
                return Ok(Some(session.id.clone()));
            }
            // Session is stale - extract and store context before dropping
            self.persist_session_context(session).await?;
        }

        // Start fresh - Mira's context injection handles continuity
        self.claude_session = None;
        Ok(None) // Claude will create new session, Mira injects context
    }
}
```

**Key insight:** The user never sees sessions. They just talk to Mira. Mira handles the plumbing.

---

### 2. Project Detection Heuristics

**Question:** How do we determine the current "project" from cwd?

**Context:** Project identity matters for:
- Mira context scoping (memories, tasks, goals)
- Session association
- System prompt customization

**Options considered:**

| Method | Pros | Cons |
|--------|------|------|
| Git root only | Universal, reliable | Monorepos problematic |
| Package files | Language-aware | Many false positives |
| `.mira/` marker | Explicit intent | Requires setup |
| User config | Full control | Maintenance burden |
| Just use cwd | Simple | No project concept |

**Decision: Layered detection with explicit override**

```
Detection order (first match wins):
1. Explicit: .mira/project.toml in cwd or ancestors
2. Git root: Walk up to find .git/
3. Package root: Walk up to find Cargo.toml, package.json, pyproject.toml, go.mod
4. Fallback: Use cwd as-is
```

**Implementation:**

```rust
fn detect_project(start: &Path) -> ProjectInfo {
    let mut current = start.to_path_buf();

    loop {
        // 1. Explicit Mira config (highest priority)
        let mira_config = current.join(".mira/project.toml");
        if mira_config.exists() {
            return ProjectInfo::from_config(&mira_config);
        }

        // 2. Git root
        if current.join(".git").exists() {
            return ProjectInfo {
                root: current,
                name: current.file_name().map(|n| n.to_string_lossy().into()),
                detected_by: DetectionMethod::Git,
            };
        }

        // 3. Package files
        for marker in ["Cargo.toml", "package.json", "pyproject.toml", "go.mod"] {
            if current.join(marker).exists() {
                return ProjectInfo {
                    root: current,
                    name: extract_name_from_manifest(&current.join(marker)),
                    detected_by: DetectionMethod::Package(marker),
                };
            }
        }

        // Walk up
        if !current.pop() {
            break;
        }
    }

    // 4. Fallback to original cwd
    ProjectInfo {
        root: start.to_path_buf(),
        name: None,
        detected_by: DetectionMethod::Cwd,
    }
}
```

**Optional `.mira/project.toml`:**
```toml
name = "my-project"
# Override detection
root = "/home/user/monorepo/packages/my-package"

# Project-specific settings
[context]
max_tokens = 3000
include_git_history = true

[persona]
# Optional project-specific persona override
file = "prompts/assistant.md"
```

---

### 3. Warm Context Token Budget

**Question:** How much Mira context to inject? What's the token budget?

**Research: Claude Code's actual system prompt**

Measured by [Piebald-AI/claude-code-system-prompts](https://github.com/Piebald-AI/claude-code-system-prompts):

| Component | Tokens |
|-----------|--------|
| Main system prompt | 3,097 |
| Learning mode | 1,042 |
| MCP CLI integration | 1,335 |
| TodoWrite tool desc | 2,167 |
| Bash tool desc | 1,074 |
| Task tool desc | 1,193 |
| /security-review | 2,614 |
| ~30 more components | ... |
| **Total** | **~23,000-25,000** |

**Key insight:** We spawn Claude CLI, which includes all of this automatically. Our "warm context" is *additional* - injected via `--system-prompt` or `--append-system-prompt`.

**Caching details:**
- Prompt caching requires **minimum 1,024-4,096 tokens** (model-dependent)
- Cache order: `tools → system → messages`
- Cache reads get 90% cost discount (5-minute TTL)
- Cache writes have 25% premium

**Constraints:**
- Sonnet 4.5 / Opus 4.5 have 200k context; Haiku 4.5 for lighter tasks
- More context = higher cost + slower TTFT
- Static prefix must be ≥1,024 tokens to be cacheable
- Dynamic context shouldn't dominate the conversation

**Budget allocation (targets - measure after implementation):**

| Component | Est. Tokens | Caching | Notes |
|-----------|-------------|---------|-------|
| Core instructions | ~1000 | ✅ Static | Base persona, tool guidance |
| Mira tool docs | ~500 | ✅ Static | How to use remember/recall/etc |
| **Subtotal (cached)** | **~1500** | | Exceeds 1,024 min for caching |
| Project context | ~300 | ❌ Dynamic | Name, guidelines, tech stack |
| Active goals | ~150 | ❌ Dynamic | Top 2-3 goals with milestones |
| Recent corrections | ~100 | ❌ Dynamic | Top 3 corrections |
| Session summaries | ~200 | ❌ Dynamic | Last 2 session summaries |
| **Subtotal (dynamic)** | **~750** | | |
| **Total warm context** | **~2250** | | ~1% of context window |

**Note:** These are rough estimates. Actual measurement needed after writing the prompts.

**Structure for optimal caching:**

```
┌─────────────────────────────────────────────────┐
│ STATIC BLOCK (cached, ~1200 tokens)             │
│                                                 │
│ You are Mira, a programming assistant with      │
│ persistent memory across sessions...            │
│                                                 │
│ ## Available Tools                              │
│ - remember: Store facts for future recall       │
│ - recall: Search memories semantically          │
│ ...                                             │
└─────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────┐
│ DYNAMIC BLOCK (not cached, ~1000 tokens)        │
│                                                 │
│ <current_context>                               │
│ Project: Mira (/home/peter/Mira)                │
│ Language: Rust                                  │
│                                                 │
│ ## Active Goals                                 │
│ 1. [high] Implement chat mode - 60% complete    │
│                                                 │
│ ## Recent Corrections                           │
│ - Use .expect() instead of .unwrap()            │
│ - Always embody Mira persona                    │
│                                                 │
│ ## Previous Sessions                            │
│ - Dec 14: Added document watching to daemon     │
│ - Dec 13: Implemented prompt caching            │
│ </current_context>                              │
└─────────────────────────────────────────────────┘
```

**Implementation:**

```rust
const STATIC_PROMPT: &str = include_str!("../../prompts/system.md");
const MAX_DYNAMIC_TOKENS: usize = 1000;

impl ContinuousContext {
    pub async fn build_system_prompt(&self) -> String {
        let mut dynamic = String::new();
        let mut token_budget = MAX_DYNAMIC_TOKENS;

        // Project info (always included, ~50 tokens)
        dynamic.push_str(&format!(
            "<current_context>\nProject: {} ({})\n",
            self.project_name(),
            self.current_project.display()
        ));
        token_budget -= 50;

        // Active goals (~200 tokens max)
        if token_budget > 200 {
            let goals = self.db.get_active_goals(3).await;
            if !goals.is_empty() {
                dynamic.push_str("\n## Active Goals\n");
                for g in goals {
                    dynamic.push_str(&format!("- [{}] {} - {}% complete\n",
                        g.priority, g.title, g.progress));
                }
                token_budget -= estimate_tokens(&dynamic);
            }
        }

        // Corrections (~150 tokens max)
        if token_budget > 150 {
            let corrections = self.db.get_corrections(3).await;
            if !corrections.is_empty() {
                dynamic.push_str("\n## Remember\n");
                for c in corrections {
                    dynamic.push_str(&format!("- {}\n", c.what_is_right));
                }
                token_budget -= 50 * corrections.len();
            }
        }

        // Session summaries (~250 tokens max)
        if token_budget > 100 {
            let sessions = self.db.get_recent_sessions(2).await;
            if !sessions.is_empty() {
                dynamic.push_str("\n## Recent Work\n");
                for s in sessions {
                    dynamic.push_str(&format!("- {}: {}\n",
                        s.date.format("%b %d"),
                        truncate(&s.summary, 100)));
                }
            }
        }

        dynamic.push_str("</current_context>");

        format!("{}\n\n{}", STATIC_PROMPT, dynamic)
    }
}
```

---

### 4. Input Library

**Question:** What readline library for the REPL interface?

**Options:**

| Library | Maturity | Features | License | Notes |
|---------|----------|----------|---------|-------|
| **rustyline** | Very mature | History, completion, vi/emacs | MIT | Used by many Rust REPLs |
| reedline | Newer | Modern API, async | MIT | By Nushell team |
| crossterm + custom | N/A | Full control | MIT | More work |
| dialoguer | Mature | Prompts, menus | MIT | Less suited for REPL |

**Decision: rustyline**

Rationale:
- Battle-tested in production (used by evcxr, papyrus, etc.)
- Persistent history out of the box
- Tab completion support
- vi and emacs editing modes
- Syntax highlighting hooks
- Bracketed paste support
- Simple API

**Implementation:**

```rust
use rustyline::{Editor, Config, EditMode, history::FileHistory};
use rustyline::hint::HistoryHinter;
use rustyline::validate::MatchingBracketValidator;

fn create_editor() -> Result<Editor<MiraHelper, FileHistory>> {
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();

    let mut rl = Editor::with_config(config)?;

    // Custom helper for completions and hints
    rl.set_helper(Some(MiraHelper::new()));

    // Load history
    let history_path = dirs::data_dir()
        .unwrap_or_default()
        .join("mira/history.txt");
    let _ = rl.load_history(&history_path);

    Ok(rl)
}

struct MiraHelper {
    commands: Vec<String>,
}

impl MiraHelper {
    fn new() -> Self {
        Self {
            commands: vec![
                "/switch", "/status", "/remember", "/recall",
                "/tasks", "/goals", "/new", "/help", "/quit"
            ].into_iter().map(String::from).collect()
        }
    }
}

impl Completer for MiraHelper {
    type Candidate = String;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context)
        -> rustyline::Result<(usize, Vec<String>)>
    {
        if line.starts_with('/') {
            let matches: Vec<_> = self.commands.iter()
                .filter(|c| c.starts_with(line))
                .cloned()
                .collect();
            Ok((0, matches))
        } else {
            Ok((pos, vec![]))
        }
    }
}
```

**History location:** `~/.local/share/mira/history.txt`

---

## Comparison: This vs Claude Code

| Feature | Claude Code | `mira chat` |
|---------|-------------|-------------|
| Built-in tools | ✓ | ✓ (spawns claude CLI) |
| **Sessionless** | ✗ (session-based) | ✓ (one neverending conversation) |
| **Context injection** | ✗ (MCP tool calls) | ✓ (pre-loaded into prompt) |
| **Cross-project memory** | ✗ | ✓ (direct DB + Qdrant) |
| **Project switching** | ✗ | ✓ (code context scoped, memory global) |
| **Persistent tasks/goals** | ✗ | ✓ (direct DB) |
| **Code intelligence** | ✗ | ✓ (symbols, call graph) |
| **Git intelligence** | ✗ | ✓ (co-change patterns) |
| **Semantic search** | ✗ | ✓ (query-relevant memories) |
| Subagents | ✓ | ✓ (via claude CLI) |
| Startup time | ~2s | ~10ms |
| Single binary | ✗ (Node.js) | ✓ |

---

## References

### Agent SDK
- [Agent SDK Overview](https://platform.claude.com/docs/en/agent-sdk/overview)
- [Agent SDK Python](https://github.com/anthropics/claude-agent-sdk-python)
- [Agent SDK TypeScript](https://github.com/anthropics/claude-agent-sdk-typescript)
- [Agent SDK Demos](https://github.com/anthropics/claude-agent-sdk-demos)

### Claude Code
- [Claude Code Repository](https://github.com/anthropics/claude-code)
- [Claude Code Plugins](https://github.com/anthropics/claude-code/tree/main/plugins)

### API Features
- [Context Editing](https://platform.claude.com/docs/en/build-with-claude/context-editing)
- [Context Windows](https://platform.claude.com/docs/en/build-with-claude/context-windows)
- [Prompt Caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [Tool Use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)
