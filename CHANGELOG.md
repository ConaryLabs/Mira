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
- **Tool consolidation** - 40+ MCP tools reduced to 11 action-based tools
- **Goal/milestone tracking** - Replaced tasks with persistent goals

### Era 6: MCP Transformation (December 2025)
Pivoted from web chat to Claude Code integration.

- **MCP-only architecture** - Removed REST/WebSocket, pure MCP server
- **Major simplification** - Dropped Qdrant for rusqlite + sqlite-vec
- **HTTP/SSE transport** - Remote access via streaming
- **Daemon consolidation** - Merged mira, mira-chat, and daemon into single binary
- **Code intelligence hooks** - PreToolUse context injection
- **LLM migration** - GPT 5.1 to DeepSeek Reasoner

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

## [0.6.5] - 2026-02-08

### Fixed
- **Asymmetric co-change gap mining** -- `mine_co_change_gaps` only checked A-without-B direction, missing half of all co-change patterns. Now checks both directions.
- **Missing project_id filter** -- `gap_outcomes` CTE in co-change mining joined `diff_outcomes` without a `project_id` filter, inconsistent with other CTEs. Added `WHERE do2.project_id = ?` for defense-in-depth.
- **Variable shadowing in impact analysis** -- `compute_historical_risk` destructured `ref files` shadowed the function parameter `files`, making future changes fragile. Renamed to `pattern_files`.
- **Silent session summary failure** -- `build_session_summary` used `.ok()?` which silently returned `None` on DB errors. Now logs `tracing::warn!` before returning.
- **Inline embedding latency** -- `remember` tool blocked 200-500ms on inline OpenAI embedding call. Removed in favor of background fast lane worker which already handles un-embedded facts.
- **Missing input validation** -- Goal/milestone IDs now validated as positive integers. Empty/whitespace-only memory content now rejected with clear error message.
- **Opaque routing errors** -- Internal routing error messages in `session(action=tasks)` and `code(action=diff)` now include the action name for better bug reports.

### Changed
- **Prepared statement optimization** -- Co-change gap mining now prepares the SQL statement once before the loop instead of re-parsing on each of up to 50 file pairs.
- **Dead code removal** -- Removed unused `PartialOrd, Ord` derives from `TaskPriority` enum and fixed misleading "higher priority runs first" comment.
- **CI concurrency control** -- Added `concurrency` group to `ci.yml` to cancel stale PR runs, saving runner minutes.

## [0.6.4] - 2026-02-08

### Fixed
- **Entity upsert no-op bug** -- `upsert_entity_sync` ON CONFLICT clause used unqualified `COALESCE(display_name, ?)` which always resolved to the existing row's value, making updates a no-op. Now correctly uses `COALESCE(memory_entities.display_name, excluded.display_name)`.
- **Memory import duplicates** -- `import_confirmed_memory_sync` used plain INSERT, creating duplicate rows when re-importing from CLAUDE.local.md. Now upserts by key.
- **Scheduler panic guard** -- Added `Some(0)` guard in slow lane `should_run()` to prevent `is_multiple_of(0)` panic that would crash the background worker in a tight restart loop.
- **Silent error swallowing** -- Replaced 10 instances of `.ok().flatten()` and silent `Err(_) => return` patterns with `tracing::warn!` logging across `db/team.rs`, `hooks/mod.rs`, and `hooks/stop.rs`. Errors are still gracefully handled but no longer invisible.
- **RecallRow doc comment** -- Removed contradictory duplicate doc comment on the `RecallRow` type alias.
- **Error message consistency** -- Standardized error messages in `tasks.rs` and `recipe.rs` to match the `"X is required for action 'Y'"` convention used everywhere else.
- **`.env.example` clarity** -- Commented out `DEEPSEEK_API_KEY` to match other optional keys and avoid implying it's required.

### Changed
- **Transactional migrations** -- Each schema migration is now wrapped in a SAVEPOINT/RELEASE transaction. Partial migration failures roll back cleanly instead of leaving the database in a broken state.
- **Defense-in-depth for git refs** -- Added explicit `validate_ref()` calls in `briefings.rs` before git operations, consistent with `git/commit.rs` and `git/diff.rs`.
- **Ref length limit** -- `from_ref` and `to_ref` parameters in the diff tool are now capped at 256 characters.
- **HashSet optimization** -- `detect_followup_fixes` in outcome scanner now uses `HashSet` for O(1) file overlap checks instead of O(n) Vec scans.

## [0.6.3] - 2026-02-08

### Added
- **Proactive intelligence delivery** -- Pondering insights now surface automatically via the UserPromptSubmit hook with slot-based limiting, confidence thresholds, and cooldown/dedup tracking.
- **LLM provider circuit breaker** -- Three-state circuit breaker (Closed/Open/HalfOpen) automatically skips failing providers after 3 failures in 5 minutes, with 2-minute cooldown and probe recovery.
- **Migration versioning system** -- `schema_versions` table tracks all 31 migrations by version number, skipping already-applied migrations on startup.
- **Slow lane priority ordering** -- Background tasks now have Critical/Normal/Low priorities. Low-priority tasks are automatically skipped when the previous cycle exceeded 60 seconds.
- **Recipe system** -- Reusable team blueprints for Agent Teams. Built-in `expert-review` recipe with 5 roles (architect, code-reviewer, security, scope-analyst, plan-reviewer).
- **Status line** -- Shell status line integration (`mira statusline`) showing project info, session stats, and unread insights.
- **Insight system overhaul** -- Project-aware pondering with actionable outputs, proper dedup by row ID, and daily-scoped unread counts.
- **Inline milestones** -- Goal list responses now include milestones inline for richer context.

### Fixed
- **LIKE wildcard injection** -- Keyword search now strips `%`, `_`, and `\` from user-supplied terms before LIKE pattern construction.
- **Nested tokio runtime** -- Session hook now uses async like all other hooks instead of creating a second runtime.
- **Permission hook fragility** -- Permission rules now use canonical JSON serialization (sorted keys) and field-level matching instead of depending on serialization order.
- **Memory input validation** -- Added 10KB max length check on memory content to prevent unbounded storage.
- **Silent error swallowing** -- Replaced `.filter_map(|r| r.ok())` with `log_and_discard()` across the codebase, and stopped silently swallowing errors in fire-and-forget operations.
- **Duplicate pondering insights** -- Context injection and per-type caps prevent repeated insights.
- **Cross-project goal leakage** -- Rewrote task/goal queries to UNION ALL for index-friendly scoping; fixed async lock contention and dedup correctness.
- **Revert cluster timespan** -- Uses SQLite epoch seconds correctly.
- **Insight accumulation** -- Stopped insight count from accumulating indefinitely; scoped to daily unread.

### Changed
- **Comprehensive code audit** -- 42 files cleaned up: removed unused code, hardened DB operations, optimized query paths.
- **Expert system removal** -- Removed legacy expert system (tools, db, docs, skills) in favor of recipe-based Agent Teams approach.

### Removed
- **Dead code cleanup** -- Removed `db/chat.rs.backup`, unused `SessionPattern.pattern_type` field, and expert system leftovers.

## [0.6.2] - 2026-02-06

### Fixed
- **Cross-platform path normalization** — `path_to_string()` now normalizes backslashes to forward slashes for Windows compatibility. Added `sanitize_project_path()` and `truncate_at_boundary()` utilities.
- **Install script portability** — Fixed `install.sh` and `test-install.sh` for cross-platform use. Added Windows CI workflow.
- **Resume context scoping** — Resume context is now scoped to the current working directory, preventing stale context from other projects.
- **Crossref punctuation tolerance** — Cross-reference search now handles punctuation in query terms.
- **Comprehensive audit fixes** — 31 fixes across expert system, memory, session, hooks, and tools from Codex review.
- **ARM64 Linux cross-compilation** — Fixed aarch64-linux builds with vendored OpenSSL.

### Changed
- **Test quality overhaul** — Replaced tautological tests with meaningful integration tests covering real tool behavior.

## [0.6.1] - 2026-02-06

### Added
- **Ollama provider** — Local LLM support for background tasks. `mira setup` auto-detects running Ollama instances and available models. Sets `background_provider = "ollama"` in `config.toml`.
- **Setup wizard** (`mira setup`) — Interactive configuration with live API key validation, Ollama auto-detection, and safe `.env` merging. Supports `--yes` for non-interactive/CI use and `--check` for read-only validation.

### Fixed
- **`mira setup --yes` false negative** — No longer reports "No providers configured" when existing API keys are present in `~/.mira/.env`.
- **Setup summary with non-provider keys** — `.env` files containing only non-provider keys (e.g. `MIRA_USER_ID`) no longer trigger "Existing configuration unchanged" message.

## [0.6.0] - 2026-02-06

### Added
- **Team intelligence layer** - Full support for Claude Code Agent Teams. Automatic team detection, file ownership tracking, conflict detection across teammates, session lifecycle management, stale session cleanup, and team-scoped memory distillation. New `team` tool with `status`, `review`, and `distill` actions.
- **Zhipu GLM-4.7 provider** - Added GLM-4.7 as an expert LLM option via the Zhipu coding endpoint (`api.z.ai`). 200K context window, 128K max output. Configure with `ZHIPU_API_KEY` or `expert(action="configure", provider="zhipu")`.
- **Knowledge distillation** - Background system that analyzes accumulated memories and distills cross-cutting patterns into higher-level insights.
- **Subagent hooks** - `SubagentStart` injects relevant memories and context when subagents spawn. `SubagentStop` captures discoveries from completed subagent work.
- **Session resume tracking** - Hooks detect `startup` vs `resume` sessions, track previous session ID, and restore "you were working on" context from session snapshots.
- **Configurable expert guardrails** - Runtime limits for expert agentic loops via environment variables: max turns, timeouts, concurrent experts, tool call limits.

### Changed
- **Embeddings switched from Gemini to OpenAI** - Now uses `text-embedding-3-small` via `OPENAI_API_KEY`. Stale Gemini embeddings are automatically invalidated on provider change.
- **Gemini/Google provider removed** - Cleaned out all Gemini provider code, API client, and configuration. Simplifies the provider system.
- **Clippy clean** - Resolved all 15 warnings: collapsed nested if-statements, removed redundant closures, replaced `unwrap()` on Options with safe alternatives, exported `RecallRow` type alias to eliminate `type_complexity` warnings.
- **Hardened background processing** - 20+ fixes from Codex reviews across expert system, memory, semantic search, and team intelligence.

### Fixed
- **Embedding retry on failure** - `has_embedding` flag now resets on embedding failure, allowing background retry.
- **Recall filters** - `category` and `fact_type` filters in `memory(action="recall")` now work correctly.
- **Read file panic** - Fixed panic on inverted line range in `read_file`.
- **DNS pinning and test portability** - Corrected DNS rebinding key and made test paths portable.
- **Team session deduplication** - Deterministic dedupe merge with single-active-team DB constraint prevents orphaned sessions.

## [0.5.3] - 2026-02-05

### Added
- **Enhanced hook system** - Added `PreToolUse` (context injection before Grep/Glob/Read), `SubagentStart`/`SubagentStop` (subagent context and discovery capture), and `SessionEnd` hooks.
- **New skills** - Added `/mira:diff` (semantic diff analysis), `/mira:experts` (expert consultation), `/mira:insights` (background analysis), and `/mira:remember` (quick memory storage) skills.
- **Task list bridging** - SessionStart hook now captures Claude Code's task list ID for session-task correlation.
- **Session resume detection** - Hooks detect `startup` vs `resume` sessions and track the previous session ID.

### Fixed
- **Recall filters** - `category` and `fact_type` filters in `memory(action="recall")` now work correctly.
- **Clippy warnings** - Resolved needless borrows, collapsible ifs, and let-else patterns in hooks.

## [0.5.2] - 2026-02-04

### Changed
- **UTF-8 safe truncation** — Replaced ~35 raw `&s[..N]` string slices with `truncate_at_boundary()`, preventing panics on multi-byte UTF-8 characters. Added `truncate_at_boundary()` to utils as a zero-allocation safe boundary function.
- **Expert module split** — Split `experts/tools.rs` (959 lines) into `definitions.rs` and `web.rs` for better maintainability.
- **Batch findings writes** — Separated scan computation from DB writes and batch-insert findings instead of one-at-a-time.
- **Clippy cleanup** — Fixed 130 collapsible if-statements, added 13 type aliases (eliminating all `type_complexity` warnings), created params structs for 6 functions (eliminating all `too_many_arguments` warnings), moved dead code to `#[cfg(test)]`.
- **Type system improvements** — `ReviewFindingParams` now owns data, `store_findings` takes `Vec` by value (8 `.clone()` calls removed).
- **Rust-native PATH scan** — Replaced shell-based tool detection with Rust-native PATH scanning.
- **Search reranking** — Cached file metadata during search result reranking instead of re-reading per result.
- **Code formatting** — Applied `cargo fmt` across all crates.
- **Net reduction of ~544 lines** across 110+ files.

### Fixed
- **SQLITE_LOCKED retry** — Added `is_sqlite_contention()` to catch both `SQLITE_BUSY` and `SQLITE_LOCKED` errors, with `run_with_retry()` for tool handlers. Fixes failures in shared-cache in-memory databases under concurrent access.
- **MCP client double-connect race** — Fixed race condition with `Mutex<HashSet>` guard preventing duplicate connections.
- **Latent JSON escaping bug** — Derived `Serialize` on `PatternMatch`, fixing incorrect JSON output.

## [0.5.1] - 2026-02-04

### Added
- **Codex HTTP MCP server support** — Added `--transport http` mode for running Mira as an HTTP-based MCP server, enabling integration with OpenAI Codex and similar HTTP-based MCP clients.

### Changed
- **Tool consolidation (12 → 10)** — Merged `analyze_diff` into `code(action="diff")` and `tasks` into `session(action="tasks")`. Reduces tool count and schema overhead while keeping all functionality accessible.
- **Keyword-rich tool descriptions** — Rewrote all tool descriptions for better discoverability via BM25 Tool Search. Descriptions are now concise but keyword-dense, improving how models find and select the right tool.
- **Sub-agents rule updated** — Sub-agents can now access Mira MCP tools directly (Claude Code v2.1.30+). Context pre-injection is still recommended for efficiency but no longer required.
- **Major codebase refactoring** — Six rounds of Codex-audit cleanups: split 1800-line `responses.rs` into focused modules, added `db!` test macro, extracted shared agentic loop and query core, deduplicated LLM logging. Net reduction of ~4,500 lines across the codebase.
- **Documentation-code alignment** — Fixed 15 mismatches between documentation and actual code behavior identified by Codex review.

## [0.5.0] - 2026-02-03

### Added
- **MCP Sampling** — Zero-key expert consultation via host client. Experts now use the MCP sampling protocol to consult through the host LLM, eliminating the need for `DEEPSEEK_API_KEY` for expert consultations.
- **MCP Elicitation** — Interactive API key setup flow. On first run, Mira walks users through configuring API keys via the MCP elicitation protocol instead of requiring manual `.env` file editing.
- **MCP Tasks** — Async long-running operations (SEP-1686). Tools like `index(action="project")` and `index(action="health")` now run in the background with progress tracking via `tasks(action="list|get|cancel")`.
- **MCP outputSchema** — Structured JSON responses for all 11 tools. Every tool now returns typed, parseable JSON instead of free-form text, enabling programmatic consumption of results.
- **Change Intelligence (Goal 103)** — Outcome tracking for commits, pattern mining across change history, and predictive risk scoring. Tracks whether changes led to follow-up fixes and surfaces risky patterns.
- **Entity Layer (Goal 104)** — Lightweight entity extraction from memory facts. Automatically identifies projects, technologies, people, and concepts to boost recall relevance.
- **Context-aware convention injection (Goal 102)** — Automatically injects project conventions (naming, patterns, architecture) into relevant tool responses.
- **Enhanced code intelligence** — Dependency graph analysis (`code(action="dependencies")`), architectural pattern detection (`code(action="patterns")`), and per-module tech debt scoring (`code(action="tech_debt")`).
- **Unified insights digest** — `session(action="insights")` merges pondering, proactive suggestions, and documentation gaps into a single queryable surface.
- **Auto-enqueue long-running tools** — Health scans and full project indexing automatically enqueue as background tasks. Manual `index(action="health")` triggers a full code health scan.

### Changed
- **Tool consolidation** — Reduced from ~20 tools to 11 action-based unified interfaces. `memory` (remember/recall/forget), `code` (search/symbols/callers/callees/dependencies/patterns/tech_debt), `session` (history/recap/usage/insights), `expert` (consult/configure), `finding` (list/get/review/stats/patterns/extract), and `index` (project/file/status/compact/summarize/health). Breaking change for anyone scripting against old tool names.
- **Spring cleaning** — Deduplicated LLM, database, and tool layers. Removed 356 lines of boilerplate across shared request/response handling.

### Fixed
- **Structured output content split** — Tools now correctly return both `content` (text for display) and `structuredContent` (typed JSON via outputSchema) in MCP responses.
- **Compact sqlite-vec storage** — `index(action="compact")` now VACUUMs vec tables to reclaim space from deleted embeddings.

## [0.4.1] - 2026-01-31

### Added
- **Heuristic fallbacks for LLM-disabled mode** - `analyze_diff`, `summarize_codebase`, and pondering/insights all work without LLM providers. Diff analysis uses regex-based function detection and security keyword scanning. Module summaries use file count, language distribution, and symbol metadata. Pondering generates insights from tool usage stats, friction detection, and focus area analysis. All results tagged with `[heuristic]` and upgradeable when an LLM becomes available.
- **Nucleo fuzzy search** - New fuzzy search engine (via `nucleo-matcher`) for memory and code recall when embeddings are unavailable. Provides fast, typo-tolerant matching as a fallback for semantic search.
- **`MIRA_DISABLE_LLM` environment variable** - Explicitly disable all LLM calls to force heuristic fallbacks across the board.

### Changed
- **Code chunks decoupled from embeddings** - Code search indexing no longer requires embeddings. Chunks are stored and searchable via keyword/fuzzy search even without `OPENAI_API_KEY`.
- **Keyword search stays fresh without embeddings** - FTS results no longer go stale when the embedding pipeline is unavailable.
- **Expert consultation error improved** - Clear error message with setup instructions when no LLM provider is configured, instead of a generic failure.
- **Documentation updated** - All tool docs, CONFIGURATION, DESIGN, README, and background module docs updated to reflect graceful degradation behavior.

### Fixed
- **Fuzzy search hardening** - Fixed TOCTOU race condition in cache invalidation, memory bloat from unbounded caches, and score normalization producing out-of-range values.

## [0.4.0] - 2026-01-30

### Added
- **Iterative council architecture** - Expert consultations now use a multi-round council pipeline with a coordinator that synthesizes findings, identifies conflicts, and runs delta rounds for resolution. Replaces the previous parallel-only and debate pipelines.
- **Expert stakes framing** - Expert prompts now include stakes context, accountability rules, and self-check requirements for higher quality output.
- **Non-semantic search overhaul** - Keyword search now uses AND-first query logic, tree-guided scope filtering, and a code-aware tokenizer for significantly better precision on identifier and symbol searches.
- **Smart CLAUDE.local.md export** - Memory export is now hotness-ranked and budget-aware (stays under 500 lines). Automatically exports on session close via the Stop hook.
- **CI install tests** - New workflow and Podman-based multi-distro script for testing the install process.

### Fixed
- **Council reliability** - Fixed race condition in expert task completion, added per-expert timeouts, iteration limits, and graceful fallbacks to parallel mode on council failure.
- **OOM in expert consultations** - Split DeepSeek chat/reasoner clients so reasoning models don't accumulate unbounded `reasoning_content`. Capped debate output and stripped `$schema` from tool definitions to reduce token usage.
- **Plugin marketplace category** - Moved `category` field from plugin manifest to marketplace config where it belongs. Expanded keywords for better discoverability.
- **Clippy warnings** - Fixed `let_unit_value`, `strip_prefix`, `new_ret_no_self`, and `items_after_test_module` warnings.
- **Code formatting** - Applied `cargo fmt` across all crates.

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
- GLM/ZhipuAI provider (simplified to DeepSeek + OpenAI only)
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
