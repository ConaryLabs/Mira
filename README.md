# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A second brain for Claude Code**

![Mira Demo](demo.gif)

Mira transforms Claude Code from a stateless assistant into one that truly knows your project. It remembers your decisions, understands your codebase architecture, and continuously builds intelligence about your code - all persisted locally across sessions.

Think of it as giving Claude Code long-term memory, deep code understanding, and a team of expert reviewers on call.

## What's New in v0.3.5

- **SQLite Concurrency Overhaul**: Code index sharded into separate `mira-code.db` - indexing no longer blocks tool calls
- **Write Contention Fixes**: `busy_timeout`, retry logic, and fire-and-forget logging eliminate `SQLITE_BUSY` failures
- **Dual Database Pools**: Background workers and tool handlers route to the correct database automatically

See the [CHANGELOG](CHANGELOG.md) for full version history.

## The Problem

Every Claude Code session starts fresh. You explain your architecture, your preferences, your decisions - again and again. Claude can't remember that you prefer tabs over spaces, that you chose Redux over Zustand last week, or why the auth module is structured that way. And it doesn't really *understand* your codebase - it just reads files when asked.

## The Solution

Mira runs as an MCP server alongside Claude Code, providing:

- **Persistent Memory** - Decisions, preferences, and context survive across sessions
- **Code Intelligence** - Semantic search, call graphs, and symbol navigation
- **Background Intelligence** - Continuously analyzes your codebase, generates summaries, detects capabilities
- **Expert Consultation** - Second opinions from specialized AI reviewers with codebase access
- **Automatic Documentation** - Detects gaps, flags stale docs, generates updates
- **Goal Tracking** - Goals and milestones that persist across conversations

## Installation

### Quick Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

This automatically detects your OS, downloads the binary, and installs the Claude Code plugin.

Then add your API keys to `~/.mira/.env`:
```bash
DEEPSEEK_API_KEY=your-key-here  # https://platform.deepseek.com/api_keys
GEMINI_API_KEY=your-key-here    # https://aistudio.google.com/app/apikey
```

### Manual Install

<details>
<summary>Click to expand manual installation options</summary>

#### Download Binary

**Linux/macOS:**
```bash
# Linux (x86_64)
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv mira /usr/local/bin/

# macOS (Apple Silicon)
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-aarch64-apple-darwin.tar.gz | tar xz
sudo mv mira /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-apple-darwin.tar.gz | tar xz
sudo mv mira /usr/local/bin/
```

**Windows (PowerShell):**
```powershell
Invoke-WebRequest -Uri "https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-pc-windows-msvc.zip" -OutFile mira.zip
Expand-Archive mira.zip -DestinationPath .
Remove-Item mira.zip
Move-Item mira.exe C:\Tools\  # Or another directory in your PATH
```

#### Install Plugin

```bash
claude plugin install ConaryLabs/Mira
```

</details>

### Option 2: Install via Cargo (MCP Server Only)

```bash
cargo install --git https://github.com/ConaryLabs/Mira.git
```

Then add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "mira",
      "args": ["serve"]
    }
  }
}
```

### Option 3: Build from Source

```bash
git clone https://github.com/ConaryLabs/Mira.git
cd Mira
cargo build --release
```

The binary will be at `target/release/mira`. Add to `.mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira",
      "args": ["serve"]
    }
  }
}
```

### Set Up API Keys

Create `~/.mira/.env`:

```bash
# Required for intelligence features
DEEPSEEK_API_KEY=sk-your-key-here

# Required for semantic search
GEMINI_API_KEY=your-key-here
```

Get your keys from:
- DeepSeek: https://platform.deepseek.com/api_keys
- Gemini: https://aistudio.google.com/app/apikey

### Add to CLAUDE.md

Add Mira instructions to your project's `CLAUDE.md` so Claude Code knows how to use the tools effectively.

**Minimal setup** - Add this at minimum:

```markdown
## Session Start
project(action="start", project_path="/path/to/your/project")
Then recall("preferences") before writing code.
```

**Recommended setup** - For best results, include guidance on:
- When to use Mira tools vs Grep/Glob
- Memory system usage (remember/recall)
- Goal tracking for multi-session work
- Expert consultation patterns
- Sub-agent context injection

See **[docs/CLAUDE_TEMPLATE.md](docs/CLAUDE_TEMPLATE.md)** for a complete template you can copy into your project.

### Enable Hooks (Required for Full Features)

The quick install script automatically configures hooks. For manual installs, add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 3000}]}],
    "UserPromptSubmit": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 3000}]}],
    "SessionStart": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 3000}]}],
    "PreCompact": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 5000}]}]
  }
}
```

Hooks enable:
- **Behavior tracking** - Learns your workflow patterns for proactive suggestions
- **Session awareness** - Captures session IDs for context persistence
- **Proactive context** - Injects relevant memories and predictions into prompts

### Plugin vs MCP Server

The **plugin installation** (Option 1) provides the full Mira experience with proactive context injection - auto-recall on every prompt, hooks for file changes, and session-aware features.

The **MCP server installation** (Options 2-3) provides the core tools. Add hooks manually (see above) for proactive features.

See [plugin/README.md](plugin/README.md) for advanced plugin configuration.

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
| `search_code` | Find code by meaning, not just text |
| `find_callers` / `find_callees` | Trace call relationships |
| `get_symbols` | Extract functions, structs, classes |

Supports Rust, Python, TypeScript, JavaScript, and Go via tree-sitter parsing.

### Intelligence Engine

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

```
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")
```

| Expert | Use case |
|--------|----------|
| `architect` | System design, patterns, tradeoffs |
| `code_reviewer` | Bugs, quality issues, improvements |
| `security` | Vulnerabilities, attack vectors |
| `scope_analyst` | Missing requirements, edge cases |
| `plan_reviewer` | Validate implementation plans |

Experts have tool access - they can search code, trace call graphs, and explore the codebase to give informed answers.

### Automatic Documentation

Mira tracks your codebase and flags documentation that needs attention:

- **Gap detection** - Finds undocumented MCP tools, public APIs, and modules
- **Staleness tracking** - Flags docs when source code changes
- **Claude Code writes** - Get task details, read source, write docs directly

```
documentation(action="list")               # See what needs documentation
documentation(action="get", task_id=42)    # Get task details + guidelines
documentation(action="complete", task_id=42)  # Mark done after writing
documentation(action="skip", task_id=42)   # Skip if not needed
```

### Goal & Milestone Tracking

```
goal(action="create", title="v2.0 Release", description="Ship new features")
goal(action="add_milestone", goal_id="1", milestone_title="Complete API redesign", weight=5)
goal(action="complete_milestone", milestone_id="1")
goal(action="list")  # Shows weighted progress
```

Goals and milestones persist across sessions - pick up where you left off.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
                                  |
                                  +--->  Google (embeddings)
                                  +--->  DeepSeek (intelligence)
```

All data stored locally in `~/.mira/` (`mira.db` for memories/sessions, `mira-code.db` for code index). No cloud storage, no external databases.

## Troubleshooting

### "No LLM API keys configured"

Set at least one API key in `~/.mira/.env`. DeepSeek is recommended for intelligence features.

### Semantic search not working

Ensure `GEMINI_API_KEY` is set. Gemini provides the embeddings for semantic search.

### MCP connection issues

1. Check the binary path in `.mcp.json` is absolute
2. Ensure `mira serve` runs without errors: `./target/release/mira serve`
3. Check Claude Code logs for MCP errors

### Memory not persisting

Project context is auto-initialized from Claude Code's working directory. If memories still aren't persisting, verify that Mira is running (`project(action="get")`) and that the working directory matches your project root.

## Documentation

- [Design Philosophy](docs/DESIGN.md) - Architecture decisions and tradeoffs
- [Core Concepts](docs/CONCEPTS.md) - Memory, intelligence, experts explained
- [Configuration](docs/CONFIGURATION.md) - All options and hooks
- [Database Schema](docs/DATABASE.md) - Tables and relationships

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Support

- [Report Issues](https://github.com/ConaryLabs/Mira/issues)
- [Discussions](https://github.com/ConaryLabs/Mira/discussions)

## License

Apache-2.0
