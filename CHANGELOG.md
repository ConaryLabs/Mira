# Changelog

All notable changes to Mira will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## Development History

Mira evolved through several major eras before adopting semantic versioning. This section captures the journey from 1,200+ commits.

### Era 7: Intelligence Platform (January 2026)
The current architecture - a persistent intelligence layer for Claude Code.

- **Plugin architecture** - Claude Code integration via hooks and skills
- **Expert consultation** - Architect, security, code reviewer personas via DeepSeek Reasoner
- **Evidence-based memory** - Candidate-to-confirmed promotion system
- **Cross-project intelligence** - Opt-in pattern sharing across codebases
- **DatabasePool migration** - Complete async database layer rewrite
- **Tool consolidation** - 40+ MCP tools reduced to ~22 action-based tools
- **Goal/milestone tracking** - Replaced tasks with persistent goals

### Era 6: MCP Transformation (December 2025)
Pivoted from web chat to Claude Code integration.

- **MCP-only architecture** - Removed REST/WebSocket, pure MCP server
- **Major simplification** - Dropped Qdrant for rusqlite + sqlite-vec
- **HTTP/SSE transport** - Remote access via streaming
- **Daemon consolidation** - Merged mira, mira-chat, and daemon into single binary
- **Code intelligence hooks** - PreToolUse context injection
- **LLM migration** - GPT 5.1 to Gemini 3 Pro

### Era 5: Restructuring (November 2025)
Architecture overhaul and codebase cleanup.

- **Monorepo structure** - Split into backend/ and frontend/ subdirectories
- **Schema redesign** - Fresh database architecture with GPT 5.1
- **Budget tracking** - LLM cache and cost management
- **Comprehensive testing** - Fixed all ignored integration tests
- **Git-style diffs** - Unified diff viewing for artifacts

### Era 4: Code Intelligence (October 2025)
Added semantic understanding of codebases.

- **Cross-language dependency tracking** - WebSocket-based analysis
- **Layered context architecture** - Summaries and hierarchical memory
- **Semantic code embedding** - Vector search for code elements
- **User message analysis pipeline** - Intent extraction and embedding
- **ChatOrchestrator** - Dynamic reasoning based on complexity

### Era 3: WebSocket Refactoring (September 2025)
Stabilized the real-time infrastructure.

- **Tool system integration** - File search, image generation, function calling
- **Handler modularization** - 750-line monolith split into 4 focused modules (73% reduction)
- **Memory decay** - Scheduled salience decay for memory management
- **Git operations** - Full WebSocket integration for version control

### Era 2: API Migrations (August 2025)
Navigated rapid API evolution.

- **OpenAI Responses API** - Migrated from deprecated Assistants API
- **Claude experiment** - Brief attempt at Anthropic backend, reverted
- **GPT-5 migration** - Complete rewrite for new API structure
- **Streaming challenges** - Extensive work on real-time response handling

### Era 1: Web Chat Origins (July 2025)
Where it all began - a personal AI assistant with memory.

- **Initial scaffold** - Rust 2024 edition, GPT-4.1, Axum backend
- **Qdrant integration** - Semantic memory with 3072-dimensional embeddings
- **Persona system** - Mood overlays, emotional asides, personality preservation
- **Sprint-based development** - Rapid iteration on core features
- **VPS deployment** - Production hosting on Oregon server
- **Eternal sessions** - Single-user persistent conversation mode

---

## [Unreleased]

## [0.3.2] - 2026-01-28

### Added
- Session lifecycle management: sessions now properly close when Claude Code exits
- LLM-powered session summaries generated automatically for sessions with 3+ tool calls
- Background worker closes stale sessions (inactive 30+ min) with auto-generated summaries
- New database functions: `close_session_sync`, `get_stale_sessions_sync`, `get_sessions_needing_summary_sync`

### Fixed
- Sessions no longer stay "active" forever - stop hook now marks them as "completed"
- `session_history` now shows meaningful session data with summaries

## [0.3.1] - 2026-01-28

### Added
- Plugin marketplace distribution (`claude plugin install ConaryLabs/Mira`)
- Auto-initialize project from Claude Code's working directory

### Changed
- Updated installation docs to recommend marketplace installation

## [0.3.0] - 2026-01-28

### Added
- GitHub Actions CI pipeline (test, clippy, format, build for Linux/macOS)
- CHANGELOG.md for version tracking
- CONTRIBUTING.md with development guidelines
- Issue templates for bug reports and feature requests
- CI status badge in README

### Changed
- Cleaned up .env.example (removed deprecated GLM references)
- Code formatted with cargo fmt for consistency

## [0.2.0] - 2026-01-27

### Added
- Task-type-aware embeddings for better semantic search quality
- Background worker split into fast/slow lanes for better responsiveness
- Proactive interventions from pondering insights
- Goal and milestone tracking across sessions
- Evidence-based memory system (candidate → confirmed promotion)
- Cross-project intelligence sharing (opt-in)
- Expert consultation with codebase access (architect, code_reviewer, security, etc.)
- Automatic documentation gap detection
- LLM usage and cost analytics

### Changed
- Default embedding dimensions changed from 768 to 1536
- Consolidated MCP tools from 40+ to ~22 action-based tools
- Improved semantic code search with CODE_RETRIEVAL_QUERY task type

### Removed
- GLM/ZhipuAI provider (simplified to DeepSeek + Gemini only)
- Legacy Database struct (fully migrated to DatabasePool)

### Fixed
- vec_code table no longer dropped on startup
- Memory embedding backfill now works correctly

## [0.1.0] - 2025-12-31

### Added
- Initial release
- Persistent memory system with semantic search
- Code intelligence (symbols, call graphs, semantic search)
- Background indexing and embedding generation
- Expert consultation via DeepSeek Reasoner
- Session history and context tracking
- MCP server integration with Claude Code
