# Mira

**A second brain for Claude Code**

Mira transforms Claude Code from a stateless assistant into one that truly knows your project. It remembers your decisions, understands your codebase architecture, and continuously builds intelligence about your code - all persisted locally across sessions.

Think of it as giving Claude Code long-term memory, deep code understanding, and a team of expert reviewers on call.

## The Problem

Every Claude Code session starts fresh. You explain your architecture, your preferences, your decisions - again and again. Claude can't remember that you prefer tabs over spaces, that you chose Redux over Zustand last week, or why the auth module is structured that way. And it doesn't really *understand* your codebase - it just reads files when asked.

## The Solution

Mira runs as an MCP server alongside Claude Code, providing:

- **Persistent Memory** - Decisions, preferences, and context survive across sessions
- **Code Intelligence** - Semantic search, call graphs, and symbol navigation
- **Background Intelligence** - Continuously analyzes your codebase, generates summaries, detects capabilities
- **Expert Consultation** - Second opinions from specialized AI reviewers with codebase access
- **Automatic Documentation** - Detects gaps, flags stale docs, generates updates
- **LLM Proxy** - Route requests to multiple backends with usage tracking
- **Task Tracking** - Goals and tasks that persist across conversations

## Quick Start

```bash
# Build from source
cargo build --release

# Add to your project's .mcp.json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/target/release/mira",
      "args": ["serve"]
    }
  }
}
```

Set `DEEPSEEK_API_KEY` for intelligence features. Optionally set `GEMINI_API_KEY` for embeddings.

Add to your `CLAUDE.md`:
```markdown
## Session Start
session_start(project_path="/path/to/your/project")
Then recall("preferences") before writing code.
```

## Features

### Memory System

```
"Remember that we use the builder pattern for all config structs"
"What did we decide about error handling?"
"Forget memory 42"
```

Memories are evidence-based: new facts start as candidates, gain confidence through repeated use across sessions, and get promoted to confirmed status automatically.

### Code Intelligence

| Capability | What it does |
|------------|--------------|
| `semantic_code_search` | Find code by meaning, not just text |
| `find_callers` / `find_callees` | Trace call relationships |
| `get_symbols` | Extract functions, structs, classes |
| `check_capability` | "Does this codebase have caching?" |

Supports Rust, Python, TypeScript, JavaScript, and Go via tree-sitter parsing.

### Intelligence Engine (DeepSeek-Powered)

DeepSeek Reasoner runs continuously in the background, building understanding of your codebase:

**Background Tasks (automatic):**
| Task | What it does |
|------|--------------|
| Module summaries | Generates human-readable descriptions of code modules |
| Capability detection | Discovers what features exist ("Does this have auth?") |
| Git briefings | Summarizes changes since your last session |
| Code health analysis | Flags complexity issues, poor error handling |
| Tool extraction | Extracts insights from tool results into memories |

**Expert Consultation (on-demand):**
| Expert | Use case |
|--------|----------|
| `consult_architect` | System design, patterns, tradeoffs |
| `consult_code_reviewer` | Bugs, quality issues, improvements |
| `consult_security` | Vulnerabilities, attack vectors |
| `consult_scope_analyst` | Missing requirements, edge cases |
| `consult_plan_reviewer` | Validate implementation plans |

Experts have tool access - they can search code, trace call graphs, and explore the codebase to give informed answers. (Configurable LLM backends planned.)

### Automatic Documentation

Mira tracks your codebase and flags documentation that needs attention:

- **Gap detection** - Finds undocumented MCP tools, public APIs, and modules
- **Staleness tracking** - Flags docs when source code changes
- **Expert generation** - `write_documentation(task_id)` calls the documentation expert to generate and write docs directly

```
list_doc_tasks()        # See what needs documentation
write_documentation(42) # Expert generates and writes the doc
skip_doc_task(42)       # Skip if not needed
```

The documentation expert analyzes the actual code behavior, not just signatures, to produce comprehensive docs.

### LLM Proxy (Experimental)

Route LLM requests through Mira for multi-backend support and usage tracking:

- **Multi-backend routing** - Anthropic (default), DeepSeek, GLM, or any Anthropic-compatible API
- **Dynamic switching** - Change providers at runtime
- **Usage tracking** - Token counts and cost estimation per request
- **Model mapping** - Map model names between proxy and backends

Configure backends with pricing info to track costs across providers.

### Task & Goal Tracking

```
task(action="create", title="Implement auth middleware", priority="high")
goal(action="create", title="v2.0 Release", description="Ship new features")
```

Tasks and goals persist across sessions - pick up where you left off.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
                                  |
                                  +--->  Google (embeddings)
                                  +--->  DeepSeek (intelligence)
```

All data stored locally in `~/.mira/mira.db`. No cloud storage, no external databases.

## Requirements

- Rust toolchain (build from source)
- `DEEPSEEK_API_KEY` - Required for most features: background intelligence, experts, documentation, summaries, capability detection, code health analysis
- `GEMINI_API_KEY` - Optional, enables semantic search embeddings (Google text-embedding-004)

## Documentation

- [Design Philosophy](docs/DESIGN.md) - Architecture decisions and tradeoffs
- [Core Concepts](docs/CONCEPTS.md) - Memory, intelligence, experts explained
- [Configuration](docs/CONFIGURATION.md) - All options and hooks
- [Database Schema](docs/DATABASE.md) - Tables and relationships

## License

MIT
