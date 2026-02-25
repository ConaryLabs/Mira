# Changelog

All notable changes to Mira will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.9.0] - 2026-02-25

### Changed
- **Background LLM permanently removed** -- Deleted LLM factory, removed dead background task variants. All background intelligence (pondering, briefings, summaries, diff analysis) uses heuristics permanently. Only OpenAI embeddings remain as an optional API-key feature.
- **Zero-key mode** -- Mira works fully out of the box without any API keys. Embeddings provide the only optional upgrade (semantic search).

### Fixed
- **Error message sanitization** -- MiraError::Db and callers/callees errors no longer leak rusqlite internals (table names, SQL fragments) to users.
- **Watcher robustness** -- File watcher pending_changes retry count no longer resets on re-queue (preventing infinite retries under continuous modification). Pending changes preserved across panic-restarts via shared Arc.
- **File permissions race** -- persist_api_key now uses atomic mode(0o600) on file creation instead of post-write chmod.
- **Silent error logging** -- Added tracing for 8+ silently swallowed `let _ =` sites in hooks (user_prompt, precompact, session).
- **Chunk start_line accuracy** -- split_large_chunk now calculates correct start_line for each sub-chunk instead of reusing the symbol's start_line.
- **Memory stale-matching precision** -- mark_memories_stale uses parent/basename matching instead of basename-only, preventing over-broad staling for common filenames (mod.rs, lib.rs).
- **SQLite retry robustness** -- retry_with_backoff inspects rusqlite error codes directly instead of brittle string matching.
- **Embeddings batch isolation** -- A single bad batch no longer permanently blocks all pending embeddings.
- **Retention safety** -- days=0 in retention config now skipped with warning instead of silently wiping tables.
- **Supervisor shutdown latency** -- Worker backoff sleep now uses tokio::select with shutdown channel, reducing max shutdown delay from 60s to immediate.
- **AllowedTable enum** -- count_table uses compile-time enum instead of runtime string allowlist.
- **Session test determinism** -- Removed 1-second wall-clock sleep, replaced with SQL backdating.

### Documentation
- Updated all docs to reflect background LLM removal (DESIGN.md, CONCEPTS.md, CONFIGURATION.md, diff.md, index.md, team.md, module docs).
- Fixed PostToolUseFailure hook name typo in DESIGN.md.
- Added bundle action documentation to code.md.
- Fixed insight_id vs row_id naming inconsistency in insights.md.
- Rephrased function-call syntax in error messages and README to natural language.

---

## [0.8.13] - 2026-02-24

### Changed
- **Background tasks are heuristic-only** -- DeepSeek and Ollama are no longer used for background intelligence (module summaries, briefings, pondering, diff analysis). These features now use heuristic fallbacks permanently regardless of API keys configured. Only OpenAI embeddings continue to provide an upgrade path (semantic search).

### Added
- **Token efficiency improvements** -- PreToolUse file-read cache with reread advisory for unchanged files, symbol hints for large files (>200 lines) from code index, batch-aware cooldown with injection summary replay, cross-prompt injection dedup via content hashing, post-compaction context recovery from session snapshots.
- **Type-aware subagent budgets** -- Narrow agents (Explore, code-reviewer) get 800-char context cap vs 2000 for full agents. Goals skipped for narrow subagents.
- **Tighter keyword recall** -- Minimum 2 keywords with AND-join to reduce false memory injections.
- **`/mira:efficiency` skill** -- Dashboard showing active token-saving mechanisms.
- **Injection telemetry** -- Stop hook logs 24h hit/miss rate for injection feedback tracking.

### Fixed
- **Post-compaction recovery** -- Fixed query to extract compaction_context from snapshot JSON blob instead of nonexistent column. Flag consumed only after successful extraction.
- **Symbol hints path matching** -- Reverse LIKE for absolute-to-relative path matching, removed over-broad basename fallback.
- **Session ID sanitization** -- All temp file paths now filter session_id to alphanumeric + hyphens, preventing path traversal.
- **Atomic state writes** -- Read cache and injection dedup state files use temp+rename pattern with 0o600 permissions.
- **Stable content hashing** -- Replaced DefaultHasher with FNV-1a for cross-compilation stability in injection dedup.
- **UTF-8 safe truncation** -- Injection summary and session ID slicing use char-boundary-safe operations.
- **Cooldown arithmetic** -- Use saturating_sub to prevent underflow on clock skew.

---

## [0.8.12] - 2026-02-24

### Fixed
- **Token overhead reduction** -- Cut UserPromptSubmit budget multiplier, raised PreToolUse semantic thresholds, added 2500-char SessionStart cap, and tightened CLAUDE.local.md export (per-type truncation, dedup, line/char caps, newline normalization).
- **Proactive/cross-project context gates** -- Restored quality gates (simple command skip, message length bounds). Both features now default to off and require explicit opt-in via stored preference (`key=proactive:enabled`). Cross-project recall requires a resolved project ID to prevent leaking unrelated memories.

---

## [0.8.11] - 2026-02-24

### Added
- **Context bundling for agent teams** -- New `code(action="bundle")` packages module summaries, symbols, dependencies, and code snippets into a budget-constrained markdown digest for agent spawn prompts. Reduces redundant file re-reading when launching agentic teams. Supports scope (path or concept), configurable budget (default 6000 chars), and three depth levels (overview, standard, deep). Includes semantic search fallback for concept-based scopes.

---

## [0.8.10] - 2026-02-24

### Changed
- **Cost-efficient agent routing** -- Recipe members now include a `model` field. Read-only discovery and analysis agents use Sonnet instead of inheriting the parent Opus model, significantly reducing cost and latency for agentic team workflows. Implementation agents that write code still inherit the parent model.

---

## [0.8.9] - 2026-02-20

### Fixed
- **Installer hardening** -- Both install paths (install.sh and plugin marketplace) now produce identical, validated configurations. Default install directory changed from `/usr/local/bin` to `~/.mira/bin` (no sudo needed). Bundled jq with SHA256 verification for reliable JSON operations without system dependencies. Added checksum verification for mira binary in install.sh (aligned with mira-wrapper). Config function failures now degrade gracefully instead of killing the script.
- **Status line reliability** -- Fixed status line pointing to wrong binary when settings.json was synced between machines. Both install paths now validate the binary exists before yielding to an existing status line config. Plugin wrapper uses POSIX-compatible locking with 60-second stale timeout (replaced non-portable `trap RETURN`).
- **Symlink-safe config writes** -- Atomic writes now resolve symlinks before writing, preserving link targets on both Linux (`readlink -f`) and macOS/BSD (directory-relative fallback).
- **Release URL construction** -- install.sh now correctly uses `v`-prefixed tags in GitHub release URLs, matching actual tag format.
- **POSIX shell compatibility** -- mira-statusline shebang changed to `#!/bin/sh` with `pipefail` removed. mira-wrapper HOME guard added for edge environments.

### Added
- **MCP server fallback** -- install.sh now configures `~/.claude/mcp.json` when plugin install fails, ensuring mira tools are available without the plugin.
- **PATH guidance** -- Post-install output suggests adding `~/.mira/bin` to PATH when not already present.
- **Test coverage** -- New tests for jq bundling, MCP server fallback, and output verification in `test-install.sh`.

---

## [0.8.8] - 2026-02-19

### Added
- **Smarter precompact extraction** -- New context categories (decisions, todos, errors, preferences, file modifications) extracted from conversation transcript before summarization, preserving critical context across compactions.
- **Improved compaction context preservation** -- PreCompact hook now captures richer session state including active goals, modified files, and recent decisions.
- **Improved memory recall quality** -- Parallel semantic + keyword search with score-based ranking. Hook recall paths (pre-tool, subagent, post-tool-failure) now use the same improved hybrid search.

### Changed
- **Codebase structural refactoring** -- Split 7 monolith files (11k+ lines) into focused submodules: `db/memory` (5 files), `hooks/precompact` (3 files), `hooks/session` (3 files), `ipc/client` (5 files), `tools/core/memory` (5 files), `tools/core/project` (4 files), `tools/core/session` (4 files). No behavior changes.
- **Plugin audit fixes** -- Expanded skill trigger phrases across all 13 skills. Fixed `experts` skill (parallel spawning, task ordering, permission guard). Aligned `full-cycle` skill with recipe backend (added Dependency Updates phase, authority declaration, stalled-agent handling). Fleshed out `search` skill with empty-args handling, fallback guidance, and example output. Fixed `help` skill self-reference and hardcoded tool count. Aligned marketplace and plugin.json descriptions. Added LICENSE to plugin directory.

### Fixed
- **Windows UNC path prefix** -- `canonicalize()` on Windows produces `\\?\` prefixed paths that break SQLite and path comparisons. Now stripped automatically.

---

## [0.8.7] - 2026-02-18

### Added
- **Seamless multi-project switching** -- Per-session directory isolation (`~/.mira/sessions/{session_id}/`) ensures concurrent Claude Code sessions on different projects no longer cross-contaminate state. Session ID threaded through all hooks (subagent, pre_tool, stop, session-end) for correct project resolution.
- **45 tests for session MCP tool** -- Full coverage of `session(action="recap/current_session")` with `MockToolContext`, including startup vs resume, project scoping, team detection, and error handling.
- **58 tests across 6 LLM client modules** -- Coverage for DeepSeek, Ollama, Sampling, provider factory, config, and rate limiting. Includes circuit breaker behavior, retry logic, and error handling.

### Fixed
- **Code tool oversized response hardening** -- Truncates search results, symbol lists, and call graph output to prevent context window overflow. Silent DB errors now surface as user-visible messages.
- **`vec_code` dimension mismatch** -- Code index now detects embedding dimension changes and recreates the virtual table instead of silently failing queries.
- **Per-session cleanup ordering** -- Cleanup only runs on session-end (not per-response), and after project resolution completes, preventing wrong-project attachment in concurrent sessions.
- **Hook fast-path JSON format** -- `mira-wrapper` first-install hook response now uses correct `hookSpecificOutput` wrapper.

---

## [0.8.6] - 2026-02-18

### Added
- **3 new agentic team recipes** -- `debug` (locate bug, trace root cause, implement fix, write regression test), `test-gen` (coverage analysis, parallel test writing, quality review), and `pr-review` (diff-scoped correctness, conventions, test coverage, docs check).
- **274 new tests across 23 modules** -- 104 tests for `background/` (diff_analysis heuristics, documentation detection, pondering cooldown/storage, queries with data) and 170 tests for `db/` and core modules (`milestones`, `usage`, `documentation`, `diff_analysis`, `diff_outcomes`, `cartographer`, `search`, `background`, `retention`, `dependencies`, `tech_debt`, `summaries`, `embeddings`, `memory_embeddings`, `crossref`, `parsing`). Covers happy paths, error cases, boundary values, and edge cases.
- **Windows `mira-wrapper.cmd` shim** -- Enables Mira hooks and CLI on Windows `cmd.exe` outside of Git Bash/WSL. (PR #9)
- **Release script pre-flight checks** -- `release.sh` now runs `cargo fmt`, `cargo clippy`, `cargo test`, and verifies a `CHANGELOG.md` entry exists before bumping the version.

### Fixed
- **Indexer embedding fan-out parallelized** -- Sub-batches are now embedded in concurrent groups of 4 instead of sequentially, matching the intended throughput. Capped at 4 concurrent requests to avoid rate-limit spikes on large flushes. `MAX_BATCH_SIZE` and `MAX_CONCURRENT` promoted to `pub(crate)` as single sources of truth.
- **Recipe quality fixes** -- `expert-review` missing `run_in_background=true` caused sequential spawning; task creation now happens before spawning agents in all recipes; `full-cycle` plan-reviewer renamed to project-health to avoid role confusion; stale expert count references corrected.
- **Weak test assertions strengthened** -- `indexer/parsing` tests for Python, TypeScript, and Go now assert specific symbol names (not just `is_ok()`). Two `db/usage` tests for unknown `group_by` values now insert data before querying.

---

## [0.8.5] - 2026-02-17

### Fixed
- **`mira index` CLI opens correct database** -- Standalone `mira index` command now opens `mira-code.db` instead of the main `mira.db`, matching the MCP handler behavior. (PR #4)
- **Ollama context overflow on dense code** -- Lowered `MAX_TEXT_CHARS` from 32768 to 12000 to prevent 400 errors on token-dense inputs like C# with long PascalCase identifiers. (PR #6)
- **Adaptive retry on Ollama context overflow** -- On a 400 response from Ollama embeddings, the retry now halves the truncation limit (12K to 6K chars), recovering from worst-case token density without losing embedding coverage for normal inputs.

### Added
- **Mock server test for Ollama retry path** -- Verifies the retry-with-halved-truncation behavior end-to-end using a local TCP mock that returns 400 then 200.

---

## [0.8.4] - 2026-02-17

### Added
- **Cross-platform IPC transport** — IPC handler is now generic over `AsyncRead + AsyncWrite`, supporting Unix domain sockets and Windows Named Pipes. Windows users get full IPC hook functionality instead of the silent `Backend::Unavailable` fallback.
- **Windows Named Pipe listener** — `run_ipc_listener` on Windows uses `tokio::net::windows::named_pipe` with `ServerOptions`, semaphore-limited concurrency, and correct move semantics via `std::mem::replace`.
- **`IpcStream` abstraction** — Transport-agnostic boxed reader/writer struct replaces platform-specific stream types in `Backend::Ipc`. Removes all `#[cfg]` gates from the `Backend` enum.
- **Duplex-based IPC tests** — 8 transport-agnostic tests using `tokio::io::duplex` run on all platforms. Unix socket integration tests preserved in `#[cfg(unix)]` submodule.
- **Windows `.tar.gz` release asset** — CI now produces both `.zip` and `.tar.gz` for Windows targets.
- **Data surfacing improvements** — Score-ranked hybrid search results, improved recall quality, memory list with richer metadata.
- **Compact status line** — Emoji indicators, dropped verbose labels for cleaner `project(action="start")` output.

### Fixed
- **IPC bounded read (OOM prevention)** — Replaced unbounded `read_line` with `fill_buf`/`consume` loop that rejects requests exceeding 1 MB *before* allocating, preventing OOM from malicious clients.
- **TOCTOU socket race** — `umask(0o177)` set before `bind()` and restored after, so socket is created with owner-only permissions atomically.
- **Socket path fallback** — When `HOME` is unset, falls back to `$XDG_RUNTIME_DIR/mira/` then UID-scoped `/tmp/mira-<uid>/`, preventing socket impersonation on shared systems. Parent directories created before bind.
- **mira-wrapper Git Bash compatibility** — Wrapper now prefers `.tar.gz` (Git Bash has GNU tar but not `unzip`), with `.zip` + PowerShell `Expand-Archive` fallback for older releases.
- **Milestones migration** — `DEFAULT` clause no longer uses non-constant expressions, fixing migration on older SQLite versions.
- **Auto-link temporal fields** — Fixed recap filter ordering and file activity tracking.
- **Dead cleanup category removed** — Chat cleanup no longer references removed category.
- **Data lifecycle robustness** — Suggestion wiring fixes, cleanup output improvements, dead code removal.

### Changed
- **IPC listener no longer Unix-only** — `serve.rs` spawns IPC listener unconditionally; cleanup is platform-conditional.
- **Comprehensive docs audit** — 18 files updated for accuracy across tools, hooks, and module documentation.

---

## [0.8.3] - 2026-02-17

### Added
- **`/mira:recall` skill** — New slash command for browsing and searching stored memories. Without args: lists recent 20 memories grouped by category. With a query: semantic recall. Fills the gap where the flagship feature had no slash command shortcut.
- **`docs/tools/insights.md`** — Documentation for the `insights` MCP tool (previously undocumented after being split from `session`).
- **Code embedding recovery on startup** — If a previous embedding provider switch cleared `vec_code` but crashed before re-queuing, Mira now detects the invariant violation (`vec_code` empty, chunks present, nothing queued) and self-heals automatically.

### Fixed
- **Unescaped LIKE wildcards** — `hooks/pre_tool.rs` and `cartographer/map.rs` now escape `%` and `_` before using filenames/module paths in SQL LIKE clauses, consistent with the rest of the codebase.
- **Double pool open in `pre_tool`** — `handle_edit_write_patterns` was opening a second `DatabasePool` redundantly; now reuses the existing pool.
- **`vec_code` not invalidated on embedding provider change** — Switching embedding providers previously cleared `vec_memory` but left stale `vec_code` embeddings, causing garbage semantic code search results. `vec_code` is now cleared and all chunks re-queued for re-embedding on provider switch. `code_fts` (keyword search) is preserved unchanged.
- **Duplicate re-queue on provider switch** — `INSERT OR IGNORE` had no effect without a UNIQUE constraint on `pending_embeddings`. Fixed with `DELETE WHERE status='pending'` + plain `INSERT` for exact-once queuing.
- **`mode: basic` messaging** — Now explicitly lists what features are disabled and links to `mira setup`, instead of a bare mode label.
- **Silent HOME directory fallback** — All three paths that fall back to CWD when `$HOME` is unset now emit a `tracing::warn!` explaining the risk.

### Changed
- **Zhipu/GLM provider removed** — `Provider::Zhipu`, `llm/zhipu.rs`, and all references removed. LLM providers are now DeepSeek, Ollama, and MCP Sampling only.
- **Docs sync** — Tool count updated from 8 to 9 everywhere (`insights` is now a standalone tool). Hook timeouts in `CONFIGURATION.md` and `README.md` synced to `hooks.json`. Dropped tables removed from `DATABASE.md`. Zhipu references removed from `.env.example`, `CONTRIBUTING.md`, `DATABASE.md`. `docs/tools/session.md` no longer documents `insights`/`dismiss_insight` actions. Three missing hooks added to `plugin/README.md` hooks table.

---

## [0.8.2] - 2026-02-16

### Added
- **/mira:help skill** — Tiered command listing (getting started, daily use, power user) for discoverability.
- **Recovery hints on errors** — ~32 user-facing error messages across goals, session, diff, tasks, documentation, usage, code search, and index tools now include actionable recovery hints.
- **Compact status line on project start** — `project(action="start")` output now includes a one-line Mira status (memories, symbols, goals).
- **Growth-strategist recipe role** — New expert-review and full-cycle recipe member focused on public-facing presentation, onboarding friction, naming/branding consistency, and feature discoverability.
- **Welcome message on first session** — New users see a welcome message instead of silent `{}`.
- **Doc gap insight dismissal** — `session(action="dismiss_insight")` now supports `insight_source="doc_gap"` to dismiss documentation gap insights. `insight_source` is now required for all dismissals to prevent cross-table ID collisions.

### Fixed
- **IPC module gated with `#[cfg(unix)]`** — Fixes Windows CI compilation.
- **Install script hook sync** — `install.sh` now matches plugin `hooks.json` (3 missing hooks, matcher patterns, timeouts).
- **File permissions hardening** — `.env.backup` chmod 600, `~/.mira` dir chmod 700.
- **`home_dir` fallback** — Warns instead of silently using CWD when home directory is unavailable.
- **Mira-wrapper version mismatch** — Pinned to 0.8.1 (was stale at 0.8.0) and fixed `stat` portability.
- **Setup wizard** — Added DeepSeek to setup steps, reordered by impact.
- **Clippy `doc_lazy_continuation` warnings** — Resolved across codebase.

### Changed
- **README rewrite** — Repositioned from "memory for Claude" to "intelligence layer" with before/after structure and upfront install command.
- **26 stale docs fixed** — Agentic team audit across 56 doc files: recipe.md no longer falsely marked CLI-only, DESIGN.md updated to 8 MCP tools, Zhipu references removed, analyze_diff.md renamed to diff.md, missing hooks/functions/types added.
- **First-run welcome simplified** — Single CTA instead of verbose output.
- **Recipe tool description** — Updated for newcomer clarity.

---

## [0.8.1] - 2026-02-16

### Changed
- **Extract `diff` into standalone MCP tool** — `diff` is now its own `#[tool]` with correct `DiffOutput` schema, fixing a latent schema mismatch where the `code` tool declared `CodeOutput` but returned `DiffOutput` for diff actions. The `code` tool drops from 9 to 6 params.
- **Clippy cleanup** — resolved collapsible-if, search-is-some, single-match, unwrap-used, and too-many-arguments warnings across pondering storage, IPC client, and IPC ops modules.

### Fixed
- Code tool MCP description no longer advertises diff capability (was causing invalid_params when clients followed the description).
- Plugin README now correctly lists 8 MCP tools (was 6, missing diff and recipe).

## [0.8.0] - 2026-02-14

### Added
- **Parallel fuzzy search** -- Fuzzy search now runs alongside semantic and keyword search in every query via `tokio::join!`, instead of only as a fallback when embeddings are unavailable. Catches typos and partial matches that both semantic and keyword miss.
- **Cross-project knowledge** -- Session recap and prompt context now surface relevant patterns and preferences learned from other projects.
- **Health dashboard** -- Categorized insights with trend tracking and 54 new tests. Provides system health overview via `session(action="insights")`.
- **/mira:status slash command** -- Quick health check showing index stats, storage, and active goals.
- **Codex CLI MCP integration** -- Mira can be used with the Codex CLI in addition to Claude Code.
- **Insight differentiation** -- Insights from Mira are now clearly distinguished from Claude Code's built-in system messages.

### Changed
- **Tool surface consolidation** -- Reduced from 9 MCP tools / 58 actions to 6 tools / 28 actions. Grouped by workflow instead of domain.
- **MIRA_FUZZY_FALLBACK renamed to MIRA_FUZZY_SEARCH** -- Reflects always-on behavior. The old env var name is no longer accepted.
- **Fuzzy search bounded concurrency** -- Fuzzy runs with a 500ms timeout via `tokio::spawn` (cache warms in background on timeout) and a semaphore limits to 1 concurrent fuzzy task to prevent pileup under load.

### Fixed
- **Session summaries missing for 94% of sessions** -- Fixed summary generation pipeline.
- **Behavior log tool count undercount** -- Snapshot zero counts and summary source selection corrected.
- **Stop hook summary source** -- Uses richer source, with regression tests added.
- **Goal list total** -- Returns true total for all modes; consistent empty-state message when limit=0 with existing goals.
- **Churn query column mismatch** -- Fixed legacy pattern leak and required MCP enforcement.
- **Source selection counts** -- Capped at 50 to match summary query limits.
- **Stale MCP references** -- Removed references to removed actions and corrected task method names.
- **Cross-project knowledge codex review findings** -- EXISTS hardening and UTF-8 truncation edge cases.
- **Collapsible if clippy warning** -- Fixed in session.rs cross-project preferences.

## [0.7.6] - 2026-02-13

### Added
- **Cross-session error pattern learning** -- Tool failures are fingerprinted (normalized error text hashed for O(1) lookup) and stored in `error_patterns` table. When the same error recurs in a future session, the `[Mira/fix]` hook injects the previously successful fix as context.
- **Data retention and cleanup system** -- Configurable retention policy via `[retention]` in config.toml with per-table age limits. `mira cleanup` CLI command with dry-run preview, category filtering, and orphan cleanup. Batched deletes to avoid holding SQLite write locks.
- **Structured compaction context** -- PreCompact hook extracts decisions, TODOs, and errors from the conversation transcript before Claude Code summarizes it, preserving structured context across compaction boundaries.

### Fixed
- **Error pattern auto-resolution correctness** -- Original implementation resolved ALL unresolved error patterns for a tool on any success, even when failures had different root causes. Now uses per-fingerprint validation: stores `error_fingerprint` in behavior log events, requires 3+ session failures of the SAME fingerprint, selects the most recently failing pattern by `sequence_position` (monotonic), scopes queries by `project_id`, and resolves at most one pattern per success.
- **CLI cleanup category filtering** -- `error_patterns` table was missing from `table_category()` mapping, causing `--category behavior` to miss it and potentially hit the "nothing to clean up" early-return path.
- **Orphan cleanup SQL** -- Fixed broken orphan cleanup queries and made CLI cleanup default to dry-run.
- **Hook dispatch panic** -- Fixed panic in hook dispatch, PreToolUse config mismatch, and cross-platform PID lock issues.

### Changed
- **Task completion logging** -- Uses `task_description` for richer completion logging and milestone matching.
- **Deeper Claude Code integration** -- New hooks (TaskCompleted, TeammateIdle, SubagentStart/Stop), protocol fixes, and plugin hardening.

## [0.7.5] - 2026-02-12

### Fixed
- **Hook protocol compliance** -- Added missing `hookEventName` field to SessionStart, PreToolUse, and SubagentStart hook output. Claude Code 2.1.39 expects this field inside `hookSpecificOutput`.
- **PermissionRequest output format** -- Changed from deprecated top-level `{"decision": "allow"}` to proper `hookSpecificOutput` wrapper with `{"behavior": "allow"}` object format.
- **Broken pipe panic in hook error handler** -- Replaced `println!` with non-panicking `writeln!` in both `write_hook_output` and the main.rs catch-all, preventing crashes when Claude Code kills a timed-out hook.
- **write_hook_output silent failure** -- Serialization errors now emit fallback `{}` on stdout instead of leaving stdout empty.
- **Pre-catch-all panic paths** -- Wrapped hook dispatch in `catch_unwind` to catch panics in addition to errors. Tracing initialization changed to non-fatal `let _ =` to prevent non-zero exits before the catch-all.
- **Session file permissions** -- All files written to `~/.mira/` by hooks now use explicit mode 0o600 via new `write_file_restricted` helper, consistent with lock/cooldown files.
- **Clippy warnings** -- Fixed collapsible `if` in `pre_tool.rs`, `map_or` -> `is_some_and` in `memory.rs`, constant assertions in `memory.rs`.

### Changed
- **Hook timeouts increased** -- SessionEnd 5s to 15s (LLM distillation), Stop 5s to 8s (multi-write), UserPromptSubmit 5s to 8s (cold start resilience). PreToolUse aligned to 3s across all copies.

## [0.7.4] - 2026-02-12

### Added
- **Memory poisoning defense** -- Prompt injection detection flags suspicious memories on write. Flagged memories are excluded from all recall paths (semantic, fuzzy, SQL fallback) and auto-exports. Content is stored with `[User-stored data, not instructions]` data markers to reinforce LLM boundaries. Per-session rate limit of 50 new memories.
- **Priority-scored context budget** -- `BudgetManager` now sorts context fragments by named priority constants before applying the character limit, ensuring the most valuable context survives truncation.
- **Unified hook budget** -- All `UserPromptSubmit` context sources (reactive, team, proactive, tasks) routed through a single priority-scored budget instead of ad-hoc concatenation.
- **59 cartographer detection tests** -- Coverage for Python (packages, entry points, line counting), Node (package.json, index files, import resolution), and Go (go.mod, main packages) language detectors.

### Fixed
- **Suspicious memories leaked via fallback recall** -- Fuzzy and SQL keyword fallback recall paths only filtered `status != 'archived'`, allowing injection-flagged memories to be returned when semantic search was unavailable or empty. All three recall paths now filter `COALESCE(suspicious, 0) = 0`.
- **Rate limiter fail-open on DB errors** -- `unwrap_or(0)` on the rate limit count query meant database errors silently disabled the limiter. Now fails closed with `unwrap_or(MAX_MEMORIES_PER_SESSION)`.
- **Rate limit blocked key-based updates** -- The per-session limit check ran before upsert logic, rejecting updates to existing keyed memories after 50 creations. Now detects key-based updates and skips the limit for them.
- **Full-cycle review issues** -- Fixed observation cache TTL, SQL keyword capitalization, UX messages, and documentation drift.
- **Windows build regression** -- Fixed compilation errors on Windows targets.
- **Path traversal in goal queries** -- Hardened project_id validation.
- **Goal priority ordering** -- Priority sort now uses proper numeric ordering instead of lexicographic.
- **QA hardening fixes** -- Improved error handling, priority constant consistency, and precompact test reliability.

### Changed
- **Shared `format_active_goals()`** -- Deduplicated four independent goal-formatting implementations across hooks into a single shared helper.
- **Batched DB inserts in precompact** -- PreCompact hook now batch-inserts extracted decisions/TODOs instead of one-at-a-time writes.

## [0.7.3] - 2026-02-12

### Added
- **System observations table** -- New `system_observations` table and `db/observations.rs` module for structured, TTL-aware storage of background analysis results. All readers and writers migrated from the previous ad-hoc storage.
- **TTL cleanup for observations** -- Automatic expiration of system observations with configurable retention (default 90 days).
- **`mira config` CLI command** -- Inspect and validate Mira configuration from the command line.
- **Status line redesign** -- Rainbow Mira display with provider info and improved ordering.
- **Lightweight startup context** -- Fresh sessions now get injected context without requiring a full recap.
- **Recall module extraction** -- Hook-side recall logic extracted into dedicated `hooks/recall.rs` with injection feedback tracking.

### Fixed
- **Doc task re-creation loop** -- Documentation tasks that were already skipped or completed would be re-created on every scan. Added unconditional unique index on `(project_id, target_doc_path)` with deterministic tie-breaking to prevent duplicates.
- **Migration ordering** -- `pattern_provenance` now dropped before `cross_project_patterns` to respect foreign key constraints.
- **DB schema drift** -- Aligned migration SQL with actual codebase column usage.
- **Proactive/reactive gating unification** -- Merged duplicated gating logic for hook-injected context.
- **Hook system DB access** -- Fixed incorrect database pool usage and removed dead code in hook system.
- **TTL timestamp resolution** -- Relative TTL durations now resolved to absolute timestamps before storage.
- **Cartographer pool mismatch** -- Map generation now uses `code_pool` instead of the main pool.
- **Memory confidence default** -- User-created memories default to 0.8 confidence (was 0.5), reflecting that explicit user storage implies high confidence.
- **Health scan cold-start** -- Auto-queue health scan only on genuine cold start, not on valid empty states.
- **Config hardening** -- Reject `sampling` as a configurable provider, fail loudly on config read errors, improved validation and error messages.
- **Team tool consistency** -- Consistent error messages and task retention cache behavior.
- **Clippy warnings** -- Collapsed nested `if let` chains, removed redundant closures, stabilized constant assertions.

### Changed
- **Cooldown file permissions** -- Cooldown state files now written with mode 0600 instead of world-readable defaults.
- **Dead table cleanup** -- Dropped unused tables: `corrections`, `users`, `pattern_provenance`, `cross_project_patterns`, `pattern_sharing_log`, `cross_project_preferences`.

## [0.7.2] - 2026-02-11

### Removed
- **Dead WebSocket and reply_to_mira infrastructure** -- ~670 lines of dead code from the pre-hook era: `WsEvent` enum, `AgentRole` enum, `broadcast()`/`is_collaborative()` on `ToolContext` and `MiraServer`, the `reply_to_mira` tool, `PendingResponseMap`, and all associated docs/tests/CLI handlers. Experts now use team recipes instead.

### Fixed
- **PreCompact `"matcher": "*"` drift** -- `install.sh` had a stale matcher that diverged from the plugin hooks config.
- **PermissionRequest timeout mismatch** -- Docs said 2s, actual is 3s. Fixed docs to match reality.
- **Tool count drift** -- Updated 10 → 9 across docs, skills, and plugin manifests after `reply_to_mira` removal.
- **Stale doc references** -- Corrected tools-reference session schema (was documenting non-existent nested sub-actions), added missing `dismiss_insight` and `export_claude_local` actions to tool docs, fixed file references from `.rs` to directories, added legacy/inactive notes to removed features.
- **Skills missing argument hints** -- Added argument-hint metadata to skill manifests for better discoverability.

### Changed
- **Marketplace description updated** -- Reflects current feature set.

## [0.7.1] - 2026-02-11

### Added
- **`/mira:qa-hardening` skill** -- Production readiness review is now a slash command. Previously only accessible via raw MCP tool calls.
- **`/mira:refactor` skill** -- Safe code restructuring is now a slash command. Runs architect analysis, code-reviewer validation, then implementation.
- **`use_when` field on recipe list** -- Recipe list responses now include a one-liner explaining when to use each recipe, so Claude can pick the right one without fetching every recipe's full coordination text.
- **Recipe validation tests** -- 4 structural invariant tests: task assignees match members, unique member names, no empty fields, "When to Use" section required. Catches broken recipes at test time.
- **"When to Use" sections** -- Added to expert-review and full-cycle recipe coordination strings (qa-hardening and refactor already had them).

### Fixed
- **Zhipu model case mismatch** -- Setup validation sent `"GLM-5"` but runtime uses `"glm-5"`. Case-sensitive APIs would reject valid keys during setup.
- **Wrong parsers reinstall hint** -- Error message said `cargo install mira --features parsers` but the package is `mira-server` and installs from git.
- **Phase 2.5 numbering in full-cycle** -- Renamed to Phase 3, bumped subsequent phases to 4 and 5.
- **Task tool parameter gaps in recipes** -- All 4 recipes now consistently document `team_name`, `name`, `subagent_type` for the Task tool. Previously only expert-review listed them.
- **Missing recipe hint on empty name** -- `recipe(action=get)` without a name now lists available recipes in the error message.
- **New skills registered in plugin manifest** -- Without this, the SKILL.md files wouldn't load as slash commands.

### Changed
- **Case-insensitive recipe lookup** -- `recipe(action=get, name="Expert-Review")` now works instead of returning "not found".
- **Shared prompt module** -- Extracted architect, security, and scope-analyst prompts into `prompts.rs` to eliminate duplication between expert-review and full-cycle recipes.
- **Stale docs updated** -- CONFIGURATION.md provider table corrected from `glm-4.7` to `glm-5`. Slash command tables in README.md, CLAUDE.md, and plugin/README.md updated with new skills.

## [0.7.0] - 2026-02-11

### Added
- **Self-updating wrapper** -- The plugin wrapper now checks GitHub for new releases every 24 hours and auto-updates the binary. Uses redirect-based version checks (no API rate limit), TTL-cached results, and graceful fallback when offline.
- **SHA256 checksum verification** -- Downloaded binaries are verified against checksums published with each release. The release workflow now generates and uploads `checksums.sha256`.
- **Version pinning** -- Set `MIRA_VERSION_PIN=X.Y.Z` to lock the wrapper to a specific version and skip auto-updates.
- **Dismiss insight action** -- New `session(action="dismiss_insight", insight_id=N)` to permanently hide individual insights from future queries.
- **Documentation system overhaul** -- Heuristic fallback for doc gap detection when LLM is unavailable, batch skip support, pagination, and UX improvements.

### Fixed
- **Insight dismiss scoped to active project** -- `dismiss_insight` now validates project ownership and restricts to `insight_%` pattern types, preventing cross-project mutation or dismissal of non-insight behavior patterns.
- **Insight dedup hardening** -- Entity-aware dedup with punctuation/apostrophe handling, key migration for legacy patterns, possessive-plural quote extraction, and single-quote delimiter validation.
- **Session close on interrupt** -- Properly closes sessions on SIGINT. Fixed hook scope bypass and UTF-8 panics.
- **Session-file race** -- Fixed race condition between session file reads and LLM factory inconsistency.
- **Entity backfill partial writes** -- Per-fact savepoints prevent partial database writes during entity extraction.
- **Watcher drop diagnostics** -- Improved circuit-breaker behavior and drop-time diagnostics.
- **Pagination edge case** -- Skip pagination header when offset is past the end.
- **Stale impact analysis** -- Codex review fix for stale data in impact analysis, path traversal prevention.
- **Insight hook messages** -- Surface actual pattern content instead of raw JSON in hook-surfaced insights.

### Changed
- **Wrapper messaging** -- Log prefix changed from `[mira-wrapper]` to `[mira]`. Distinct messages for fresh install vs update vs up-to-date. Release notes URL shown after updates.
- **Atomic version file writes** -- Version file now written via tmp+mv to prevent corruption from concurrent wrapper instances.
- **Stricter shell defaults** -- Wrapper uses `set -eu` and `umask 077` for safer operation.
- **Pool boilerplate reduction** -- Reduced repetitive database pool patterns, batch entity extraction.

### Removed
- Duplicate `.example` files (`hooks.json.example`, `.mcp.json.example`) that were identical to their non-example counterparts.

### Tests
- 21 scope isolation tests for multi-user memory sharing.
- 27 tests for hooks system helper functions.
- 4 dismiss insight tests (success, cross-project blocked, non-insight blocked, requires project).
- Multi-distro install tests for hooks, setup, and pinning.

## [0.6.9] - 2026-02-10

### Fixed
- **Hook dedup now handles all mira executable formats** -- Reinstalling on Windows no longer duplicates hooks. The jq filter matches `mira`, `mira.exe`, absolute Unix paths, quoted Windows paths (e.g., `"C:/Program Files/Mira/mira.exe"`), and backslash paths.
- **Hook dedup no longer over-matches** -- Regex is anchored to the first command token, so commands like `samira hook ...`, wrapper scripts with mira in arguments, or quoted strings containing "mira hook" are correctly preserved.
- **Hook merge preserves mixed entries at command level** -- When an entry contains both Mira and non-Mira commands, only the Mira commands are stripped; the entry and its custom commands survive.
- **Setup wizard .env single-char value panic** -- Parsing a `.env` value like `X=a` no longer panics on the quote-stripping range check.
- **Setup wizard [llm] section detection** -- Inline TOML comments (e.g., `[llm] # note`) are now recognized, preventing duplicate section creation.
- **Short API key exposure** -- Keys 12 chars or shorter are now fully masked instead of showing prefix/suffix.
- **config.toml indentation** -- `background_provider` replacement now preserves existing line indentation.

### Changed
- **CI: pinned `dtolnay/rust-toolchain` to `@v1` tag** -- All workflows now use the stable tag instead of `@stable` branch ref.
- **Hook documentation** -- CLAUDE.md hook table updated to accurately describe each hook's behavior, removed inaccurate claims, added undocumented capabilities.

### Added
- **Hook dedup fixture tests** -- `scripts/test-hook-dedup.sh` covers 10 cases (bare, absolute path, Windows, quoted, backslash, substring, wrapper, quoted arg, mixed entry, empty settings) to prevent future regressions.

## [0.6.8] - 2026-02-10

### Fixed
- **Health subtask starvation** -- Fast scans consumed the scan-needed flag, preventing module analysis and LLM tasks from finding work in the same cycle. Now fast scans leave the flag intact and module analysis finalizes it. LLM tasks use independent timestamp tracking.
- **Module analysis clearing scan flag on fast scan failure** -- When fast scans errored out, module analysis unconditionally cleared `health_scan_needed`, preventing retries. Now gated behind a `health_fast_scan_done` marker that only successful fast scans set.
- **Stale `health_fast_scan_done` marker** -- A timed-out module analysis could leave a stale marker from a previous cycle, letting a failed fast scan's successor incorrectly consume the scan flag. Marker is now cleared at the start of each fast scan.
- **Non-deterministic project selection in health subtasks** -- Subtasks independently queried for projects without `ORDER BY`, risking different phases operating on different projects. Added `ORDER BY id` to both project queries.
- **MCP tool doc-gap detection** -- Was reading `mcp/mod.rs` instead of `mcp/router.rs` where `#[tool()]` methods live. Also fixed parser breaking on multi-line `#[tool(description=..., output_schema=...)]` attributes.
- **Stale circular dependency findings** -- Module analysis now clears the `circular_dependency` category so findings don't persist after refactors.
- **Insight shown_count desync** -- When `UserPromptSubmit` hook surfaced a pondering insight, `shown_count` on the underlying `behavior_patterns` row wasn't incremented. Status line "new insights" count is now consistent regardless of surfacing path.
- **Priority sort in stop hook** -- Lexicographic sort replaced with proper `CASE` ordering.
- **`team_file_path_for_session` empty path ops** -- Now returns `Option<PathBuf>` to prevent operations on empty paths.
- **8 negative i64→usize casts** -- Hardened with `.max(0)` to prevent wraparound.
- **"1 results" grammar** -- Fixed pluralization in semantic search output.
- **Background service reliability** -- Added exponential backoff + max retries to supervisor, slow lane heartbeat for stall detection, `busy_timeout=1000` on readonly connections, `BufRead::read_line` to avoid blocking on unclosed stdin, shutdown checks between slow lane tasks.
- **Status line false positives** -- Uses heartbeat (5min threshold) instead of pondering timestamp (24h) to detect stalled background, eliminating false positives from pondering's cooldown gates.
- **SessionStart hook session registration** -- Upserts session into DB so background loop can discover active projects. Previously only wrote session ID to file, leaving `sessions` table empty.
- **Background intelligence UX** -- Clippy/formatting CI fixes, "no project" hint in status line, `[HIGH]`/`[INFO]` prefixes on insights, human-readable insight types, tool gap detection now only collects `#[tool]`-annotated functions.
- **Watcher reliability** -- Exponential backoff + max restart matching supervisor pattern.
- **Non-localhost OLLAMA_HOST warning** -- Defense-in-depth warning for remote Ollama hosts.

### Changed
- **Code health scan architecture** -- Split monolithic `scan_project_health()` (12 sequential steps where one timeout killed the rest) into 4 independent `BackgroundTask` variants: `HealthFastScans`, `HealthModuleAnalysis`, `HealthLlmComplexity`, `HealthLlmErrorQuality`. Each has its own timeout, project lookup, and category clearing. LLM tasks run at Low priority so they're skipped under load.
- **Status line** -- Removed vanity "knowledge" counter. Added stale docs count and background health indicator. Widened insight window from 24h to 7 days with new/seen split. Reordered by actionability.
- **Refactor recipe** -- Architect → code-reviewer now sequential (reviewer needs the plan). Team lead implements small/medium refactors directly. Test-runner optional for large refactors only.

### Refactored
- **recipe.rs** -- Split monolithic 773-line file into `recipe/` directory with one file per recipe and handler logic in `mod.rs`.
- **claude_local.rs** -- Split 1155-line file into `claude_local/` directory: `export.rs`, `import.rs`, `auto_memory.rs`, plus shared helpers in `mod.rs`.
- **code_health dependencies** -- Extracted `DepEdge`, `collect_dependency_data`, `scan_dependencies_sharded` and all dependency tests from `code_health/mod.rs` (1094 → 580 lines) into `dependencies.rs`. Removed dead `analyze_module_dependencies` code.

### Documentation
- Added on-ramp note to expert-review recipe pointing to full-cycle.
- Fixed lane assignments in `background.md`, expanded `CONCEPTS.md` task list.
- Removed deleted `cross_project/` from `CONTRIBUTING.md`, corrected tool count in CHANGELOG.

## [0.6.7] - 2026-02-10

### Added
- **QA hardening team recipe** -- New `qa-hardening` recipe for automated production readiness reviews. 5 parallel read-only agents (test-runner, error-auditor, security, edge-case-hunter, ux-reviewer) with synthesis into prioritized hardening backlog. Includes Phase 3 implementation guidance with file-ownership-grouped agents.
- **Refactor team recipe** -- New `refactor` recipe for safe code restructuring with parallel agents.

### Fixed
- **Silent data loss in diff caching** -- `.ok()` on serialization in `diff_analysis/mod.rs` silently dropped failures. Now logs `tracing::warn!` so incomplete cached results are visible.
- **Triple error suppression in hooks** -- `post_tool.rs` had three nested `let _ =` swallowing all diagnostics from tool use and file access logging. Inner errors now log at debug level.
- **usize underflow in stop hook** -- `file_names.len() - 3` could panic in debug builds. Replaced with `saturating_sub()`.
- **Non-atomic CLAUDE.local.md write** -- Direct `fs::write` could leave truncated files on crash. Now uses temp-file-and-rename pattern matching `write_auto_memory_sync()`.
- **Stale team detection** -- Leftover `.agent-team.json` files from previous sessions could inject phantom team context. Now validates config file exists and is less than 24 hours old.
- **Cooldown state corruption** -- `pre_tool.rs` used `unwrap_or_default()` on JSON serialization, writing empty strings that corrupt the next parse. Now skips the write on failure.
- **Unbounded stdin in hooks** -- `read_hook_input()` had no size limit. Added 1MB cap via `Read::take()`.
- **Unbounded retention DELETE** -- Retention cleanup could hold SQLite write lock on large backlogs. Now uses batched deletes with `LIMIT 10000` in a loop.
- **Session ID path injection** -- `team_file_path_for_session` used session ID in file paths without format validation. Now rejects non-alphanumeric/hyphen characters.
- **Git ref validation** -- `validate_ref()` now also rejects null bytes, newlines, and carriage returns.
- **Bulk goal size limit** -- `bulk_create` now rejects arrays over 100 goals.
- **MCP tool descriptions** -- Memory tool description now includes `archive` action. Team tool description now includes `distill` action.

### Changed
- **Error context on pool operations** -- Added descriptive `.map_err()` context to 8 key `pool.run()`/`pool.interact()` calls in memory, goals, and code search tools. Errors now include which operation failed, not just the raw database error.
- **QA hardening recipe improvements** -- Updated coordination instructions based on real-world usage: task-first creation, file-ownership grouping for implementation agents, Rust-specific build guidance, single-agent documentation consolidation.

### Documentation
- **CONCEPTS.md** -- Fixed `goal_id`/`milestone_id` shown as strings instead of integers in all examples. Added missing `goal_id` parameter to `progress` action example.
- **CONFIGURATION.md** -- Added `async: true` to PostToolUse, PreCompact, and SubagentStop in manual hook config. Added `MIRA_PROJECT_PATH` to environment variables table. Added `matcher: "*"` to PreCompact.
- **README.md** -- Corrected expert consultation description to reference Agent Teams recipes instead of removed MCP Sampling feature.

## [0.6.6] - 2026-02-09

### Changed
- **Flat session API** -- Replaced nested `session(action="history", history_action="get_history")` with flat `session(action="get_history")`. SessionAction enum now has 11 flat variants: `current_session`, `list_sessions`, `get_history`, `recap`, `usage_summary`, `usage_stats`, `usage_list`, `insights`, `tasks_list`, `tasks_get`, `tasks_cancel`. Deleted `SessionHistoryAction`, `UsageAction`, `TasksAction` enums and `TasksRequest` struct.
- **Optional tree-sitter and rayon** -- Tree-sitter dependencies now behind `parsers` feature, rayon behind `parallel` feature (both default on). Enables minimal builds with `--no-default-features`.

### Added
- **Data retention / GC** -- New `db/retention.rs` with periodic cleanup for 12 unbounded tables across 3 tiers (30/60/90 days). Runs as a low-priority background task every ~10 minutes. Preserves active sessions, memory_facts, behavior_patterns, goals, and code index tables.
- **152 new tests** -- 93 tests for background/code_health modules, 59 tests for tools/core/ modules.

### Removed
- **Cross-project module** -- Deleted orphaned `cross_project/` directory (1,090 lines) and `CrossProjectAction`/`CrossProjectRequest` types. DB schema preserved for potential future use.
- **scraper dependency** -- Removed unused `scraper` crate from Cargo.toml.

### Fixed
- **Retention timestamp columns** -- `diff_outcomes` and `pattern_sharing_log` retention rules used nonexistent `observed_at`/`shared_at` columns; corrected to `created_at`.
- **cfg parser edge cases** -- Handle tabs/newlines between `not` and `(` in cfg attribute parser, plus whitespace in `not()` and quoted strings.
- **PreToolUse hook performance** -- Added cooldown and dedup to reduce context bloat from repeated hook invocations.

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
- **Full-cycle recipe improvements** -- Added import cleanup rule, struct pattern rename syntax guide, diagnostic monitoring guidance, and documentation freshness checks for both plan-reviewer and ux-reviewer roles.
- **diff_outcomes UNIQUE constraint** -- Fixed duplicate key constraint on `(diff_analysis_id, outcome_type, evidence_commit)` that could cause insert failures.
- **MemoryRequest.id type** -- Changed from `String` to `i64`, removing runtime parse step.
- **Error message consistency** -- Standardized 30 error messages to `"<param> is required for <tool>(action=<action>)"` format.
- **Bulk goal atomicity** -- `bulk_create` goals now wrapped in a single transaction.
- **Goal/milestone auth** -- Fail-closed when project context is `None` instead of silently proceeding.

### Documentation
- **Comprehensive documentation audit** -- Fixed parameter types (`id`/`goal_id`/`milestone_id` are `i64` not `String`), removed nonexistent params, corrected tool counts, added missing skills to `plugin.json`, created `docs/tools/recipe.md`.
- **CLAUDE.md and rules fixes** -- Removed `--release` from build command, fixed anti-pattern file references, added `/mira:full-cycle` skill, added `forget`/`archive` actions to memory rules.
- **Module docs accuracy** -- Fixed CONFIGURATION.md, DATABASE.md schema, DESIGN.md paths, and tool references across 6 doc files.
- **hooks.json.example synced** -- Added 5 missing hooks (PermissionRequest, PreToolUse, SessionEnd, SubagentStart, SubagentStop) and `async: true` on PostToolUse and PreCompact.

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
- **Recipe system** -- Reusable team blueprints for Agent Teams. Built-in `expert-review` recipe with 6 roles (architect, code-reviewer, security, scope-analyst, ux-strategist, plan-reviewer).
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
- **MCP outputSchema** — Structured JSON responses for all 10 tools. Every tool now returns typed, parseable JSON instead of free-form text, enabling programmatic consumption of results.
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
- **Tool consolidation** - 40+ MCP tools reduced to **9** action-based tools
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
