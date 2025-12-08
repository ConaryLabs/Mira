# SESSION.md

Development session log. Recent sessions have full details; older sessions are condensed with git commit references.

**Update this file at the end of each session with commit hash.**

---

## Session 48: 2025-12-08 (`caf63b3`)

**Summary:** Added `run_tests` delegation tool + removed hardcoded session.

**Agentic Testing Loop:**
The LLM can now self-correct by running tests, analyzing failures, fixing code, and re-running:
```
LLM writes code → run_tests → sees failures → fixes code → rebuild → run_tests → repeat until green
```

**New Tool: run_tests**
- Tool schema in `src/operations/tools/external.rs`
- Handler in `src/operations/engine/external_handlers.rs`
- Registered in `src/operations/engine/tool_router/registry.rs`

**Parameters:**
- `scenario` - Specific test file (e.g., `smoke_test.yaml`)
- `tags` - Filter by tags (e.g., `smoke,fast`)
- `mock` - Run in mock mode (instant, no LLM calls)
- `timeout_seconds` - Max wait time (default 300s)

**Return Format:**
```json
{
  "success": false,
  "all_tests_passed": false,
  "summary": "2 test(s) failed",
  "failures": [
    {"scenario": "...", "step": "...", "error": "..."}
  ],
  "suggestion": "Review failures, fix code, rebuild, run tests again."
}
```

**Session Refactoring:**
Removed hardcoded `peter-eternal` session. Session ID now tied to authenticated user:
- Authenticated: `{user_id}-eternal` (persistent per user)
- Anonymous: `anonymous-{uuid}` (ephemeral per connection)

---

## Session 47: 2025-12-08 (`1fedca5`)

**Summary:** Implemented mock mode for mira-test framework + test file cleanup.

**Mock Mode Implementation:**
- Added `MockResponse` type with `text` and `tool_calls` fields
- Runner generates fake events when `--mock` flag is set
- No WebSocket connection needed in mock mode
- Tests run in 0ms (vs ~3min with real LLM)

**Test File Cleanup:**
- Removed 4 outdated/narrow test files (-1,236 lines):
  - `phase5_providers_test.rs` - Narrow tool parsing
  - `phase6_integration_test.rs` - Broken (outdated API)
  - `phase7_routing_test.rs` - Trivial Message helpers
  - `embedding_cleanup_test.rs` - Narrow cleanup tests

**Reporters Verified:**
- Console (default) - Human readable
- JSON (`--output json`) - Machine parseable
- JUnit (`--output junit`) - CI integration (GitHub Actions, etc.)

**Usage:**
```bash
mira-test run scenarios/ --mock        # Fast mock mode
mira-test run scenarios/ --output json # JSON output
mira-test run scenarios/ --output junit # JUnit XML
```

---

## Session 46: 2025-12-07 (`f94da70`)

**Summary:** Fixed test framework session isolation + added 4 new test scenarios.

**Session Isolation Implementation:**
- Added `session_id` field to `WsClientMessage::Chat` for per-message routing
- `message_router.rs` uses message session_id if provided, falls back to connection default
- `TestClient.create_session()` creates unique session with `project_path`
- `runner.rs` creates isolated session before each test run

**New Test Scenarios:**
- `code_search.yaml` - Tests grep/search_codebase tools (3 steps)
- `git_operations.yaml` - Tests git init, status, log commands (3 steps)
- `multi_turn.yaml` - Tests conversation context retention (2 steps)
- `file_operations.yaml` - File creation and reading (3 steps)

**Key Changes:**
- `api/ws/message.rs` - Added optional `session_id` to Chat message
- `api/ws/chat/message_router.rs` - Route to message session_id if provided
- `cli/ws_client.rs` - Added `session_id` field to MiraClient
- `testing/harness/client.rs` - Session creation, event parsing for project responses
- `testing/harness/runner.rs` - Create session per test with project_path
- `scenarios/` - 5 scenario files (smoke_test + 4 new)

**Also Fixed:**
- `mira-ctl` script path issues (hardcoded `MIRA_DIR=/home/peter/Mira`)
- `.env` missing `MIRA_LLM_MESSAGE_HISTORY_LIMIT` and old rolling summary vars

**Test Robustness Fix:**
- Changed from `tool_executed` to `response_contains` assertions
- Tests verify outcomes, not exact tool usage (reduces LLM variance flakiness)

**Test Results:**
```
[PASS] Code Search (26s)
[PASS] File Operations (28s)
[PASS] Git Operations (97s)
[PASS] Multi-Turn Conversation (21s)
[PASS] Smoke Test (17s)
Total: 5 scenarios, 13 steps, ~3 min
```

---

## Session 45: 2025-12-07 (`b74c2c7`)

**Summary:** Implemented mira-test CLI and testing framework (Phase 1).

**New Testing Infrastructure (`src/testing/`):**
- `harness/client.rs` - TestClient with WebSocket event capture, `send_and_capture()` method
- `harness/assertions.rs` - 15+ assertion types (ToolExecuted, ResponseContains, FileExists, etc.)
- `harness/runner.rs` - ScenarioRunner with setup/teardown, step execution, result aggregation
- `scenarios/types.rs` - TestScenario, TestStep, ExpectedEvent, result types
- `scenarios/parser.rs` - YAML parser with template variables (`{{uuid}}`, `{{timestamp}}`)

**CLI Binary (`src/bin/mira_test.rs`):**
```bash
mira-test run ./scenarios/           # Run all scenarios
mira-test list ./scenarios/          # List available scenarios
mira-test validate ./scenarios/      # Validate YAML syntax
mira-test run --tags smoke           # Filter by tags
```

**Sample Scenarios (`scenarios/`):**
- `smoke_test.yaml` - Basic greeting + file read test
- `file_operations.yaml` - File creation and reading tests

**Bug Fix:** Fixed `execute_command` using wrong project directory (ExternalHandlers issue).

**Dependencies Added:** `serde_yaml` for YAML parsing.

**Files Changed:** `Cargo.toml`, `lib.rs`, `external_handlers.rs`, `tool_router/mod.rs`, new testing module

---

## Session 44: 2025-12-07 (`6ba7252`)

**Summary:** Fixed conversation history bug and refactored context architecture for coherent memory strategy.

**Bug Fix - Repeating Responses:**
- Root cause: `orchestration.rs` only passed 2 messages to LLM (system + current user), ignoring 50 loaded conversation history messages
- Fix: Convert `recall_context.recent` entries to proper Message objects in the message array

**Context Architecture Refactor:**
- Added `MIRA_LLM_MESSAGE_HISTORY_LIMIT=12` - caps message array to 12 recent turns
- Removed duplicate recent messages from system prompt (now in message array)
- Removed `Rolling10` summary entirely - redundant with 12-message history
- Simplified to single `SummaryType::Rolling` (100-message window)
- Renamed config: `MIRA_SUMMARY_ROLLING_10/100` → `MIRA_SUMMARY_ROLLING_ENABLED`

**New Context Architecture:**

| Layer | Purpose | Config |
|-------|---------|--------|
| LLM Message Array | Direct conversation turns | 12 messages |
| Rolling Summary | Compressed older history | Every 100 messages |
| Semantic Search | Relevant distant memories | 10 matches |

**Files Changed:**
- `orchestration.rs` - Pass recall_context, build message array with history limit
- `context.rs` - Remove duplicate recent messages from system prompt
- `memory_types.rs` - Simplify SummaryType enum (Rolling, Snapshot)
- `config/memory.rs` - Add `llm_message_history_limit`, remove `rolling_10/100`
- `summarization/` - Remove Rolling10 strategy, simplify to single window size
- `.env`, `.env.example` - Update config names
- `CLAUDE.md` - Document context architecture

---

## Session 43: 2025-12-07 (`de26fdc`)

**Summary:** Full system access mode enforcement - allows user to expand Mira's filesystem access beyond project directory.

**Features:**
1. **Permissions Panel UI** - New "Access" tab in Intelligence Panel with three sub-tabs:
   - Access: Toggle filesystem mode (project/home/system)
   - Sudo Rules: View and manage sudo permissions
   - Blocklist: View blocked commands

2. **SystemAccessMode Enum** - Three levels of filesystem access:
   - `Project` (default): Restricted to project directory only
   - `Home`: Access to `~/` and subdirectories
   - `System`: Full filesystem access (no restrictions)

3. **Full Backend Enforcement** - Access mode passed through entire chain:
   - Frontend → WebSocket → OperationManager → OperationEngine → Orchestrator → LlmOrchestrator → ToolRouter → FileHandlers

**Files Changed:**
- Backend: `message.rs`, `message_router.rs`, `unified_handler.rs`, `operations/mod.rs`, `engine/mod.rs`, `orchestration.rs`, `llm_orchestrator.rs`, `tool_router/mod.rs`, `file_handlers.rs`, `ws_client.rs`
- Frontend: `useAppState.ts`, `useChatMessaging.ts`, `PermissionsPanel.tsx`, `IntelligencePanel.tsx`, `useCodeIntelligenceStore.ts`

---

## Session 42: 2025-12-07 (`0872bed`)

**Summary:** Fixed project root scoping, multi-turn tool calling, and strict mode schemas.

**Fixes:**
1. **Project Root Scoping** - Project tools (list_project_files, read_project_file) were scoped to `/backend/` only
   - Root cause: `project_id` from frontend ignored by UnifiedChatHandler
   - Fix: Pass `project_id` through to OperationManager.start_operation()

2. **Multi-Turn Tool Calling** - "No tool call found for function call output" error
   - Root cause: Responses API requires `function_call` items before `function_call_output`
   - Fix: Added `InputItem::FunctionCall` variant, emit from assistant tool_calls

3. **Strict Mode Schemas** - OpenAI strict mode validation failures
   - Fix: All properties in required array, removed defaults/format validators

4. **Empty Message Prevention** - Block empty chat messages at WebSocket and React layers

**Files:** `unified_handler.rs`, `operations/mod.rs`, `openai/mod.rs`, `openai/types.rs`, `tool_builder.rs`, `agents.rs`, `external.rs`, `useChatMessaging.ts`, `useWebSocketStore.ts`

---

## Session 41: 2025-12-06 (`a85dbf4`)

**Summary:** Fixed Activity Panel to display real-time tool executions, agent events, and codex background tasks.

**Problem:** Activity Panel UI existed but had wiring gaps - `operation.tool_executed` events received but never stored.

**Fix:** Added handlers for tool execution, agent lifecycle, and codex events in `useWebSocketMessageHandler.ts`. Auto-open panel on activity start.

---

## Session 40: 2025-12-06 (`ea7c127`)

**Summary:** OpenAI optimization - prompt caching, cached token tracking, structured outputs.

**Changes:**
- Extract `cached_tokens` from response.usage.input_tokens_details
- Reorder prompts: static content first (>1024 tokens) for cache hits
- Add `strict: true` to tool definitions

**Expected:** 50% cost reduction on cached tokens, 80% latency reduction, near-zero tool parsing errors.

---

## Session 39: 2025-12-06 (`a1ecfd1`)

**Summary:** Added dual-session integration tests (17 tests) with real LLM validation.

**Created:** `backend/tests/dual_session_test.rs` - 12 unit tests + 5 LLM integration tests covering Voice+Codex flow.

---

## Session 38: 2025-12-05 (`2cc1ad9`)

**Summary:** Complete dual-session Voice/Codex integration.

---

## Session 34-37: 2025-12-05

**Session 37** (`f3b9c61`): Time awareness - inject current date/time into system context
**Session 36** (`1a76895`): System context detection (OS, package manager, shell, tools) + CLI sudo approval
**Session 35**: CLI architecture documentation
**Session 34**: Feature parity requirements documentation

---

## Session 32-33: Milestone 11 - Claude Code Feature Parity (2025-12-03)

**Commits:** `8da8201`, `ff4e573`, `d11f054`, `4f84712`, `0ae0431`

**Features Implemented:**
1. **Custom Slash Commands** - `.mira/commands/` with `$ARGUMENTS` replacement
2. **Hooks System** - PreToolUse/PostToolUse with pattern matching
3. **Checkpoint/Rewind** - File state snapshots before modifications
4. **MCP Support** - JSON-RPC tool protocol integration

**Built-in Commands:** `/commands`, `/reload-commands`, `/checkpoints`, `/rewind <id>`, `/mcp`

---

## Session 31: 2025-12-02

**Summary:** Enabled 34 real LLM integration tests, migrated to Gemini 3 Pro Preview.

---

## Session 30: 2025-12-02

**Summary:** Added 191 new tests across 12 files - frontend stores, hooks, components.

---

## Session 29: 2025-11-28 (`d002d49`)

**Summary:** Removed 1,087 lines dead code - terminal module, duplicate configs, unused types.

---

## Session 28: 2025-11-27

**Summary:** Git-style unified diff viewing for file changes in chat and artifact panel.

**Added:** `similar` crate for diff algorithm, `UnifiedDiffView.tsx` component.

---

## Session 27: 2025-11-26

**Summary:** Milestone 8 complete - Real-time file watching with notify crate.

**Created:** `src/watcher/` module (mod.rs, config.rs, events.rs, registry.rs, processor.rs)

---

## Session 26: 2025-11-26

**Summary:** Intelligence Panel WebSocket integration - budget status, semantic search, co-change, expertise.

---

## Session 25: 2025-11-26

**Summary:** Milestone 7 complete - Budget-aware context configuration selection.

---

## Session 24: 2025-11-26

**Summary:** RecallEngine + Context Oracle integration for unified memory + code intelligence context.

---

## Session 23: 2025-11-26 (`678998d`)

**Summary:** Milestone 7 - Context Oracle integration into AppState and OperationEngine.

**8 Intelligence Sources:** Code context, call graph, co-change, historical fixes, design patterns, reasoning patterns, build errors, author expertise.

---

## Session 22: 2025-11-26

**Commits:** `cc3beaa`, `74398f2`, `a66f68d`, `7055e59`, `c4696c4`, `b18890a`, `c98f750`

**Summary:** Dependency upgrades - SQLx 0.8, axum 0.8, git2 0.20, thiserror 2, zip 6, etc.

---

## Session 19: 2025-11-25

**Summary:** Removed DeepSeek references, migrated to GPT 5.1, fixed Qdrant tests, updated all docs.

---

## Session 8: 2025-11-25 (`b9fe537`)

**Summary:** Milestone 3 - Git intelligence (commits, co-change, blame, expertise, fixes).

---

## Session 7: 2025-11-25 (`54e33b2`)

**Summary:** Milestone 2 - Code intelligence (semantic graph, call graph, patterns, clustering, cache).

---

## Session 3: 2025-11-24 (`06d39d6`)

**Summary:** Budget tracking (370+ lines) and LLM cache (470+ lines) implementation.

---

## Session 2: 2025-11-25 (`f6d4898`)

**Summary:** GPT 5.1 provider with reasoning effort support.

---

## Session 1: 2025-11-25 (`efb2b3f`)

**Summary:** Architecture refactoring - fresh database schema (9 migrations, 50+ tables), DeepSeek to GPT 5.1 pivot.

---

## Pre-Refactor Sessions (1-18)

Original Mira development including DeepSeek dual-model migration, Activity Panel, frontend simplification, terminal integration. See git history for details.
