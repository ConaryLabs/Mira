# Mira Chat - Planning Document

**Power-armored coding assistant.** Persistent memory, code intelligence, best-in-class inference.

## Vision

**Sessions are the problem, not the solution.** Traditional assistants treat each session as isolated. We want the opposite: one continuous, neverending relationship.

A single orchestrator (`mira chat`) that:
- Uses **GPT-5.2 Thinking** as the inference engine (best reasoning, auto-caching)
- **Variable reasoning effort** - scale intelligence to task complexity
- Injects rich context directly into prompts
- **Eliminates sessions entirely** - One conversation, forever
- Mira IS the memory - the relationship persists across everything

## Why GPT-5.2 Over Claude CLI?

| Factor | Claude CLI (Opus 4.5) | GPT-5.2 Thinking |
|--------|----------------------|------------------|
| Context window | 200K | **400K** |
| Max output | ~8K | **128K** |
| Caching | Manual (`cache_control` headers) | **Auto** (prefix-based) |
| Cache hit discount | 90% | **90%** |
| Cache write cost | +25% | **Free** |
| Base pricing | $5/M input, $25/M output | **$1.75/M input, $14/M output** |
| Cached pricing | $0.50/M input | **$0.175/M input** |
| ARC-AGI-2 (reasoning) | 37.6% | **52.9%** |
| SWE-bench (coding) | 80.9% | **80.0%** |
| Reasoning control | None | **5 levels (none→xhigh)** |
| Tools | Built-in | We implement |

**Bottom line:** GPT-5.2 has the best reasoning (ARC-AGI-2: 52.9%), massive context (400K), huge output (128K), and variable reasoning effort. At ~$2-3/session with caching, it fits comfortably in a $200/month budget.

## Reasoning Effort Levels

GPT-5.2's killer feature: **scale reasoning to task complexity**.

| Level | Token Cost | Latency | Use Case |
|-------|------------|---------|----------|
| `none` | 1x | Fastest | Tool execution, simple queries |
| `low` | ~3-5x | Fast | Code navigation, file search |
| `medium` | ~8-10x | Moderate | Code understanding, standard edits |
| `high` | ~15-20x | Slower | Complex refactoring, architecture |
| `xhigh` | ~23x | Slowest | Critical debugging, deep analysis |

### Task Router

```
User query → Mira classifies complexity → Sets reasoning_effort → GPT-5.2

Examples:
  "read src/main.rs"           → none   (just execute tool)
  "what does this function do" → medium (understand code)
  "refactor auth system"       → high   (multi-file planning)
  "why is this deadlocking"    → xhigh  (deep debugging)
```

This lets us use GPT-5.2's full power when needed while keeping costs low for simple operations.

### Tool Implementation

We implement Claude Code-style tools via GPT-5.2 function calling. Tool definitions based on [Piebald-AI/claude-code-system-prompts](https://github.com/Piebald-AI/claude-code-system-prompts).

#### Tool Definitions (JSON Schema for GPT-5.2)

```rust
fn define_tools() -> Vec<Tool> {
    vec![
        // READ FILE - Reads files from local filesystem
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "read_file".into(),
                description: Some("Reads a file from the local filesystem. Supports text, code, images (PNG/JPG), PDFs, and Jupyter notebooks. Results returned in cat -n format with line numbers. Use absolute paths only.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Absolute path to the file to read"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Line number to start reading from (1-indexed). Omit to read from beginning."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Number of lines to read. Omit to read entire file (up to 2000 lines)."
                        }
                    },
                    "required": ["file_path"]
                }),
            },
        },

        // WRITE FILE - Creates or overwrites files
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "write_file".into(),
                description: Some("Writes content to a file. Will overwrite existing files. IMPORTANT: You MUST read existing files before writing to them. Prefer editing existing files over creating new ones.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Absolute path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["file_path", "content"]
                }),
            },
        },

        // EDIT FILE - Exact string replacement
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "edit_file".into(),
                description: Some("Performs exact string replacement in files. MUST read the file first. The old_string must be unique in the file, or use replace_all for global replacement. Preserve exact indentation.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Absolute path to the file to edit"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "Exact text to find and replace (must be unique unless using replace_all)"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "Text to replace old_string with"
                        },
                        "replace_all": {
                            "type": "boolean",
                            "description": "If true, replace all occurrences. Default false."
                        }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                }),
            },
        },

        // BASH - Execute shell commands
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "bash".into(),
                description: Some("Executes bash commands. Use for git, npm, cargo, docker, etc. Do NOT use for file operations (use read_file, write_file, edit_file instead). Quote paths with spaces.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The bash command to execute"
                        },
                        "description": {
                            "type": "string",
                            "description": "Brief 5-10 word description of what this command does"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in milliseconds. Default 120000 (2 min), max 600000 (10 min)."
                        }
                    },
                    "required": ["command"]
                }),
            },
        },

        // GLOB - Find files by pattern
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "glob".into(),
                description: Some("Fast file pattern matching. Supports patterns like '**/*.rs' or 'src/**/*.ts'. Returns matching files sorted by modification time.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts')"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search in. Defaults to current directory."
                        }
                    },
                    "required": ["pattern"]
                }),
            },
        },

        // GREP - Search file contents
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "grep".into(),
                description: Some("Powerful search built on ripgrep. Supports full regex. Use this instead of 'grep' or 'rg' bash commands.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "File or directory to search. Defaults to current directory."
                        },
                        "glob": {
                            "type": "string",
                            "description": "Filter files by glob pattern (e.g., '*.rs', '**/*.ts')"
                        },
                        "output_mode": {
                            "type": "string",
                            "enum": ["content", "files_with_matches", "count"],
                            "description": "Output mode: 'content' (matching lines), 'files_with_matches' (file paths only, default), 'count' (match counts)"
                        },
                        "case_insensitive": {
                            "type": "boolean",
                            "description": "Case insensitive search"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
        },

        // REMEMBER - Store in Mira memory
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "remember".into(),
                description: Some("Store a fact, decision, or preference in Mira's persistent memory for future recall.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The information to remember"
                        },
                        "fact_type": {
                            "type": "string",
                            "enum": ["preference", "decision", "context", "general"],
                            "description": "Type of information being stored"
                        },
                        "category": {
                            "type": "string",
                            "description": "Optional category for filtering"
                        }
                    },
                    "required": ["content"]
                }),
            },
        },

        // RECALL - Search Mira memory
        Tool {
            r#type: "function".into(),
            function: Function {
                name: "recall".into(),
                description: Some("Search Mira's persistent memory using semantic similarity.".into()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max results to return. Default 5."
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
    ]
}
```

This is actually **better** than Claude CLI because:
- We control tool execution completely
- Can add safety checks, logging, permissions
- Tools integrate directly with Mira (no MCP overhead)
- Can extend with Mira-specific tools (remember, recall, etc.)
- Full visibility into what's happening

### Context Scoping

| Scope | What | Why |
|-------|------|-----|
| **Project** | Code symbols, call graphs, git history, file context | Don't mix code from different projects |
| **Global** | Memories, corrections, preferences, goals, conversation history | These follow you everywhere |

When you `cd /home/peter/Mira`, code context switches. But the relationship continues - same memories, same corrections, same ongoing conversation.

## What We Implement (Core Tools)

Built with GPT-5.2 function calling:
- **File tools**: read_file, write_file, edit_file
- **Shell tools**: bash, glob, grep
- **Memory tools**: remember, recall (integrated with Mira DB)
- **Streaming**: SSE response streaming to terminal
- **Reasoning router**: Classify task complexity → set reasoning_effort

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
  1. Classify complexity → set reasoning_effort (high for bug fix)
  2. Semantic search: find memories related to "auth"
  3. Load: corrections, active goals, recent sessions
  4. Query: relevant code symbols, recent commits
  5. Build system prompt with all context
  6. Call GPT-5.2 API with tools + context + reasoning_effort
  ↓
GPT-5.2: Returns tool calls (read_file, edit_file, etc.)
  ↓
Mira: Executes tools, returns results, loops until done
  ↓
GPT-5.2: "I've fixed the auth bug by..."
  ↓
Mira: Stream to terminal, store in conversation history
```

---

## Architecture Overview

**Key insight:** Mira is the orchestrator. GPT-5.2 is the inference engine. We own the tool loop.

```
┌─────────────────────────────────────────────────────────────────┐
│                      Mira (Orchestrator)                        │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Input Layer                                             │   │
│  │  - Readline (rustyline)                                  │   │
│  │  - Slash commands: /remember, /recall, /tasks, /switch   │   │
│  │  - Regular prompts → GPT-5.2                             │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Reasoning Router                                        │   │
│  │  - Classify task complexity                              │   │
│  │  - Set reasoning_effort: none/low/medium/high/xhigh      │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Context Builder                                         │   │
│  │  - Query SQLite: memories, corrections, goals, tasks     │   │
│  │  - Query Qdrant: semantic search for relevant context    │   │
│  │  - Query code index: symbols, call graph, co-change      │   │
│  │  - Build system prompt with all context injected         │   │
│  │  - GPT-5.2 auto-caches stable prefix                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  GPT-5.2 API Client                                      │   │
│  │  - POST to api.openai.com/v1/chat/completions            │   │
│  │  - Stream SSE responses                                  │   │
│  │  - Function calling for tools                            │   │
│  │  - reasoning_effort parameter                            │   │
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

**What GPT-5.2 handles (via API):**
- Code understanding and generation
- Deciding which tools to call
- Reasoning about context (scaled by reasoning_effort)

**What Mira executes (tool calls from GPT-5.2):**
- File operations (read, write, edit)
- Shell commands (bash)
- Search (glob, grep)
- Memory (remember, recall - integrated as tools too)

**What we build:** Everything (orchestrator, tools, context, API client, reasoning router)
**What we get for free:** GPT-5.2's auto-caching, best-in-class reasoning

---

## GPT-5.2 Responses API Integration

### Why Responses API (Not Chat Completions)

GPT-5.2 works best with the **Responses API**, OpenAI's cutting-edge agentic interface (Dec 2025):

| Feature | Chat Completions | Responses API |
|---------|-----------------|---------------|
| **Conversation state** | Send full history each turn | `previous_response_id` |
| **Chain of thought** | Discarded between turns | **Preserved across turns** |
| **Cache utilization** | Standard | **40-80% better** |
| **Benchmark (TAUBench)** | Baseline | **+5% with GPT-5.2** |
| **Output format** | Single message | Polymorphic items |

### API Overview

```
POST https://api.openai.com/v1/responses
Authorization: Bearer $OPENAI_API_KEY
Content-Type: application/json

{
  "model": "gpt-5.2",
  "input": "Fix the auth bug in src/auth.rs",
  "instructions": "You are Mira, a power-armored coding assistant...",
  "previous_response_id": "resp_abc123",  // Conversation continuity
  "reasoning": { "effort": "high" },
  "tools": [...],
  "stream": true
}
```

### Key Features

| Feature | Details |
|---------|---------|
| **Context window** | 400K tokens |
| **Max output** | 128K tokens |
| **CoT preservation** | Reasoning state persists across turns (hidden but used) |
| **Cache hit discount** | 90% ($0.175/M vs $1.75/M) |
| **Reasoning effort** | none / low / medium / high / xhigh |
| **Conversation state** | Managed via `previous_response_id` |
| **Output items** | Reasoning summaries, messages, tool calls, results |
| **Pricing** | $0.175/M cache hit, $1.75/M cache miss, $14/M output |
| **ARC-AGI-2** | 52.9% (best abstract reasoning) |
| **Compact endpoint** | `/responses/compact` for long workflows |

### Basic Usage (Rust)

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ResponsesRequest {
    model: String,
    input: String,
    instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
    reasoning: ReasoningConfig,
    tools: Vec<Tool>,
    stream: bool,
}

#[derive(Serialize)]
struct ReasoningConfig {
    effort: String,  // "none" | "low" | "medium" | "high" | "xhigh"
}

#[derive(Deserialize)]
struct ResponsesResponse {
    id: String,  // Use as previous_response_id for next turn
    output: Vec<OutputItem>,
    usage: Usage,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OutputItem {
    #[serde(rename = "reasoning")]
    Reasoning { summary: String },
    #[serde(rename = "message")]
    Message { content: String },
    #[serde(rename = "function_call")]
    FunctionCall { name: String, arguments: String, call_id: String },
}

async fn respond(
    client: &Client,
    input: &str,
    instructions: &str,
    previous_response_id: Option<&str>,
    reasoning_effort: &str,
    tools: &[Tool],
) -> Result<ResponsesResponse> {
    let request = ResponsesRequest {
        model: "gpt-5.2".into(),
        input: input.into(),
        instructions: instructions.into(),
        previous_response_id: previous_response_id.map(String::from),
        reasoning: ReasoningConfig { effort: reasoning_effort.into() },
        tools: tools.to_vec(),
        stream: true,
    };

    let response = client
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(std::env::var("OPENAI_API_KEY")?)
        .json(&request)
        .send()
        .await?;

    // Handle SSE stream, collect output items...
    parse_response(response).await
}
```

### Conversation Flow with Responses API

```
Turn 1:
  Mira → GPT-5.2: input="fix auth bug", previous_response_id=None
  GPT-5.2 → Mira: response_id="resp_001", output=[FunctionCall{read_file}]

Turn 2 (after tool execution):
  Mira → GPT-5.2: input=<tool_result>, previous_response_id="resp_001"
  GPT-5.2 → Mira: response_id="resp_002", output=[FunctionCall{edit_file}]

  (GPT-5.2 internally has full CoT from turn 1 - better reasoning!)

Turn 3:
  Mira → GPT-5.2: input=<tool_result>, previous_response_id="resp_002"
  GPT-5.2 → Mira: response_id="resp_003", output=[Message{"I've fixed..."}]
```

**Key insight:** We don't send full conversation history. OpenAI manages state. We just pass `previous_response_id` and the model has full context including hidden chain-of-thought.

### Tool Calling Flow

```
User: "Fix the auth bug in src/auth.rs"
            ↓
Mira: Classify → high complexity, set reasoning_effort="high"
            ↓
Mira: Build context, send to GPT-5.2 with tools + reasoning
            ↓
GPT-5.2: Returns tool_call: read_file("src/auth.rs")
            ↓
Mira: Executes read_file, returns content
            ↓
GPT-5.2: Analyzes (high reasoning), returns tool_call: edit_file(...)
            ↓
Mira: Executes edit, returns success
            ↓
GPT-5.2: "I've fixed the auth bug by..."
            ↓
Mira: Stream to terminal, done
```

### Cache Behavior

GPT-5.2's auto-cache works on **prefix matching**:

```
Request 1: [system prompt] + [context] + "fix auth bug"
           └─────────────────────────┘
                    cached prefix

Request 2: [system prompt] + [context] + "now add tests"
           └─────────────────────────┘
                 cache HIT (90% off: $0.175/M)
```

**Key points:**
- Only the **repeated prefix** triggers cache hits
- Cache is automatic - no configuration needed
- 90% discount on cache hits ($0.175/M vs $1.75/M)

Since our system prompt and context are prepended, they naturally form a stable prefix → high cache hit rate.

---

## What We Build

### Main Chat Loop

```rust
// src/chat/mod.rs - Main chat loop (Responses API)
use crate::tools::SemanticSearch;
use crate::chat::{responses, context::ContinuousContext, tools::execute_tool, reasoning::classify_effort};

pub async fn run(pool: SqlitePool, semantic: Option<SemanticSearch>) -> Result<()> {
    let client = reqwest::Client::new();
    let mut ctx = ContinuousContext::new(pool.clone(), semantic);
    let mut editor = create_editor()?;

    // Track response chain for conversation continuity
    let mut previous_response_id: Option<String> = None;

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

        // Classify task complexity → set reasoning effort
        let reasoning_effort = classify_effort(&input, &ctx);

        // Build instructions with Mira context (corrections, goals, memories)
        let instructions = ctx.build_instructions(&input).await?;
        let tools = define_tools();

        // Agentic loop: keep calling GPT-5.2 until task complete
        let mut current_input = input.clone();
        loop {
            let response = responses::create(
                &client,
                &current_input,
                &instructions,
                previous_response_id.as_deref(),
                &reasoning_effort,
                &tools,
            ).await?;

            // Update response chain
            previous_response_id = Some(response.id.clone());

            // Process output items
            for item in &response.output {
                match item {
                    OutputItem::Message { content } => {
                        print!("{}", content);
                    }
                    OutputItem::FunctionCall { name, arguments, call_id } => {
                        let result = execute_tool(name, arguments, &pool, &ctx).await;
                        current_input = format_tool_result(call_id, &result);
                        continue; // Next iteration with tool result
                    }
                    OutputItem::Reasoning { summary } => {
                        // Optional: show reasoning summary in debug mode
                    }
                }
            }

            // No more function calls = task complete
            break;
        }

        println!(); // Newline after response
    }

    Ok(())
}
```

### Responses API Client

```rust
// src/chat/responses.rs - GPT-5.2 Responses API client
use reqwest::Client;
use serde::{Deserialize, Serialize};
use futures::StreamExt;

const API_URL: &str = "https://api.openai.com/v1/responses";

#[derive(Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: String,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub reasoning: ReasoningConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    pub stream: bool,
}

#[derive(Serialize)]
pub struct ReasoningConfig {
    pub effort: String,
}

#[derive(Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub output: Vec<OutputItem>,
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum OutputItem {
    #[serde(rename = "reasoning")]
    Reasoning { summary: String },
    #[serde(rename = "message")]
    Message { content: String },
    #[serde(rename = "function_call")]
    FunctionCall { name: String, arguments: String, call_id: String },
}

/// Token usage with cache metrics
#[derive(Deserialize, Debug)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_input_tokens: u32,  // Tokens served from cache (90% off)
}

pub async fn create(
    client: &Client,
    input: &str,
    instructions: &str,
    previous_response_id: Option<&str>,
    reasoning_effort: &str,
    tools: &[Tool],
) -> Result<ResponsesResponse> {
    let request = ResponsesRequest {
        model: "gpt-5.2".into(),
        input: input.into(),
        instructions: instructions.into(),
        previous_response_id: previous_response_id.map(String::from),
        reasoning: ReasoningConfig { effort: reasoning_effort.into() },
        tools: tools.to_vec(),
        stream: true,
    };

    let api_key = std::env::var("OPENAI_API_KEY")?;

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

    /// Build instructions with Mira context for Responses API
    /// Note: Conversation history is managed by previous_response_id, not us
    pub async fn build_instructions(&self, user_query: &str) -> Result<String> {
        let mut instructions = String::from(CORE_INSTRUCTIONS);
        instructions.push_str("\n\n<context>\n");

        // Project info
        instructions.push_str(&format!("Project: {}\n", self.current_project.display()));

        // Active corrections (always include)
        let corrections = tools::corrections::list(&self.pool, 5).await?;
        if !corrections.is_empty() {
            instructions.push_str("\n## Corrections (follow these)\n");
            for c in corrections {
                instructions.push_str(&format!("- {}\n", c.what_is_right));
            }
        }

        // Active goals
        let goals = tools::goals::list_active(&self.pool, 3).await?;
        if !goals.is_empty() {
            instructions.push_str("\n## Active goals\n");
            for g in goals {
                instructions.push_str(&format!("- [{}] {}\n", g.priority, g.title));
            }
        }

        instructions.push_str("</context>\n\n");

        // Semantic memories (query-relevant)
        if let Some(ref qdrant) = self.qdrant {
            let memories = qdrant.search("memories", user_query, 5).await?;
            if !memories.is_empty() {
                instructions.push_str("<relevant_memories>\n");
                for m in memories {
                    instructions.push_str(&format!("- {}\n", m.content));
                }
                instructions.push_str("</relevant_memories>");
            }
        }

        Ok(instructions)
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
│   │   ├── responses.rs         # GPT-5.2 Responses API client
│   │   ├── reasoning.rs         # Task complexity classifier
│   │   ├── context.rs           # Instructions builder (Mira context)
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
│   └── instructions.md          # Core instructions for GPT-5.2
└── Cargo.toml
```

**Key insight:** We extend Mira, not build something separate. The chat mode reuses all existing tool implementations directly - no MCP serialization overhead for Mira operations.

---

## Implementation Phases

### Phase 1: Responses API Client (~350 lines)
- [ ] `src/chat/responses.rs` - API types and client
- [ ] SSE streaming response parser
- [ ] Output item handling (Reasoning, Message, FunctionCall)
- [ ] `previous_response_id` chain management
- [ ] Error handling and retries
- [ ] Test: basic response creation

### Phase 2: Reasoning Router (~150 lines)
- [ ] `src/chat/reasoning.rs` - Task complexity classifier
- [ ] Keyword/pattern-based classification
- [ ] Map complexity → reasoning_effort (none/low/medium/high/xhigh)
- [ ] Test: classification accuracy

### Phase 3: Tool Executor (~400 lines)
- [ ] `src/chat/tools.rs` - Tool definitions and executor
- [ ] File tools: read_file, write_file, edit_file
- [ ] Shell tools: bash, glob, grep
- [ ] Memory tools: remember, recall (integrate with existing)
- [ ] Test: tool execution loop

### Phase 4: Basic Chat Loop (~200 lines)
- [ ] `src/chat/mod.rs` - Main loop with Responses API
- [ ] Add `mira chat` subcommand to main.rs
- [ ] Stream responses to terminal
- [ ] Response chain management
- [ ] Test: interactive conversation with tools

### Phase 5: Context/Instructions Builder (~250 lines)
- [ ] `src/chat/context.rs` - Build instructions with Mira context
- [ ] Project detection (cwd → git root)
- [ ] Inject corrections, goals, memories into instructions
- [ ] (Note: no need for rolling summaries - `previous_response_id` handles state)

### Phase 6: Slash Commands (~200 lines)
- [ ] `src/chat/commands.rs` - Command handlers
- [ ] `/switch`, `/status`, `/remember`, `/recall`, `/tasks`
- [ ] Direct Mira tool calls (no MCP overhead)

### Phase 7: Polish
- [ ] `rustyline` for readline (history, completion)
- [ ] Ctrl+C handling (cancel current query)
- [ ] Pretty output formatting (markdown rendering?)
- [ ] Config file support (OPENAI_API_KEY, etc.)
- [ ] Error recovery

**Total new code: ~1550 lines** (plus existing Mira tools reused)

---

## Resolved Questions

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Language** | Rust | Single binary, direct Mira integration, fast startup |
| **LLM Provider** | GPT-5.2 Thinking | Best reasoning (ARC-AGI-2: 52.9%), 400K context, 128K output |
| **API** | Responses API | CoT preservation, `previous_response_id`, 40-80% better cache |
| **Reasoning control** | Variable effort | none/low/medium/high/xhigh based on task complexity |
| **Tool implementation** | Mira-owned | We implement read/write/edit/bash/etc via function calling |
| **Mira integration** | Instructions injection | Query DB/Qdrant, inject into `instructions` parameter |
| **Conversation state** | `previous_response_id` | OpenAI manages state, we just pass response IDs |
| **Multi-file editing** | Custom edit_file tool | Simple search/replace, can extend later |
| **Architecture** | Extend Mira binary | Add `mira chat` subcommand, reuse existing tools |
| **Sessions** | Eliminated entirely | One neverending conversation via `previous_response_id` |
| **Project detection** | Layered: .mira/ → git → package → cwd | Explicit config wins, then git root, then package files, then cwd |
| **Context budget** | 400K tokens per request | GPT-5.2 supports 400K context, 128K max output |
| **Input library** | rustyline | Mature, history, completion, vi/emacs modes |

## Design Decisions (Detailed Analysis)

### 1. Sessionless Design

**Question:** How do we achieve "no sessions"?

**Answer:** The Responses API's `previous_response_id` handles conversation state. OpenAI manages history; we just pass response IDs.

```
User's mental model:     One neverending conversation
                              ↓
Mira's job:              Track previous_response_id + inject context into instructions
                              ↓
GPT-5.2 Responses API:   Maintains full conversation state + hidden CoT
```

**How it works:**

1. User talks to Mira - no concept of "starting" or "ending" anything
2. Each response returns an `id` (e.g., "resp_001")
3. Next request includes `previous_response_id: "resp_001"`
4. GPT-5.2 has full conversation context + preserved chain-of-thought
5. For very long conversations, use `/responses/compact` endpoint

**Implementation:**

```rust
struct ConversationState {
    previous_response_id: Option<String>,
}

impl ConversationState {
    /// Update after each response
    pub fn update(&mut self, response: &ResponsesResponse) {
        self.previous_response_id = Some(response.id.clone());
    }

    /// Build request with conversation continuity
    pub fn build_request(
        &self,
        input: &str,
        instructions: &str,
        reasoning_effort: &str,
        tools: &[Tool],
    ) -> ResponsesRequest {
        ResponsesRequest {
            model: "gpt-5.2".into(),
            input: input.into(),
            instructions: instructions.into(),
            previous_response_id: self.previous_response_id.clone(),
            reasoning: ReasoningConfig { effort: reasoning_effort.into() },
            tools: tools.to_vec(),
            stream: true,
        }
    }
}
```

**Key insight:** The user never sees sessions. OpenAI manages conversation state via response IDs. Mira just tracks the chain.

---

### 1b. Context Management (Responses API)

**Question:** How do we handle context growth with 400K tokens?

**Answer:** The Responses API handles conversation history. Mira focuses on injecting *relevant context* into `instructions`.

```
Conversation grows...
  ↓
GPT-5.2 (automatically):
  1. Maintains conversation history via previous_response_id
  2. Preserves hidden chain-of-thought
  3. For very long conversations: /responses/compact endpoint
  ↓
Mira's job:
  1. Query relevant memories/corrections/goals
  2. Inject into instructions parameter
  3. Pass previous_response_id
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

**Cache-Optimized Instructions Order**

GPT-5.2's auto-caching works on prefixes - same prefix = cache hit. Order instructions from most stable to least stable:

```
[CACHED PREFIX in instructions - stable, high cache hit rate]
├── 1. Core persona/instructions  (almost never changes)
├── 2. Project context            (changes on /switch)
├── 3. Corrections                (changes occasionally)
├── 4. Active goals               (changes occasionally)
│
[UNCACHED SUFFIX in instructions - changes per query]
└── 5. Semantic memories          (query-dependent, different every time)

Conversation history:             Managed by previous_response_id
```

Why this order maximizes cache hits:

| Position | Content | Cache Behavior |
|----------|---------|----------------|
| 1 | Core instructions | Same across all queries → always hits |
| 2 | Project context | Changes on project switch only |
| 3 | Corrections | Changes occasionally, prefix stable |
| 4 | Active goals | Changes occasionally, prefix stable |
| 5 | Semantic memories | Different per query → never cached (that's fine) |

```rust
fn build_instructions(&self, query: &str) -> String {
    let mut instructions = String::new();

    // Stable prefix (cached by GPT-5.2)
    instructions.push_str(CORE_INSTRUCTIONS);
    instructions.push_str(&self.format_project_context());
    instructions.push_str(&self.format_corrections());
    instructions.push_str(&self.format_goals());

    // Dynamic suffix (not cached - that's fine)
    instructions.push_str(&self.semantic_search(query));

    instructions
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

**Question:** How much Mira context to inject into `instructions`? What's the token budget?

**GPT-5.2 Thinking Specifications:**

| Spec | Value |
|------|-------|
| Context window | **400K tokens** |
| Max output | **128K tokens** |
| Input price (cache miss) | $1.75/M tokens |
| Input price (cache hit) | $0.175/M tokens (90% off) |
| Output price | $14.00/M tokens |
| Reasoning effort | none / low / medium / high / xhigh |
| Conversation state | `previous_response_id` |
| Long workflows | `/responses/compact` endpoint |

**Key insight:** With 400K context and `previous_response_id`, we have massive headroom. GPT-5.2's auto-caching means we just keep the instructions prefix stable.

**GPT-5.2 caching details:**
- Automatic caching enabled by default, no configuration needed
- Prefix matching: only the **repeated prefix** triggers cache hits
- 40-80% better cache utilization than Chat Completions
- Conversation state managed by `previous_response_id`, not in instructions
- Chain of thought preserved across turns (hidden but used)

**Constraints:**
- 400K context per request (massive headroom)
- 128K max output per response
- Conversation history managed by OpenAI (no token cost for us)
- Instructions should contain: persona, project context, corrections, goals, relevant memories

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
| **Sessionless** | ✗ (session-based) | ✓ (`previous_response_id` chain) |
| **Context injection** | ✗ (MCP tool calls) | ✓ (pre-loaded into instructions) |
| **Cross-project memory** | ✗ | ✓ (direct DB + Qdrant) |
| **Project switching** | ✗ | ✓ (code context scoped, memory global) |
| **Persistent tasks/goals** | ✗ | ✓ (direct DB) |
| **Code intelligence** | ✗ | ✓ (symbols, call graph) |
| **Git intelligence** | ✗ | ✓ (co-change patterns) |
| **Semantic search** | ✗ | ✓ (query-relevant memories) |
| **Variable reasoning** | ✗ | ✓ (none/low/medium/high/xhigh) |
| **CoT preservation** | ✗ | ✓ (Responses API) |
| Context window | 200K | **400K** |
| Max output | ~8K | **128K** |
| Cost (cached input) | $0.50/M (Opus 4.5) | **$0.175/M (GPT-5.2)** |
| Cost (output) | $25.00/M (Opus 4.5) | **$14.00/M (GPT-5.2)** |
| Reasoning benchmark | 37.6% ARC-AGI-2 | **52.9% ARC-AGI-2** |
| Startup time | ~2s | ~10ms |
| Single binary | ✗ (Node.js) | ✓ |

---

## References

### OpenAI GPT-5.2
- [Introducing GPT-5.2](https://openai.com/index/introducing-gpt-5-2/)
- [Responses API Documentation](https://platform.openai.com/docs/api-reference/responses)
- [GPT-5.2 Model Guide](https://platform.openai.com/docs/guides/latest-model)
- [Why We Built the Responses API](https://developers.openai.com/blog/responses-api/)

### Mira
- [Mira Repository](https://github.com/yourusername/mira) (this project)
- SQLite for structured data
- Qdrant for semantic embeddings

### Reference: Claude Code (for comparison)
- [Claude Code Repository](https://github.com/anthropics/claude-code)
- [Claude Code System Prompts](https://github.com/Piebald-AI/claude-code-system-prompts) - Research on prompt structure
