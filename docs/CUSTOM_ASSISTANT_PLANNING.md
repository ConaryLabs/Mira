# Mira Chat - Planning Document

**Power-armored coding assistant.** Persistent memory, code intelligence, cost-efficient inference.

## Vision

**Sessions are the problem, not the solution.** Traditional assistants treat each session as isolated. We want the opposite: one continuous, neverending relationship.

A single orchestrator (`mira chat`) that:
- Uses **DeepSeek V3.2** as the inference engine (cost-efficient, auto-caching)
- Injects rich context directly into prompts
- **Eliminates sessions entirely** - One conversation, forever
- Mira IS the memory - the relationship persists across everything

## Why DeepSeek Over Claude CLI?

| Factor | Claude CLI | DeepSeek API |
|--------|-----------|--------------|
| Caching | Manual (`cache_control` headers) | **Auto** (just works) |
| Min cache tokens | 1,024 | **64** |
| Cache TTL | 5 minutes | **Disk-persistent** |
| Cache hit discount | 90% | **90%** |
| Cache write cost | +25% | **Free** |
| Base pricing | $3/M input, $15/M output (Sonnet 4.5) | **$0.28/M input, $0.42/M output** |
| Tools | Built-in (Read, Write, Edit, Bash) | We implement |

**Bottom line:** DeepSeek is ~10x cheaper with simpler caching. We trade built-in tools for massive cost savings - and we were going to wrap everything anyway.

### Tool Implementation

Without Claude CLI's built-in tools, we implement them via DeepSeek function calling:

```rust
let tools = vec![
    Tool::new("read_file", "Read file contents", json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path to read"}
        },
        "required": ["path"]
    })),
    Tool::new("write_file", "Write content to file", json!({...})),
    Tool::new("edit_file", "Edit file with search/replace", json!({...})),
    Tool::new("bash", "Run shell command", json!({...})),
    Tool::new("glob", "Find files by pattern", json!({...})),
    Tool::new("grep", "Search file contents", json!({...})),
];

// Mira executes tool calls, returns results to DeepSeek
async fn execute_tool(call: &ToolCall) -> String {
    match call.name.as_str() {
        "read_file" => fs::read_to_string(&call.args["path"]).unwrap_or_else(|e| e.to_string()),
        "bash" => run_command(&call.args["command"]).await,
        // ...
    }
}
```

This is actually **better** because:
- We control tool execution completely
- Can add safety checks, logging, permissions
- Tools integrate directly with Mira (no MCP overhead)
- Can extend with Mira-specific tools (remember, recall, etc.)

### Context Scoping

| Scope | What | Why |
|-------|------|-----|
| **Project** | Code symbols, call graphs, git history, file context | Don't mix code from different projects |
| **Global** | Memories, corrections, preferences, goals, conversation history | These follow you everywhere |

When you `cd /home/peter/Mira`, code context switches. But the relationship continues - same memories, same corrections, same ongoing conversation.

## What We Implement (Core Tools)

Built with DeepSeek function calling:
- **File tools**: read_file, write_file, edit_file
- **Shell tools**: bash, glob, grep
- **Memory tools**: remember, recall (integrated with Mira DB)
- **Streaming**: SSE response streaming to terminal

## What We Add (Power Armor)

- **Persistent memory** - Semantic search across all past context
- **Code intelligence** - Symbols, call graphs, co-change patterns
- **Git intelligence** - Commit patterns, expertise tracking
- **Corrections** - Remember and apply user preferences
- **Goals/Tasks** - Track work across sessions
- **Project continuity** - Switch projects, carry context
- **Rolling summaries** - Never lose context, just summarize older parts

## What We Avoid

- **Sessions entirely** - No start/end, one continuous conversation
- **MCP abstraction** - Direct DB/Qdrant access, no serialization
- **Claude CLI dependency** - Direct API calls, we own the tool loop
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
  5. Call DeepSeek API with tools + context
  ↓
DeepSeek: Returns tool calls (read_file, edit_file, etc.)
  ↓
Mira: Executes tools, returns results, loops until done
  ↓
DeepSeek: "I've fixed the auth bug by..."
  ↓
Mira: Stream to terminal, store in conversation history
```

---

## Architecture Overview

**Key insight:** Mira is the orchestrator. DeepSeek is the inference engine. We own the tool loop.

```
┌─────────────────────────────────────────────────────────────────┐
│                      Mira (Orchestrator)                        │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Input Layer                                             │   │
│  │  - Readline (rustyline)                                  │   │
│  │  - Slash commands: /remember, /recall, /tasks, /switch   │   │
│  │  - Regular prompts → DeepSeek                            │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Context Builder                                         │   │
│  │  - Query SQLite: memories, corrections, goals, tasks     │   │
│  │  - Query Qdrant: semantic search for relevant context    │   │
│  │  - Query code index: symbols, call graph, co-change      │   │
│  │  - Build system prompt with all context injected         │   │
│  │  - DeepSeek auto-caches (no manual cache_control!)       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  DeepSeek API Client                                     │   │
│  │  - POST to api.deepseek.com/chat/completions             │   │
│  │  - Stream SSE responses                                  │   │
│  │  - Function calling for tools                            │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Tool Executor (Mira-owned)                              │   │
│  │  - read_file, write_file, edit_file                      │   │
│  │  - bash, glob, grep                                      │   │
│  │  - remember, recall (direct DB access)                   │   │
│  │  - Execute → return result → continue conversation       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Response Handler                                        │   │
│  │  - Stream to terminal                                    │   │
│  │  - Loop until no more tool calls                         │   │
│  │  - Rolling summaries for long conversations              │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Mira Storage (Direct Access)                 │
│  - SQLite: memories, corrections, goals, tasks, sessions        │
│  - Qdrant: semantic embeddings (code, docs, memories)           │
│  - No MCP, no subprocess - direct Rust function calls           │
└─────────────────────────────────────────────────────────────────┘
```

**What Mira handles directly (slash commands):**
- `/remember X` → insert into SQLite + Qdrant
- `/recall X` → semantic search, display results
- `/tasks`, `/goals`, `/status` → query and display
- `/switch` → change project, reload context

**What DeepSeek handles (via API):**
- Code understanding and generation
- Deciding which tools to call
- Reasoning about context

**What Mira executes (tool calls from DeepSeek):**
- File operations (read, write, edit)
- Shell commands (bash)
- Search (glob, grep)
- Memory (remember, recall - integrated as tools too)

**What we build:** Everything (orchestrator, tools, context, API client)
**What we get for free:** DeepSeek's auto-caching, cheap inference

---

## DeepSeek API Integration

### API Overview

DeepSeek provides an OpenAI-compatible API with automatic context caching:

```
POST https://api.deepseek.com/chat/completions
Authorization: Bearer $DEEPSEEK_API_KEY
Content-Type: application/json

{
  "model": "deepseek-chat",
  "messages": [...],
  "tools": [...],
  "stream": true
}
```

### Key Features

| Feature | Details |
|---------|---------|
| **Auto-caching** | Enabled by default, no configuration needed |
| **Cache granularity** | 64 tokens minimum |
| **Cache persistence** | Disk-based, survives across requests |
| **Function calling** | OpenAI-compatible tool format |
| **Streaming** | SSE streaming responses |
| **Pricing** | $0.028/M cache hit, $0.28/M cache miss, $0.42/M output |

### Basic Usage (Rust)

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    tools: Vec<Tool>,
    stream: bool,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

async fn chat(client: &Client, messages: Vec<Message>, tools: Vec<Tool>) -> Result<Response> {
    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages,
        tools,
        stream: true,
    };

    let response = client
        .post("https://api.deepseek.com/chat/completions")
        .bearer_auth(std::env::var("DEEPSEEK_API_KEY")?)
        .json(&request)
        .send()
        .await?;

    // Handle SSE stream...
    Ok(response)
}
```

### Tool Calling Flow

```
User: "Fix the auth bug in src/auth.rs"
            ↓
Mira: Build context, send to DeepSeek with tools
            ↓
DeepSeek: Returns tool_call: read_file("src/auth.rs")
            ↓
Mira: Executes read_file, returns content
            ↓
DeepSeek: Analyzes, returns tool_call: edit_file(...)
            ↓
Mira: Executes edit, returns success
            ↓
DeepSeek: "I've fixed the auth bug by..."
            ↓
Mira: Stream to terminal, done
```

### Cache Behavior

DeepSeek's disk cache works on **prefix matching**:

```
Request 1: [system prompt] + [context] + "fix auth bug"
           └─────────────────────────┘
                    cached

Request 2: [system prompt] + [context] + "now add tests"
           └─────────────────────────┘
                 cache HIT (90% off)
```

Since our system prompt and context are prepended, they naturally form a stable prefix → high cache hit rate.

---

## What We Build

### Main Chat Loop

```rust
// src/chat/mod.rs - Main chat loop
use crate::tools::SemanticSearch;
use crate::chat::{deepseek, context::ContinuousContext, tools::execute_tool};

pub async fn run(pool: SqlitePool, semantic: Option<SemanticSearch>) -> Result<()> {
    let client = reqwest::Client::new();
    let mut ctx = ContinuousContext::new(pool.clone(), semantic);
    let mut editor = create_editor()?;

    println!("Mira Chat - Type /help for commands, /quit to exit");

    loop {
        let input = match editor.readline(">>> ") {
            Ok(line) => line,
            Err(_) => break,
        };
        editor.add_history_entry(&input)?;

        // Slash commands handled directly by Mira
        if input.starts_with('/') {
            if !handle_command(&input, &mut ctx).await? {
                break; // /quit
            }
            continue;
        }

        // Add user message to conversation
        ctx.add_message(Message::user(&input));

        // Build messages with context-rich system prompt
        let system_prompt = ctx.build_system_prompt(&input).await?;
        let messages = ctx.build_messages(&system_prompt);
        let tools = define_tools();

        // Tool loop: keep calling DeepSeek until no more tool calls
        loop {
            let response = deepseek::chat(&client, &messages, &tools).await?;

            // Stream text content to terminal
            if let Some(text) = &response.content {
                print!("{}", text);
            }

            // Check for tool calls
            if response.tool_calls.is_empty() {
                ctx.add_message(Message::assistant(&response));
                break;
            }

            // Execute tools and add results
            for call in &response.tool_calls {
                let result = execute_tool(call, &pool, &ctx).await;
                ctx.add_message(Message::tool_result(&call.id, &result));
            }
        }

        println!(); // Newline after response
        ctx.maybe_summarize().await?;
    }

    // Store session summary on exit
    ctx.store_session_summary().await?;
    Ok(())
}
```

### DeepSeek API Client

```rust
// src/chat/deepseek.rs - DeepSeek API client
use reqwest::Client;
use serde::{Deserialize, Serialize};
use futures::StreamExt;

const API_URL: &str = "https://api.deepseek.com/chat/completions";

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    pub stream: bool,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct ResponseMessage {
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

pub async fn chat(client: &Client, messages: &[Message], tools: &[Tool]) -> Result<ResponseMessage> {
    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages: messages.to_vec(),
        tools: tools.to_vec(),
        stream: true,
    };

    let api_key = std::env::var("DEEPSEEK_API_KEY")?;

    let response = client
        .post(API_URL)
        .bearer_auth(&api_key)
        .json(&request)
        .send()
        .await?;

    // Handle SSE streaming
    let mut stream = response.bytes_stream();
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }
                if let Ok(delta) = serde_json::from_str::<StreamDelta>(data) {
                    if let Some(c) = delta.choices.first() {
                        if let Some(ref text) = c.delta.content {
                            print!("{}", text); // Stream to terminal
                            content.push_str(text);
                        }
                        if let Some(ref calls) = c.delta.tool_calls {
                            // Accumulate tool call chunks
                            merge_tool_calls(&mut tool_calls, calls);
                        }
                    }
                }
            }
        }
    }

    Ok(ResponseMessage {
        content: if content.is_empty() { None } else { Some(content) },
        tool_calls,
    })
}

### Tool Executor

```rust
// src/chat/tools.rs - Tool execution
use serde_json::Value;
use std::process::Command;

pub async fn execute_tool(call: &ToolCall, pool: &SqlitePool, ctx: &ContinuousContext) -> String {
    let args = &call.function.arguments;

    match call.function.name.as_str() {
        "read_file" => {
            let path = args["path"].as_str().unwrap();
            match std::fs::read_to_string(path) {
                Ok(content) => content,
                Err(e) => format!("Error reading file: {}", e),
            }
        }

        "write_file" => {
            let path = args["path"].as_str().unwrap();
            let content = args["content"].as_str().unwrap();
            match std::fs::write(path, content) {
                Ok(_) => format!("Successfully wrote to {}", path),
                Err(e) => format!("Error writing file: {}", e),
            }
        }

        "edit_file" => {
            let path = args["path"].as_str().unwrap();
            let old = args["old_string"].as_str().unwrap();
            let new = args["new_string"].as_str().unwrap();

            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if !content.contains(old) {
                        return format!("Error: old_string not found in {}", path);
                    }
                    let updated = content.replace(old, new);
                    match std::fs::write(path, &updated) {
                        Ok(_) => format!("Successfully edited {}", path),
                        Err(e) => format!("Error writing file: {}", e),
                    }
                }
                Err(e) => format!("Error reading file: {}", e),
            }
        }

        "bash" => {
            let command = args["command"].as_str().unwrap();
            match Command::new("sh").arg("-c").arg(command).output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if output.status.success() {
                        stdout.to_string()
                    } else {
                        format!("Exit code {}\nstdout: {}\nstderr: {}",
                            output.status.code().unwrap_or(-1), stdout, stderr)
                    }
                }
                Err(e) => format!("Error running command: {}", e),
            }
        }

        "glob" => {
            let pattern = args["pattern"].as_str().unwrap();
            match glob::glob(pattern) {
                Ok(paths) => {
                    let files: Vec<_> = paths.filter_map(|p| p.ok()).collect();
                    files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")
                }
                Err(e) => format!("Error: {}", e),
            }
        }

        "grep" => {
            let pattern = args["pattern"].as_str().unwrap();
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            match Command::new("rg")
                .args(["--files-with-matches", pattern, path])
                .output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }

        "remember" => {
            let content = args["content"].as_str().unwrap();
            let fact_type = args.get("fact_type").and_then(|v| v.as_str());
            match crate::tools::memory::remember(pool, content, fact_type, None, None).await {
                Ok(_) => "Remembered.".to_string(),
                Err(e) => format!("Error: {}", e),
            }
        }

        "recall" => {
            let query = args["query"].as_str().unwrap();
            match crate::tools::memory::recall(pool, query, None, None, Some(5)).await {
                Ok(results) => {
                    results.iter().map(|r| format!("• {}", r.content)).collect::<Vec<_>>().join("\n")
                }
                Err(e) => format!("Error: {}", e),
            }
        }

        _ => format!("Unknown tool: {}", call.function.name),
    }
}
```

### Continuous Context Layer

```rust
// src/chat/context.rs
use sqlx::SqlitePool;
use crate::tools::SemanticSearch;

pub struct ContinuousContext {
    pub current_project: PathBuf,
    messages: Vec<Message>,           // Full conversation history
    summaries: Vec<Summary>,          // Older chunks, searchable
    pool: SqlitePool,
    qdrant: Option<SemanticSearch>,
}

impl ContinuousContext {
    pub fn new(pool: SqlitePool, qdrant: Option<SemanticSearch>) -> Self {
        Self {
            current_project: detect_project(&std::env::current_dir().unwrap()),
            messages: Vec::new(),
            summaries: Vec::new(),
            pool,
            qdrant,
        }
    }

    pub fn add_message(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    /// Build full message array for DeepSeek API
    pub fn build_messages(&self, system_prompt: &str) -> Vec<Message> {
        let mut result = vec![Message::system(system_prompt)];
        result.extend(self.messages.clone());
        result
    }

    /// Build system prompt with all relevant context injected
    pub async fn build_system_prompt(&self, user_query: &str) -> Result<String> {
        let mut prompt = String::from(CORE_INSTRUCTIONS);
        prompt.push_str("\n\n<context>\n");

        // Project info
        prompt.push_str(&format!("Project: {}\n", self.current_project.display()));

        // Rolling summaries (older conversation)
        if !self.summaries.is_empty() {
            prompt.push_str("\n## Previous conversation summary\n");
            for s in &self.summaries {
                prompt.push_str(&format!("{}\n", s.content));
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

        prompt.push_str("</context>\n\n");

        // Semantic memories (uncached - query dependent)
        if let Some(ref qdrant) = self.qdrant {
            let memories = qdrant.search("memories", user_query, 5).await?;
            if !memories.is_empty() {
                prompt.push_str("<relevant_memories>\n");
                for m in memories {
                    prompt.push_str(&format!("- {}\n", m.content));
                }
                prompt.push_str("</relevant_memories>");
            }
        }

        Ok(prompt)
    }

    /// Summarize older messages when context grows too large
    pub async fn maybe_summarize(&mut self) -> Result<()> {
        let token_estimate = self.messages.len() * 100; // Rough estimate

        if token_estimate > 40_000 {
            let split = self.messages.len() / 2;
            let to_summarize: Vec<_> = self.messages.drain(..split).collect();

            // Generate summary (could use DeepSeek for this too)
            let summary = generate_summary(&to_summarize);
            self.summaries.push(summary);
        }
        Ok(())
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
│   ├── chat/                    # NEW: Interactive assistant
│   │   ├── mod.rs               # Main chat loop
│   │   ├── deepseek.rs          # DeepSeek API client
│   │   ├── context.rs           # ContinuousContext
│   │   ├── tools.rs             # Tool executor
│   │   ├── commands.rs          # Slash command handlers
│   │   └── input.rs             # Readline, history
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

### Phase 1: DeepSeek API Client (~300 lines)
- [ ] `src/chat/deepseek.rs` - API types and client
- [ ] SSE streaming response parser
- [ ] Tool call accumulation
- [ ] Error handling and retries
- [ ] Test: basic chat completion

### Phase 2: Tool Executor (~400 lines)
- [ ] `src/chat/tools.rs` - Tool definitions and executor
- [ ] File tools: read_file, write_file, edit_file
- [ ] Shell tools: bash, glob, grep
- [ ] Memory tools: remember, recall (integrate with existing)
- [ ] Test: tool execution loop

### Phase 3: Basic Chat Loop (~200 lines)
- [ ] `src/chat/mod.rs` - Main loop
- [ ] Add `mira chat` subcommand to main.rs
- [ ] Stream responses to terminal
- [ ] Test: interactive conversation with tools

### Phase 4: Continuous Context (~300 lines)
- [ ] `src/chat/context.rs` - ContinuousContext struct
- [ ] Project detection (cwd → git root)
- [ ] Conversation history management
- [ ] Rolling summaries when context grows
- [ ] Warm context injection from Mira DB

### Phase 5: Slash Commands (~200 lines)
- [ ] `src/chat/commands.rs` - Command handlers
- [ ] `/switch`, `/status`, `/remember`, `/recall`, `/tasks`
- [ ] Direct Mira tool calls (no MCP overhead)

### Phase 6: Polish
- [ ] `rustyline` for readline (history, completion)
- [ ] Ctrl+C handling (cancel current query)
- [ ] Pretty output formatting (markdown rendering?)
- [ ] Config file support (DEEPSEEK_API_KEY, etc.)
- [ ] Error recovery

**Total new code: ~1400 lines** (plus existing Mira tools reused)

---

## Resolved Questions

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Language** | Rust | Single binary, direct Mira integration, fast startup |
| **LLM Provider** | DeepSeek V3.2 | 10x cheaper, auto-caching, disk-persistent cache |
| **Tool implementation** | Mira-owned | We implement read/write/edit/bash/etc via function calling |
| **Mira integration** | Context injection | Query DB/Qdrant, inject into system prompt - no MCP |
| **Context strategy** | Rolling summaries + Mira context | Mira handles all context management, DeepSeek is stateless |
| **Multi-file editing** | Custom edit_file tool | Simple search/replace, can extend later |
| **Architecture** | Extend Mira binary | Add `mira chat` subcommand, reuse existing tools |
| **Sessions** | Eliminated entirely | One neverending conversation, rolling summaries for context |
| **Project detection** | Layered: .mira/ → git → package → cwd | Explicit config wins, then git root, then package files, then cwd |
| **Context budget** | ~64k tokens per request | DeepSeek supports 64k context, auto-caches efficiently |
| **Input library** | rustyline | Mature, history, completion, vi/emacs modes |

## Design Decisions (Detailed Analysis)

### 1. Sessionless Design

**Question:** How do we achieve "no sessions" with a stateless API like DeepSeek?

**Answer:** Mira owns the conversation state. DeepSeek is stateless - we send the full context each request.

```
User's mental model:     One neverending conversation
                              ↓
Mira's job:              Maintain conversation history + rolling summaries
                              ↓
DeepSeek API:            Stateless - receives full context each request
```

**How it works:**

1. User talks to Mira - no concept of "starting" or "ending" anything
2. Mira maintains the conversation history in memory
3. When context grows too large:
   - Mira summarizes older portions
   - Stores summaries in SQLite/Qdrant
   - Keeps recent conversation in full fidelity
   - User never notices - conversation continues seamlessly

**Implementation:**

```rust
impl ContinuousContext {
    /// Build full message array for DeepSeek (stateless API)
    pub fn build_messages(&self, system_prompt: &str) -> Vec<Message> {
        let mut messages = vec![Message::system(system_prompt)];

        // Include rolling summaries if context was summarized
        if !self.summaries.is_empty() {
            let summary_text = self.summaries.iter()
                .map(|s| s.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(Message::system(&format!(
                "<conversation_summary>\n{}\n</conversation_summary>",
                summary_text
            )));
        }

        // Full recent conversation
        messages.extend(self.messages.clone());
        messages
    }
}
```

**Key insight:** The user never sees sessions. They just talk to Mira. DeepSeek is stateless - Mira provides all state.

---

### 1b. Context Management (Rolling Summaries)

**Question:** How do we handle context growth with a 64k token limit?

**Answer:** Mira proactively manages context with rolling summaries before we hit the limit.

```
Conversation grows...
  ↓
Mira (proactively):
  1. Summarize older chunks → store in SQLite/Qdrant
  2. Drop summarized content from active context
  3. Keep recent conversation in full fidelity
  ↓
Next query:
  1. Semantic search finds relevant old context
  2. Re-inject only what's relevant to current query
  3. Send curated context to DeepSeek
```

**Why rolling summaries work well:**

| Naive Approach | Mira Rolling Summaries |
|----------------|------------------------|
| Hit limit, fail | Proactive, never hit limit |
| Lose old context | Old context summarized + searchable |
| One-size-fits-all | Query-relevant re-injection |
| No control | Full control over what's kept |

**Implementation sketch:**

```rust
impl ContinuousContext {
    /// Check if we should summarize older context
    async fn maybe_summarize(&mut self) -> Result<()> {
        let recent_tokens = estimate_tokens(&self.recent_messages);

        // Keep ~50k tokens of recent conversation, summarize rest
        if recent_tokens > 50_000 {
            let to_summarize = self.recent_messages.drain(..self.recent_messages.len() / 2);
            let summary = self.generate_summary(to_summarize).await?;

            // Store summary with embeddings for future retrieval
            self.store_conversation_chunk(&summary).await?;
        }
        Ok(())
    }

    /// Build context with semantic re-injection of old content
    async fn build_context(&self, query: &str) -> String {
        let mut ctx = String::new();

        // Always include: corrections, goals, recent conversation
        ctx.push_str(&self.format_corrections());
        ctx.push_str(&self.format_active_goals());
        ctx.push_str(&self.format_recent_messages());

        // Semantic re-injection: find old context relevant to this query
        let relevant_old = self.qdrant.search("conversations", query, 3).await?;
        if !relevant_old.is_empty() {
            ctx.push_str("\n## Relevant prior context\n");
            for chunk in relevant_old {
                ctx.push_str(&chunk.summary);
            }
        }

        ctx
    }
}
```

**Key insight:** Old conversation isn't lost - it's summarized and searchable. Relevant history resurfaces automatically via semantic match.

**Important: Context-aware retrieval**

Not all context ages the same way:

| Type | Freshness | Source | Example |
|------|-----------|--------|---------|
| **Code** | Current only | Daemon/file watcher index | Symbols, call graphs, file contents |
| **Decisions** | Timeless | Memory DB | "We chose PostgreSQL over MySQL because..." |
| **Preferences** | Timeless | Corrections DB | "Use .expect() not .unwrap()" |
| **Conversations about code** | Stale quickly | DON'T re-inject | Old discussion about auth bug (code has changed) |
| **Conversations about architecture** | Long-lived | Memory DB | "The system uses event sourcing" |

```rust
async fn build_context(&self, query: &str) -> String {
    // Code context: ALWAYS fresh from daemon index
    let code_symbols = self.code_index.search_current(&query).await?;

    // Non-code context: semantic search across time
    let memories = self.qdrant.search("memories", &query, 5).await?;
    let corrections = self.db.get_relevant_corrections(&query).await?;

    // Conversation history: recent only, old stuff is summarized
    // Don't re-inject old code discussions - the code has changed
    let recent_conversation = self.get_recent_messages(20);

    // ...
}
```

**Key insight:** Code is ephemeral (index it live). Knowledge is persistent (store it forever).

**The Gap Problem**

Naive approach creates blind spots:

```
Messages 1-50:    summarized ✓
Messages 51-94:   LIMBO (not summarized, not "recent") ✗
Messages 95-100:  recent ✓
```

**Solution: Overlapping coverage with no gaps**

```
                    [------------ CONTEXT WINDOW ------------]
Summarized:         [====1-50====]
Unsummarized buffer:              [=========51-100==========]
                                                   ↑
                                            current message

Rule: Everything since last summary stays in context (full fidelity)
      Only summarize when buffer would exceed budget
```

Implementation:

```rust
struct ConversationContext {
    summaries: Vec<Summary>,           // Older chunks, searchable
    buffer: Vec<Message>,              // Everything since last summary (full)
    buffer_token_count: usize,
}

impl ConversationContext {
    fn maybe_summarize(&mut self) {
        // Only summarize when buffer exceeds threshold
        if self.buffer_token_count > 40_000 {
            // Summarize older half of buffer
            let split = self.buffer.len() / 2;
            let to_summarize: Vec<_> = self.buffer.drain(..split).collect();

            let summary = generate_summary(&to_summarize);
            self.summaries.push(summary);

            // Remaining buffer stays in full fidelity - NO GAP
            self.recalculate_tokens();
        }
    }

    fn build_context(&self, query: &str) -> String {
        let mut ctx = String::new();

        // Semantic search across ALL summaries
        let relevant = semantic_search(&self.summaries, query);
        for s in relevant {
            ctx.push_str(&s.content);
        }

        // Full buffer - everything since last summary
        for msg in &self.buffer {
            ctx.push_str(&msg.format());
        }

        ctx
    }
}
```

**Key insight:** Never have messages in limbo. Either they're in the buffer (full) or they're summarized (searchable). The handoff is seamless.

**Cache-Optimized Injection Order**

DeepSeek's auto-caching works on prefixes - same prefix = cache hit. Order content from most stable to least stable:

```
[CACHED PREFIX - stable, high cache hit rate]
├── 1. Persona                    (1h TTL - almost never changes)
├── 2. Summaries                  (5m TTL - only grows, prefix stable)
├── 3. Work context               (5m TTL - goals/tasks/corrections)
├── 4. Conversation buffer        (5m TTL - grows but prefix stable)
│
[UNCACHED SUFFIX - changes per query]
├── 5. Semantic memories          (query-dependent, different every time)
└── 6. Current user message
```

Why this order maximizes cache hits:

| Position | Content | Cache Behavior |
|----------|---------|----------------|
| 1 | Persona | Same across all queries → always hits |
| 2 | Summaries | Append-only, old summaries unchanged → prefix hits |
| 3 | Work context | Changes occasionally, but prefix stable within session |
| 4 | Buffer | New messages append, old unchanged → prefix hits |
| 5 | Semantic memories | Different per query → never cached (that's fine) |
| 6 | User message | Always different → never cached |

```rust
fn build_prompt(&self, query: &str) -> Vec<SystemBlock> {
    vec![
        // Cached blocks (stable prefix)
        SystemBlock::cached_1h(self.persona.clone()),
        SystemBlock::cached(self.format_summaries()),
        SystemBlock::cached(self.format_work_context()),
        SystemBlock::cached(self.format_buffer()),

        // Uncached blocks (dynamic suffix)
        SystemBlock::text(self.semantic_search(query)),
        // User message goes in messages array, not system
    ]
}
```

**Key insight:** Put stable stuff first. Semantic memories are intentionally last because they SHOULD change per query - that's the point.

**Code Intelligence = Natural Chunking**

We don't pass raw files - the indexer breaks code into cache-friendly logical units:

```
Raw file approach (bad for caching):
  file.rs (500 lines) → any change = full recache

Code intelligence approach (cache-friendly):
  symbol: auth::validate_token    [cached ✓]
  symbol: auth::refresh_token     [CHANGED - recache]
  symbol: auth::revoke_token      [cached ✓]
  call_graph edges                [cached ✓]
  imports/dependencies            [cached ✓]
```

The daemon/indexer already chunks by:
- Functions/methods (with signatures + bodies)
- Classes/structs
- Import relationships
- Call graph edges

When injecting code context, we inject **symbols relevant to the query**, not whole files. Each symbol is a stable, cacheable unit. Only modified symbols need recaching.

```rust
fn inject_code_context(&self, query: &str) -> Vec<SystemBlock> {
    // Semantic search finds relevant symbols (already chunked)
    let symbols = self.code_index.search(query, 10).await;

    // Each symbol is a cacheable block
    symbols.iter().map(|sym| {
        SystemBlock::cached(format!(
            "// {}::{}\n{}",
            sym.file_path, sym.name, sym.body
        ))
    }).collect()
}
```

**Key insight:** The indexer isn't just for search - it's the chunking strategy for cache-efficient code injection.

**Future: Context-Aware Caching**

Not all content should be cached equally. The file watcher could track modification patterns:

```
Hot (actively editing):   Don't cache - changing too fast, cache writes wasted
Warm (recent edits):      Short TTL cache - might change soon
Cold (stable):            Aggressive caching - high hit rate
```

Detection heuristics:
- Time since last modification
- Edit frequency (3 edits in 5 minutes = hot)
- Explicit signals ("working on this file" vs "reference file")

This applies to everything the watcher processes:
- Code files → symbols chunked, cache based on stability
- Docs → sections chunked, cache based on stability
- Even conversation chunks → recent = hot, older = cold

```rust
enum CacheStrategy {
    Hot,        // No cache_control
    Warm,       // cache_control: 5m
    Cold,       // cache_control: 1h
}

fn cache_strategy_for(path: &Path, last_modified: Duration) -> CacheStrategy {
    if last_modified < Duration::minutes(5) { CacheStrategy::Hot }
    else if last_modified < Duration::hours(1) { CacheStrategy::Warm }
    else { CacheStrategy::Cold }
}
```

**Key insight:** Cache what's stable, don't waste cache writes on hot content.

**Reference implementation:** Studio already does this - see `src/studio/context.rs`:

```
build_tiered_context() returns Vec<SystemBlock>:
├── Block 1: Persona (1h cache - rarely changes)
├── Block 2: Work context (5m cache - goals, tasks, corrections, working docs)
├── Block 3: Session context + rolling summary (5m cache - stable within conversation)
└── Block 4: Semantic memories (NO cache - changes based on user query)
```

Key patterns to reuse:
- `load_rolling_summary()` - Summarized older conversation chunks
- `recall_relevant_memories()` - Semantic search based on recent user messages
- `SystemBlock::cached()` vs `SystemBlock::text()` - Strategic cache control
- Work context loaded but flagged as "background awareness" to avoid steering conversation

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

**DeepSeek V3.2 Specifications:**

| Spec | Value |
|------|-------|
| Context window | 64k tokens |
| Input price | $0.28/M tokens |
| Output price | $0.42/M tokens |
| Cache hit price | $0.028/M tokens (90% off) |
| Cache minimum | 64 tokens |
| Cache persistence | Disk-based (survives across requests) |

**Key insight:** We own the entire prompt. DeepSeek's auto-caching means we don't need to worry about `cache_control` headers - just keep the prefix stable.

**DeepSeek caching details:**
- Automatic caching enabled by default
- Prefix matching: same prefix = cache hit
- **64 token minimum** (vs 1,024 for Anthropic)
- **Disk-persistent** (survives across API calls)
- Cache writes are **free** (no premium like Anthropic's 25%)

**Constraints:**
- 64k context per request
- More context = higher cost (but cache hits are cheap)
- Keep system prompt + warm context under ~10k to leave room for conversation

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
| Built-in tools | ✓ | ✓ (Mira-implemented) |
| **Sessionless** | ✗ (session-based) | ✓ (one neverending conversation) |
| **Context injection** | ✗ (MCP tool calls) | ✓ (pre-loaded into prompt) |
| **Cross-project memory** | ✗ | ✓ (direct DB + Qdrant) |
| **Project switching** | ✗ | ✓ (code context scoped, memory global) |
| **Persistent tasks/goals** | ✗ | ✓ (direct DB) |
| **Code intelligence** | ✗ | ✓ (symbols, call graph) |
| **Git intelligence** | ✗ | ✓ (co-change patterns) |
| **Semantic search** | ✗ | ✓ (query-relevant memories) |
| Cost (input) | $3.00/M (Sonnet 4.5) | **$0.28/M (DeepSeek)** |
| Cost (output) | $15.00/M (Sonnet 4.5) | **$0.42/M (DeepSeek)** |
| Startup time | ~2s | ~10ms |
| Single binary | ✗ (Node.js) | ✓ |

---

## References

### DeepSeek
- [DeepSeek API Documentation](https://platform.deepseek.com/api-docs)
- [DeepSeek Chat API](https://platform.deepseek.com/api-docs/chat)
- [DeepSeek Function Calling](https://platform.deepseek.com/api-docs/function-calling)
- [DeepSeek Context Caching](https://platform.deepseek.com/api-docs/context-caching)

### Mira
- [Mira Repository](https://github.com/yourusername/mira) (this project)
- SQLite for structured data
- Qdrant for semantic embeddings

### Reference: Claude Code (for comparison)
- [Claude Code Repository](https://github.com/anthropics/claude-code)
- [Claude Code System Prompts](https://github.com/Piebald-AI/claude-code-system-prompts) - Research on prompt structure
