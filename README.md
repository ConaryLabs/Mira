# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**The intelligence layer that makes Claude Code dangerous.**

Claude Code is powerful but amnesiac. Every session starts from scratch — your architecture decisions evaporated, your codebase reduced to what it can grep, your last three hours of context gone. You spend the first ten minutes of every conversation re-explaining things it knew yesterday.

Mira eliminates that. It's a local Rust MCP server that gives Claude Code persistent memory, deep code understanding, background analysis, and continuous learning — all running on your machine, stored in SQLite, with 13 hooks that make everything automatic.

The result: Claude Code that remembers what you decided last week, understands your codebase by meaning not just text, notices when your docs are stale, predicts which changes are risky, and gets smarter the longer you use it.

## The Short Version

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
mira setup  # optional: configure API keys for enhanced features
```

That's it. Mira auto-configures itself, starts injecting context on every prompt, and begins indexing your codebase in the background.

## What Changes

### Before Mira
- Every session starts cold. Claude doesn't know your preferences, your patterns, your past decisions.
- Code search is grep. "Find the authentication logic" returns nothing if nobody wrote the word "authentication."
- Agent teams work in isolation. No shared context, no conflict detection.
- You are the memory. Every conversation requires re-establishing context.

### With Mira
- **Sessions have continuity.** Decisions, preferences, and architectural context persist and surface automatically when relevant. No manual `/recall` needed — the `UserPromptSubmit` hook injects context on every prompt.
- **Search works by meaning.** "Where do we handle auth?" finds the right code even if it's called `verify_credentials` in a file named `middleware.rs`. Hybrid semantic + keyword search with tree-sitter symbol matching and call graph traversal.
- **The codebase is always understood.** Background workers continuously generate module summaries, track code health, score tech debt, detect documentation gaps, and surface insights — without you asking.
- **Changes are analyzed, not just diffed.** Mira classifies what changed semantically, traces impact through the call graph, scores risk based on historical patterns, and learns which parts of your codebase are fragile.
- **Agent teams share a brain.** Automatic team detection, file ownership tracking, cross-teammate conflict detection, and shared memory distillation. Built-in recipes for expert review, full-cycle development, QA hardening, and safe refactoring.
- **Knowledge compounds.** Memories start as candidates, gain confidence through repeated cross-session use, and get promoted over time. A background distillation system extracts cross-cutting patterns from accumulated knowledge into higher-level insights.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
    |                             |
    +--13 lifecycle hooks         +--->  OpenAI (embeddings, optional)
    +--MCP Sampling (host LLM)   +--->  DeepSeek / Ollama (background tasks, optional)
```

Everything runs locally. Two SQLite databases (`~/.mira/`): one for memories, sessions, and goals; one for the code index. No cloud storage, no external databases, no accounts.

**No API keys required.** Core features — memory, code intelligence, goal tracking, documentation detection — work out of the box. Search falls back to keyword/fuzzy matching, analysis uses heuristic parsers. Add OpenAI for semantic search or DeepSeek/Ollama for background intelligence when you're ready.

## Installation

### Quick Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

Detects your OS, downloads the binary, installs the Claude Code plugin (auto-configures all hooks and skills), and creates `~/.mira/`.

Then optionally configure providers:
```bash
mira setup          # interactive wizard with live validation + Ollama auto-detection
mira setup --yes    # non-interactive (CI/scripted installs)
mira setup --check  # read-only validation
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

If you use Codex CLI, add Mira to `.codex/config.toml`:

```toml
#:schema https://developers.openai.com/codex/config-schema.json
project_doc_fallback_filenames = ["CLAUDE.md"]

[mcp_servers.mira]
command = "mira"
args = ["serve"]
required = true
startup_timeout_sec = 20
tool_timeout_sec = 90
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
    "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3000, "async": true}]}],
    "PostToolUseFailure": [{"hooks": [{"type": "command", "command": "mira hook post-tool-failure", "timeout": 5000, "async": true}]}],
    "TaskCompleted": [{"hooks": [{"type": "command", "command": "mira hook task-completed", "timeout": 5000}]}],
    "TeammateIdle": [{"hooks": [{"type": "command", "command": "mira hook teammate-idle", "timeout": 5000}]}]
  }
}
```

### Add Mira Instructions to Your Project

See **[docs/CLAUDE_TEMPLATE.md](docs/CLAUDE_TEMPLATE.md)** for a recommended `CLAUDE.md` layout that teaches Claude Code how to use Mira's tools effectively. The modular structure uses:
- `CLAUDE.md` — Core identity, anti-patterns, build commands (always loaded)
- `.claude/rules/` — Tool selection, memory, tasks (always loaded)

### Plugin vs MCP Server

The **plugin** (quick install) provides the full experience — hooks and skills are auto-configured, context is injected automatically on every prompt.

The **MCP server** (cargo install / build from source) provides the core tools. Add hooks manually (see above) for proactive features.

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
| `/mira:status` | Quick health check: index, storage, goals |

## Capabilities at a Glance

| Capability | Without API Keys | With API Keys |
|-----------|-----------------|---------------|
| Memory & recall | Keyword/fuzzy search | Semantic search (OpenAI embeddings) |
| Code search | FTS5 + fuzzy matching | Hybrid semantic + keyword |
| Code intelligence | Tree-sitter symbols, call graph | Same |
| Diff analysis | Heuristic pattern detection | LLM-powered semantic classification |
| Module summaries | File counts, symbol names | LLM-generated descriptions |
| Background insights | Tool usage analysis, friction detection | LLM-powered pattern extraction |
| Goal tracking | Full | Full |
| Agent team coordination | Full | Full |

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
