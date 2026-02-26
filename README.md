# Mira

[![CI](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml/badge.svg)](https://github.com/ConaryLabs/Mira/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ConaryLabs/Mira)](https://github.com/ConaryLabs/Mira/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**A local MCP server that gives Claude Code persistent context and code intelligence.**

Mira runs alongside Claude Code over stdio. It tracks sessions, indexes your
codebase with tree-sitter, and injects relevant context on every prompt via
lifecycle hooks. Decisions, preferences, and code structure persist in SQLite
across sessions. No cloud services, no accounts -- one binary, two database files.

## Install

**Claude Code:**
```bash
claude plugin install mira
mira setup  # optional: configure API keys for semantic search
```

**Gemini CLI:**
```bash
gemini mcp add mira --command mira-wrapper --args serve
gemini skills link /path/to/mira/plugin/skills
```

Start a new session. Mira begins injecting context automatically via lifecycle hooks.

See [docs/INSTALLATION.md](docs/INSTALLATION.md) for alternative methods (cargo install, manual binary, build from source).

## What Mira Does

- **Session continuity.** Decisions, preferences, and context persist across sessions and surface automatically on every prompt. Stored facts are evidence-based -- they earn trust through repeated cross-session use, not blind storage.
- **Semantic code search.** "Where do we handle auth?" finds `verify_credentials` in `middleware.rs`, even when the word "auth" never appears. Falls back to keyword search without API keys.
- **Background analysis.** Indexes your codebase with tree-sitter, detects unused functions, doc gaps, and error patterns without being asked. Learns how errors were fixed and surfaces solutions in future sessions.
- **Change intelligence.** Diff analysis with call graph impact tracing and cross-session change tracking.
- **Agent team coordination.** File ownership tracking and conflict detection across concurrent agents. Built-in recipes for expert review, QA hardening, and safe refactoring.
- **Goal tracking.** Multi-session objectives with weighted milestones and automatic progress updates.

## Design Principles

- **Local-first.** Two SQLite databases in `~/.mira/`. No cloud services, no accounts, no external databases required.
- **Evidence-based.** Stored facts start as candidates and are promoted through cross-session use. What surfaces in your session is traceable, not just whatever was written last.
- **Zero-config defaults.** Context persistence, code intelligence, goal tracking, and background analysis all work without API keys. Add OpenAI embeddings for semantic search when you want it.
- **Honest tooling.** Context injection is conservative -- tight relevance thresholds, cross-prompt deduplication, suppression of signals that aren't being used. Mira tells Claude what it actually knows.

## How It Works

```
Claude Code  <--MCP (stdio)-->  Mira  <-->  SQLite + sqlite-vec
    |                             |
    +--lifecycle hooks            +--->  OpenAI (embeddings, optional)
```

Mira runs as a local process spawned by Claude Code over stdio. Two databases: one for sessions, goals, and memories; one for the code index. Lifecycle hooks capture context at key moments (session start, prompt submit, tool use, compaction, stop) and inject relevant information back automatically.

## Quick Start

After installing, Mira's slash commands are available natively in Claude Code and Gemini CLI:

```
/mira:recap           -- session context, preferences, and active goals
/mira:search <query>  -- semantic code search
/mira:goals           -- manage cross-session goals and milestones
/mira:diff            -- semantic analysis of recent changes
/mira:insights        -- surface background analysis
/mira:help            -- full command list
```

## Capabilities With and Without API Keys

| Capability | Without API Keys | With OpenAI Key |
|-----------|-----------------|-----------------|
| Memory and recall | Keyword/fuzzy search | Semantic search |
| Code search | FTS5 + fuzzy matching | Hybrid semantic + keyword |
| Code intelligence | Tree-sitter symbols, call graph | Same |
| Background analysis | Heuristic pattern detection | Same |
| Goal tracking | Full | Full |
| Agent coordination | Full | Full |

OpenAI embeddings use text-embedding-3-small (~$1/month for typical usage).

## Documentation

- [Installation](docs/INSTALLATION.md) -- All install methods, CLI reference, troubleshooting
- [Design Philosophy](docs/DESIGN.md) -- Architecture decisions and tradeoffs
- [Core Concepts](docs/CONCEPTS.md) -- Memory, intelligence, sessions explained
- [Configuration](docs/CONFIGURATION.md) -- Environment variables, hooks, providers
- [Database](docs/DATABASE.md) -- Schema and storage details
- [Tool Reference](docs/tools/) -- Per-tool documentation
- [Changelog](CHANGELOG.md) -- Version history

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Support

- [Report Issues](https://github.com/ConaryLabs/Mira/issues)
- [Discussions](https://github.com/ConaryLabs/Mira/discussions)

## License

Apache-2.0
