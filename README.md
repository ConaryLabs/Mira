# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A persistent intelligence layer for Claude Code.**

Claude Code is stateless. Every session starts from zero — your architecture decisions forgotten, your preferences lost, your codebase understood only as far as it can grep. Mira fixes that.

Mira is a Rust MCP server that gives Claude Code long-term memory, deep code understanding, and a team of AI expert reviewers. It runs locally, stores everything in SQLite, and integrates through Claude Code's plugin system with hooks that make context injection automatic.

## What Mira Does

**Remembers across sessions.** Decisions, preferences, architectural context — stored locally and recalled automatically when relevant. Memories are evidence-based: they start as candidates, gain confidence through repeated use, and get promoted over time.

**Understands your code semantically.** Not just text search. Mira indexes your codebase with tree-sitter and vector embeddings, enabling search by meaning, call graph traversal, symbol navigation, dependency analysis, and architectural pattern detection. Supports Rust, Python, TypeScript, JavaScript, and Go.

**Builds intelligence in the background.** A background engine continuously generates module summaries, detects capabilities, summarizes git changes since your last session, scores tech debt, and surfaces insights — all without you asking.

**Provides expert second opinions.** On-demand consultation from specialized AI reviewers (architect, security, code reviewer, scope analyst, plan reviewer) that have full access to search your codebase before answering. Powered by DeepSeek Reasoner or Zhipu GLM-4.7, or zero-key via MCP Sampling through the host client.

**Coordinates agent teams.** Automatic team detection when using Claude Code Agent Teams — tracks file ownership, detects conflicts across teammates, and distills team discoveries into shared memory.

**Distills knowledge over time.** A background system analyzes accumulated memories and surfaces cross-cutting patterns as higher-level insights, so institutional knowledge compounds rather than just accumulates.

**Tracks goals across sessions.** Weighted milestones that persist across conversations, so multi-session work doesn't lose its thread.

**Works without API keys.** Core features (memory, code intelligence, goals, documentation) work out of the box. MCP Sampling enables expert consultation without provider keys. Add an OpenAI key for semantic search, or DeepSeek/Zhipu for direct expert access.

**Detects documentation gaps.** Finds undocumented APIs and modules, flags stale docs when source changes, and provides writing guidelines so Claude can fill the gaps directly.

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

> **No API keys?** Mira's core features (memory, code intelligence, goal tracking) work without any keys. Search falls back to fuzzy/keyword matching, analysis uses heuristic parsers, and expert consultation works via MCP Sampling through the host client. Add keys later with `mira setup` for enhanced capabilities: OpenAI for semantic search, DeepSeek or Zhipu for direct expert access with codebase search.

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
    "SessionStart": [{"hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 5}]}],
    "PermissionRequest": [{"hooks": [{"type": "command", "command": "mira hook permission", "timeout": 3}]}],
    "PreToolUse": [{"matcher": "Grep|Glob|Read", "hooks": [{"type": "command", "command": "mira hook pre-tool", "timeout": 2}]}],
    "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5, "async": true}]}],
    "PreCompact": [{"matcher": "*", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30, "async": true}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "mira hook stop", "timeout": 5}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "mira hook session-end", "timeout": 5}]}],
    "SubagentStart": [{"hooks": [{"type": "command", "command": "mira hook subagent-start", "timeout": 3}]}],
    "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3, "async": true}]}]
  }
}
```

Hooks enable automatic context injection — relevant memories recalled on every prompt, file changes tracked, session continuity across restarts, and subagent context awareness.

### Add Mira Instructions to Your Project

See **[docs/CLAUDE_TEMPLATE.md](docs/CLAUDE_TEMPLATE.md)** for a recommended `CLAUDE.md` layout that teaches Claude Code how to use Mira's tools effectively. The modular structure uses:
- `CLAUDE.md` — Core identity, anti-patterns, build commands (always loaded)
- `.claude/rules/` — Tool selection, memory, tasks, experts (always loaded)

### Plugin vs MCP Server

The **plugin** (quick install) provides the full experience — hooks and skills are auto-configured, context is injected automatically on every prompt.

The **MCP server** (cargo install / build from source) provides the core tools. Add hooks manually (see above) for proactive features.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
                                  |
                                  +--->  OpenAI (embeddings)
                                  +--->  DeepSeek Reasoner (experts)
                                  +--->  Zhipu GLM-4.7 (experts)
                                  +--->  Ollama (local background tasks)
```

All data stored locally in `~/.mira/`. No cloud storage, no external databases. Two SQLite databases: `mira.db` for memories, sessions, goals, and expert history; `mira-code.db` for the code index.

## Slash Commands

| Command | What it does |
|---------|-------------|
| `/mira:recap` | Session context, preferences, and active goals |
| `/mira:goals` | List and manage cross-session goals |
| `/mira:search <query>` | Semantic code search |
| `/mira:remember <text>` | Quick memory storage |
| `/mira:experts <question>` | Expert consultation |
| `/mira:diff` | Semantic analysis of recent changes |
| `/mira:insights` | Surface background analysis |

## Troubleshooting

### Semantic search not working

Ensure `OPENAI_API_KEY` is set in `~/.mira/.env`. OpenAI provides the embeddings for semantic search.

### MCP connection issues

1. Check the binary path in `.mcp.json` is absolute
2. Ensure `mira serve` runs without errors: `mira serve`
3. Check Claude Code logs for MCP errors

### Memory not persisting

Project context is auto-initialized from Claude Code's working directory. Verify Mira is running with `project(action="get")` and that the working directory matches your project root.

## Documentation

- [Design Philosophy](docs/DESIGN.md) — Architecture decisions and tradeoffs
- [Core Concepts](docs/CONCEPTS.md) — Memory, intelligence, experts explained
- [Configuration](docs/CONFIGURATION.md) — All options and hooks
- [Database](docs/DATABASE.md) — Schema and storage details
- [Testing](docs/TESTING.md) — Test infrastructure and patterns
- [Tool Reference](docs/tools/) — Per-tool documentation (memory, code, expert, etc.)
- [Module Reference](docs/modules/) — Internal module documentation
- [Changelog](CHANGELOG.md) — Version history

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Support

- [Report Issues](https://github.com/ConaryLabs/Mira/issues)
- [Discussions](https://github.com/ConaryLabs/Mira/discussions)

## License

Apache-2.0
