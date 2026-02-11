# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A persistent intelligence layer for Claude Code.**

Claude Code is stateless. Every session starts from zero — your architecture decisions forgotten, your preferences lost, your codebase understood only as far as it can grep. Mira fixes that.

Mira is a Rust MCP server that gives Claude Code long-term memory and deep code understanding. It runs locally, stores everything in SQLite, and integrates through Claude Code's plugin system with 10 hooks that make context injection automatic — relevant memories recalled on every prompt, file changes tracked, session continuity preserved, and subagent context shared.

## What Mira Does

**Remembers across sessions.** Decisions, preferences, architectural context — stored locally and recalled automatically when relevant. Memories are evidence-based: they start as candidates, gain confidence through repeated use, and get promoted over time. Entities (projects, technologies, people) are extracted automatically to boost recall relevance.

**Understands your code semantically.** Not just text search. Mira indexes your codebase with tree-sitter and vector embeddings, enabling search by meaning, call graph traversal, symbol navigation, dependency analysis, and architectural pattern detection. Supports Rust, Python, TypeScript, JavaScript, and Go.

**Learns from your changes.** Change intelligence tracks whether commits lead to follow-up fixes, mines co-change patterns across history, and scores risk for future changes. Over time, Mira learns which parts of your codebase are fragile and which changes tend to cause problems.

**Builds intelligence in the background.** A two-lane background engine (fast lane for embeddings and entities, slow lane for summaries and analysis) continuously generates module summaries, summarizes git changes since your last session, scores tech debt, runs code health scans, and surfaces insights — all without you asking.

**Coordinates agent teams.** Full support for Claude Code Agent Teams — automatic team detection, file ownership tracking, conflict detection across teammates, and shared memory distillation. Built-in recipes (`expert-review`, `full-cycle`) provide ready-made team blueprints with defined roles, tasks, and prompts.

**Distills knowledge over time.** A background system analyzes accumulated memories and surfaces cross-cutting patterns as higher-level insights, so institutional knowledge compounds rather than just accumulates.

**Tracks goals across sessions.** Weighted milestones that persist across conversations, so multi-session work doesn't lose its thread. Goals have priorities, statuses, and progress that auto-updates as milestones complete.

**Works without API keys.** Core features (memory, code intelligence, goals, documentation) work out of the box. Expert consultation is available via Agent Teams recipes. Add an OpenAI key for semantic search (recommended), or DeepSeek/Zhipu/Ollama for background intelligence tasks.

**Detects documentation gaps.** Finds undocumented APIs and modules, flags stale docs when source changes, classifies impact as significant or minor, and provides writing guidelines so Claude can fill the gaps directly.

## Installation

### Quick Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

This detects your OS, downloads the binary, installs the Claude Code plugin (which auto-configures all hooks and skills), and creates the `~/.mira/` config directory.

Then configure providers (optional):
```bash
mira setup
```
The setup wizard walks you through configuring API keys with live validation, auto-detects local Ollama instances and their models, and merges cleanly with any existing configuration. For CI or scripted installs, `mira setup --yes` runs non-interactively (auto-detects Ollama, skips API key prompts). Use `mira setup --check` for read-only validation of your current config.

> **No API keys?** Mira's core features (memory, code intelligence, goal tracking) work without any keys. Search falls back to fuzzy/keyword matching and analysis uses heuristic parsers. Add keys later with `mira setup` for enhanced capabilities: OpenAI for semantic search, DeepSeek/Zhipu/Ollama for background intelligence.

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

### Install via Cargo (MCP Server Only)

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

### Build from Source

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

### Enable Hooks (Manual Installs Only)

The plugin install auto-configures hooks. For MCP-only installs, add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10000}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 5000}]}],
    "PermissionRequest": [{"hooks": [{"type": "command", "command": "mira hook permission", "timeout": 3000}]}],
    "PreToolUse": [{"matcher": "Grep|Glob|Read", "hooks": [{"type": "command", "command": "mira hook pre-tool", "timeout": 2000}]}],
    "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5000, "async": true}]}],
    "PreCompact": [{"matcher": "*", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30000, "async": true}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "mira hook stop", "timeout": 5000}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "mira hook session-end", "timeout": 5000}]}],
    "SubagentStart": [{"hooks": [{"type": "command", "command": "mira hook subagent-start", "timeout": 3000}]}],
    "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3000, "async": true}]}]
  }
}
```

Hooks enable automatic context injection — relevant memories recalled on every prompt, file changes tracked, session continuity across restarts, permission auto-approval, and subagent context awareness.

### Add Mira Instructions to Your Project

See **[docs/CLAUDE_TEMPLATE.md](docs/CLAUDE_TEMPLATE.md)** for a recommended `CLAUDE.md` layout that teaches Claude Code how to use Mira's tools effectively. The modular structure uses:
- `CLAUDE.md` — Core identity, anti-patterns, build commands (always loaded)
- `.claude/rules/` — Tool selection, memory, tasks (always loaded)

### Plugin vs MCP Server

The **plugin** (quick install) provides the full experience — hooks and skills are auto-configured, context is injected automatically on every prompt.

The **MCP server** (cargo install / build from source) provides the core tools. Add hooks manually (see above) for proactive features.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
    |                             |
    +--MCP Sampling (host LLM)    +--->  OpenAI (embeddings)
                                  +--->  DeepSeek / Zhipu / Ollama (background tasks)
```

All data stored locally in `~/.mira/`. No cloud storage, no external databases. Two SQLite databases: `mira.db` for memories, sessions, and goals; `mira-code.db` for the code index. Connection pooling via deadpool-sqlite with async access throughout.

## Slash Commands

| Command | What it does |
|---------|-------------|
| `/mira:recap` | Session context, preferences, and active goals |
| `/mira:goals` | List and manage cross-session goals |
| `/mira:search <query>` | Semantic code search |
| `/mira:remember <text>` | Quick memory storage |
| `/mira:diff` | Semantic analysis of recent changes |
| `/mira:insights` | Surface background analysis |
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end review, implementation, and QA |
| `/mira:qa-hardening` | Production readiness review |
| `/mira:refactor` | Safe code restructuring with validation |

## Troubleshooting

### Semantic search not working

Ensure `OPENAI_API_KEY` is set in `~/.mira/.env`. OpenAI provides the embeddings for semantic search. Without it, search falls back to keyword and fuzzy matching.

### MCP connection issues

1. Check the binary path in `.mcp.json` is absolute
2. Ensure `mira serve` runs without errors: `mira serve`
3. Check Claude Code logs for MCP errors

### Memory not persisting

Project context is auto-initialized from Claude Code's working directory. Verify Mira is running with `project(action="get")` and that the working directory matches your project root.

### Debug commands

```bash
mira debug-session   # Debug project(action="start") output
mira debug-carto     # Debug cartographer module detection
mira setup --check   # Validate current configuration
```

## Documentation

- [Design Philosophy](docs/DESIGN.md) — Architecture decisions and tradeoffs
- [Core Concepts](docs/CONCEPTS.md) — Memory, intelligence, sessions explained
- [Configuration](docs/CONFIGURATION.md) — All options and hooks
- [Database](docs/DATABASE.md) — Schema and storage details
- [Testing](docs/TESTING.md) — Test infrastructure and patterns
- [Tool Reference](docs/tools/) — Per-tool documentation (memory, code, goal, etc.)
- [Module Reference](docs/modules/) — Internal module documentation
- [Changelog](CHANGELOG.md) — Version history

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Support

- [Report Issues](https://github.com/ConaryLabs/Mira/issues)
- [Discussions](https://github.com/ConaryLabs/Mira/discussions)

## License

Apache-2.0
