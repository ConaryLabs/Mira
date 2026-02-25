# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**The intelligence layer that makes Claude Code dangerous.**

Claude Code is powerful but amnesiac. Every session starts from scratch — architecture decisions gone, codebase reduced to what it can grep. You spend the first ten minutes re-explaining things it knew yesterday.

Mira eliminates that. It's a local Rust MCP server that gives Claude Code persistent memory, deep code understanding, background analysis, and continuous learning. Runs on your machine, stored in SQLite, 13 hooks that make it all automatic.

## The Short Version

```bash
claude plugin install mira
mira setup  # optional: configure API keys for enhanced features
```

That's it. Mira auto-configures itself, starts injecting context on every prompt, and indexes your codebase in the background.

## What Changes

### Before Mira
- Every session starts cold. Claude doesn't know your preferences, your patterns, your past decisions.
- Code search is grep. "Find the authentication logic" returns nothing if nobody wrote the word "authentication."
- Agent teams work in isolation. No shared context, no conflict detection.
- You are the memory. Every conversation requires re-establishing context.

### With Mira
- **Sessions have continuity.** Decisions, preferences, and context persist and surface automatically on every prompt.
- **Search works by meaning.** "Where do we handle auth?" finds `verify_credentials` in `middleware.rs`. Hybrid semantic + keyword search with call graph traversal.
- **The codebase is always understood.** Background workers track code health, score tech debt, detect doc gaps, and surface insights — without you asking.
- **Changes are analyzed, not just diffed.** Semantic classification, call graph impact tracing, risk scoring based on historical churn patterns.
- **Agent teams share a brain.** File ownership tracking, conflict detection, and built-in recipes for expert review, QA hardening, and safe refactoring.
- **Knowledge compounds.** Memories gain confidence through repeated use and get distilled into higher-level insights over time.
- **Cross-project knowledge.** Mira surfaces relevant solutions from your other projects when applicable.
- **Token-efficient by design.** Hooks inject only what matters: tight semantic thresholds, cross-prompt dedup, type-aware subagent budgets, file-read caching, and symbol hints for large files. Context that isn't used gets tracked and suppressed. Run `/mira:efficiency` to see injection hit rates.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
    |                             |
    +--13 lifecycle hooks         +--->  OpenAI (embeddings, optional)
    +--MCP Sampling (host LLM)   +--->  DeepSeek / Ollama (background tasks, optional)
```

Everything runs locally. Two SQLite databases in `~/.mira/`: one for memories, sessions, and goals; one for the code index. No cloud, no accounts, no external databases.

**No API keys required.** Memory, code intelligence, goal tracking, and documentation detection all work out of the box — search falls back to keyword/fuzzy matching, analysis uses heuristic parsers. OpenAI for semantic search and DeepSeek/Ollama for background intelligence are there when you want them.

## Installation

### Quick Install (Recommended)

```bash
claude plugin install mira
```

Then optionally configure providers:
```bash
mira setup          # interactive wizard with live validation + Ollama auto-detection
mira setup --yes    # non-interactive (CI/scripted installs)
mira setup --check  # read-only validation
```

To verify: start a new Claude Code session in any project. You should see "Mira: Loading session context..." in the status bar.

### Alternative: Script Install

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

Detects your OS, downloads the binary, installs the Claude Code plugin (auto-configures all hooks and skills), and creates `~/.mira/`.

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

Binary lands at `target/release/mira`. Add to `.mcp.json`:

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

Plugin install handles this automatically. For MCP-only installs, add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 8}]}],
    "PermissionRequest": [{"hooks": [{"type": "command", "command": "mira hook permission", "timeout": 3}]}],
    "PreToolUse": [{"matcher": "Grep|Glob|Read", "hooks": [{"type": "command", "command": "mira hook pre-tool", "timeout": 3}]}],
    "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit|Bash", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5}]}],
    "PreCompact": [{"matcher": "*", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30, "async": true}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "mira hook stop", "timeout": 8}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "mira hook session-end", "timeout": 15}]}],
    "SubagentStart": [{"hooks": [{"type": "command", "command": "mira hook subagent-start", "timeout": 3}]}],
    "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3, "async": true}]}],
    "PostToolUseFailure": [{"hooks": [{"type": "command", "command": "mira hook post-tool-failure", "timeout": 5, "async": true}]}],
    "TaskCompleted": [{"hooks": [{"type": "command", "command": "mira hook task-completed", "timeout": 5}]}],
    "TeammateIdle": [{"hooks": [{"type": "command", "command": "mira hook teammate-idle", "timeout": 5}]}]
  }
}
```

### Plugin vs MCP Server

The **plugin** (quick install) is the full experience — hooks and skills auto-configured, context injected on every prompt.

The **MCP server** (cargo install / build from source) gives you the core tools. Add hooks manually for the proactive stuff.

### Add Mira Instructions to Your Project

See **[docs/CLAUDE_TEMPLATE.md](docs/CLAUDE_TEMPLATE.md)** for a recommended `CLAUDE.md` layout that teaches Claude Code how to use Mira's tools. The structure:

- `CLAUDE.md` — Core identity, anti-patterns, build commands (always loaded)
- `.claude/rules/` — Tool selection, memory, tasks (always loaded)

## Slash Commands

| Command | What it does |
|---------|-------------|
| `/mira:recap` | Session context, preferences, and active goals |
| `/mira:goals` | List and manage cross-session goals |
| `/mira:search <query>` | Semantic code search |
| `/mira:remember <text>` | Quick memory storage |
| `/mira:recall [query]` | Browse or search stored memories |
| `/mira:diff` | Semantic analysis of recent changes |
| `/mira:insights` | Surface background analysis |
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end review, implementation, and QA |
| `/mira:qa-hardening` | Production readiness review |
| `/mira:refactor` | Safe code restructuring with validation |
| `/mira:help` | Show all available Mira commands and tools |
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
| Error pattern learning | Remembers how errors were fixed across sessions — Claude gets the solution faster next time | Same |
| Memory poisoning defense | Prompt injection attempts in memory writes are detected and flagged | Same |

OpenAI embeddings use text-embedding-3-small, which typically costs less than $1/month for normal development usage.

## Troubleshooting

### Semantic search not working

Make sure `OPENAI_API_KEY` is set in `~/.mira/.env`. Without it, search falls back to keyword and fuzzy matching.

### MCP connection issues

1. Check the binary path in `.mcp.json` is absolute
2. Run `mira serve` directly and confirm it starts without errors
3. Check Claude Code logs for MCP errors

### Memory not persisting

Project context is auto-initialized from Claude Code's working directory. Run `project(action="get")` to verify Mira is running and that the working directory matches your project root.

### CLI Commands

```bash
mira setup                # Interactive configuration wizard
mira setup --check        # Validate current configuration
mira index                # Index current project for semantic code search
mira index --no-embed     # Index without embeddings (faster, keyword-only search)
mira debug-session        # Debug project(action="start") output
mira debug-carto          # Debug cartographer module detection
mira config show          # Display current configuration
mira config set <k> <v>   # Update a configuration value
mira statusline           # Formatted status line for Claude Code's status bar (installed automatically)
mira cleanup              # Data retention dry-run (sessions, analytics, behavior)
mira cleanup --execute    # Actually delete accumulated data (add --yes to skip confirmation)
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
