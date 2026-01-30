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

## [0.3.7] - 2026-01-30

### Added
- **Auto-download wrapper** - Plugin marketplace installs now auto-download the `mira` binary on first launch via `plugin/bin/mira-wrapper`. No manual binary installation needed — the wrapper detects platform, downloads from GitHub Releases, and caches to `~/.mira/bin/mira` with version pinning and atomic installs.
- **Plugin manifest component refs** - `plugin.json` now declares `hooks`, `mcpServers`, `skills`, and `category` fields for explicit component discovery.

### Fixed
- **Marketplace installs broken** - `claude plugin install mira@mira` downloaded plugin files but not the `mira` binary, causing all hooks and MCP server to fail. The wrapper script resolves this completely.
- **Plugin configs not shipping** - `plugin/hooks/hooks.json` and `plugin/.mcp.json` were gitignored, so marketplace installs got no hook or MCP config. Now tracked with portable paths.
- **Hardcoded dev paths in plugin configs** - Plugin hook commands and MCP server pointed to `/home/peter/...` instead of bare `mira`. Fixed to use `${CLAUDE_PLUGIN_ROOT}/bin/mira-wrapper`.
- **Dead `MIRA_DB` env var** - Removed unused environment variable from plugin `.mcp.json` (db path is always `~/.mira/mira.db`, resolved internally).
- **PostToolUse hook too broad** - Matcher was empty (fired on every tool call). Now scoped to `Write|Edit|NotebookEdit` across all installation paths.
- **Missing Stop hook in install.sh** - Session cleanup hook was only configured via plugin install, not via the installer script or manual setup docs.
- **Timeout inconsistencies** - Aligned all hook timeouts across plugin, installer, README, and CONFIGURATION docs (SessionStart 10s, PostToolUse/UserPrompt/Stop 5s, PreCompact 30s).

## [0.3.6] - 2026-01-30

### Changed
- **Dependency cleanup** - Removed 9 unused dependencies: glob, libc, md5, similar, pdf-extract, pulldown-cmark, zerocopy, tree-sitter-javascript, dotenv (dev). Reduces build time and binary size.
- **Dead code removal** - Removed unused `EmbeddingModelCheck` enum and `CallInsert` struct.
- **Legacy wording cleanup** - Replaced "legacy data" references with "pre-branch-tracking data" across codebase.
- **CLAUDE.md restructured** - Split monolithic 547-line file into modular layout: `.claude/rules/` (always-loaded guidance) and `.claude/skills/` (on-demand reference). Reduces always-loaded context by 59%.
- **Documentation overhaul** - Added docs for 16 MCP tools, 4 public API types, and 33 modules. Updated CONTRIBUTING.md project structure and documented dual-database architecture.

### Fixed
- **Stale proactive suggestions** - Pre-generated suggestions now expire after 4 hours instead of persisting indefinitely.
- **Stale documentation interventions** - Impact analysis results older than 2 hours are no longer surfaced.
- **Ghost file predictions** - File predictions (NextFile, RelatedFiles) for files that no longer exist on disk are now filtered out before surfacing.
- **Ghost documentation interventions** - Stale-doc and missing-doc interventions now verify files exist on disk before surfacing.

## [0.3.5] - 2026-01-29

### Added
- **Code index sharding** - Code intelligence tables (`code_symbols`, `call_graph`, `imports`, `codebase_modules`, `pending_embeddings`, `vec_code`, `code_fts`) moved to a separate `mira-code.db` database with its own connection pool. Eliminates write contention between indexing and normal tool calls.
- **`PRAGMA busy_timeout=5000`** - SQLite now retries for up to 5 seconds on write contention instead of failing immediately with `SQLITE_BUSY`.
- **`interact_with_retry()`** - New retry wrapper with exponential backoff (3 attempts: 100ms/500ms/2s) for critical database writes.
- **Automatic migration** - On first run, existing code tables are detected in the main database and a fresh `mira-code.db` is created alongside it. Old tables are dropped after successful migration.
- **`code_pool()` on ToolContext** - Tool handlers can now access the code database directly via a dedicated pool.
- **MCP client manager** - Experts can now access MCP tools from the host environment, enabling codebase-aware consultations with real tool access.
- **Secret detection in `remember` tool** - Blocks storage of API keys, tokens, passwords, and other sensitive patterns in memory facts.
- **MCP spawn logging** - Server startup logs now include the full command and arguments for debugging.

### Changed
- **`log_tool_call` is now fire-and-forget** - Tool history logging no longer blocks tool responses. Uses `tokio::spawn` instead of awaiting the write.
- **Background workers use dual pools** - Slow lane workers (summaries, code health, documentation) route reads/writes to the appropriate database pool.
- **Cross-DB JOINs eliminated** - Queries that previously joined code and main tables now use two-step application-level lookups.
- **Consolidated utilities** - Simplified parsers, reduced function complexity across 37 files.

### Removed
- **`check_capability` tool** - Removed along with the background capabilities scanner. Capabilities are now inferred from code intelligence.
- **Unused `with_http_client` on DeepSeekClient** - Dead code cleanup.

### Fixed
- **SQLite concurrent write failures** - All write contention issues resolved through busy_timeout, retry logic, and database sharding.
- **Clippy warnings in memory.rs** - Resolved all warnings with added test coverage (100 lines of new tests).

## [0.3.4] - 2026-01-28

### Added
- **Documentation interventions** - Stale and missing docs now surface as proactive insights in session start, using `[~]` for stale and `[+]` for missing.
- **LLM-based change impact analysis** - Background worker analyzes stale docs to classify changes as "significant" (API changes, new functions) or "minor" (internal refactors). Only significant changes are surfaced as interventions.
- **Auto-configure hooks in installer** - `install.sh` now sets up PostToolUse and UserPromptSubmit hooks automatically in `~/.claude/settings.json`.
- **New database columns** - `documentation_inventory` gains `change_impact`, `change_summary`, and `impact_analyzed_at` for tracking analysis results.

### Changed
- **Documentation workflow simplified** - Removed expert system for doc generation. Claude Code now writes docs directly using `documentation(action="get")` to get task details and `documentation(action="complete")` to mark done.

### Removed
- **`DocumentationWriter` expert role** - No longer needed since Claude Code handles doc writing.
- `documentation(action="write")` - Replaced by `get` + `complete` workflow where Claude Code writes docs directly.

## [0.3.3] - 2026-01-28

### Added
- **Background proactive suggestion system**
  - New `proactive_suggestions` table for pre-generated LLM hints
  - Pattern mining runs every 3rd slow lane cycle (SQL only, fast)
  - LLM enhancement runs every 10th cycle (contextual suggestions)
  - Hybrid lookup in user_prompt hook: pre-generated first, fallback to templates
- **One-liner install script** - `curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash` auto-detects OS/arch, downloads the binary, and installs the Claude Code plugin.
- **CLAUDE.md template in installer** - Post-install now guides users to set up project instructions.
- **Migration for existing pondering patterns** - Automatically prefixes old patterns with `insight_`.

### Changed
- **Removed legacy `Database` struct** - All code now uses `DatabasePool` with `pool.interact()` pattern. Removed 10 files of legacy implementation.
- **Reduced code duplication** - Added `NodeExt` trait for tree-sitter, `SymbolBuilder` for fluent construction, `ResultExt::str_err()` helper. Applied across all 4 language parsers.
- **Removed deprecated `BackgroundWorker`** - ~160 lines of dead code cleaned up.

### Fixed
- **Behavior tracking sequence_position bug** - Events were always logged with position 1, breaking pattern mining. `BehaviorTracker::for_session()` now loads current max position from DB.
- **Proactive pattern type collision** - Pondering insights and mined patterns both used `tool_chain` type with incompatible data formats. Pondering now uses `insight_*` prefix (e.g., `insight_tool_chain`).
- **Silent pattern deserialization failures** - Added logging when patterns fail to deserialize, making debugging easier.
- **Installer API key setup** - Uses `PASTE_YOUR_*_KEY_HERE` placeholders with direct links to get keys.

## [0.3.2] - 2026-01-28

### Added
- **Session lifecycle management** - Sessions now properly close when Claude Code exits. Stop hook marks them as "completed".
- **LLM-powered session summaries** - Automatically generated for sessions with 3+ tool calls, focusing on actual user work (code written, bugs fixed) rather than internal housekeeping.
- **Background stale session cleanup** - Sessions inactive for 30+ minutes auto-close with summaries.
- **GitHub Releases with pre-built binaries** - Release workflow triggered by version tags, building for Linux x86_64, macOS Intel, macOS Apple Silicon, and Windows x86_64.
- **Windows support** - Added `x86_64-pc-windows-msvc` target with `.zip` packaging.

### Fixed
- Sessions no longer stay "active" forever.
- `session_history` now shows meaningful session data with summaries.
- macOS CI runner updated from retired `macos-13` to `macos-15-intel`.

## [0.3.1] - 2026-01-28

### Added
- **Plugin marketplace distribution** - Install via `claude plugin install ConaryLabs/Mira`.
- **Auto-initialize project** - Mira detects Claude Code's working directory and initializes project context automatically. No manual `project(action="start")` call needed.
- **Demo recording** - Added `demo.gif` to README, `scripts/demo.sh` for recording, and `--quiet` flag for `index` command.

### Changed
- Updated installation docs to recommend marketplace installation.

## [0.3.0] - 2026-01-28

### Added
- **GitHub Actions CI pipeline** - Test, clippy, format check, and release builds for Linux and macOS on every push.
- **CHANGELOG.md** - Version tracking with Keep a Changelog format.
- **CONTRIBUTING.md** - Development guidelines for contributors.
- **Issue templates** - Bug report and feature request templates.
- **CI status badge** in README.

### Changed
- Cleaned up `.env.example` (removed deprecated GLM references).
- Comprehensive documentation overhaul - updated README, CONCEPTS, DATABASE, DESIGN, and CONFIGURATION docs with expert review feedback.
- Code formatted with `cargo fmt` for consistency.

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
