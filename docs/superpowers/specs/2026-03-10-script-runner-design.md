# Mira Script Runner ("Code Mode")

Replace all individual MCP tools with a single `run(code: string)` tool. Claude writes Rhai scripts that call Mira's APIs, chains operations, filters/transforms results, and returns only what it needs.

## Motivation

Mira currently uses lifecycle hooks to proactively inject context (semantic search results, symbol hints, reread advisories) into Claude's context window. This doesn't make enough of a difference — the injected fragments are small, budget-constrained, and Claude still has to figure out what to do with them.

The alternative — individual MCP tools that Claude calls explicitly — works better but creates token overhead: each tool call round-trips through the LLM, intermediate results consume context, and multi-step navigation requires multiple turns.

Inspired by [Cloudflare's Code Mode](https://blog.cloudflare.com/code-mode/), this design gives Claude a single script execution tool. Claude writes code that chains multiple API calls, filters results, and returns only the final shaped output. LLMs are better at writing code than making tool calls, and the script approach eliminates intermediate round-trips.

## MCP Surface

One tool:

```
run(code: string) -> structured JSON
```

The tool description includes a condensed API reference (function signatures + one-liner descriptions). `help()` inside Rhai returns the full reference with parameter details and examples.

## Rhai Binding Layer

New module: `crates/mira-server/src/scripting/`

### Architecture

1. Creates a Rhai `Engine` with sandboxing (no filesystem, no network, no `eval`)
2. Registers functions that map to existing tool logic in `tools/core/`
3. Executes the script, captures the return value
4. Serializes the result as the MCP tool response (`CallToolResult` with `structured_content`)

The bindings reuse existing tool implementation functions — the scripting layer is a thin adapter, not a reimplementation. When internal tool logic changes, the Rhai binding gets it for free.

### Context Passing

The Rhai engine gets a reference to `MiraServer` (or a lightweight context handle) at construction time. Bindings access database pools, embeddings client, project state, etc. through this — same as current tool handlers access `&self`.

### Async Bridge

Rhai is synchronous. Bindings need to call async Mira logic from sync Rhai callbacks. The approach:

- Use `spawn_blocking` with a oneshot channel: the Rhai callback spawns the async work onto the Tokio runtime via `spawn_blocking` + `Handle::block_on`, and receives the result through a channel. This avoids the `block_in_place` pitfalls (panics on `current_thread` runtime, blocks Tokio worker threads under concurrent scripts).
- Alternative if `spawn_blocking` proves awkward: validate `tokio::block_in_place` + `Handle::block_on` in the Rhai callback context specifically, and ensure the runtime is `multi_thread`. This is simpler but riskier under concurrency.
- The chosen approach should be validated with a concurrent-scripts test before committing to it.

## API Surface

Rhai supports function overloading by arity (number of arguments), not by type. Optional parameters (marked `?` below) are implemented as separate arity overloads — e.g., `search(query)` and `search(query, limit)` are two registered functions.

### Phase 1: Code Navigation

```rhai
// Search
search(query: string) -> Array           // semantic code search
search(query, limit: int) -> Array       // with result limit

// File intelligence
symbols(file_path: string) -> Array      // list definitions in a file
callers(function_name: string) -> Array  // what calls this function?
callees(function_name: string) -> Array  // what does this function call?

// Result helpers
format(data) -> string                   // format any result for readability
summarize(results, max: int) -> Array    // take top N results by relevance score
pick(results, fields: Array) -> Array    // select specific fields
help() -> string                         // full API reference
help(topic: string) -> string            // help on a specific function
```

### Phase 2: Full API

```rhai
// Goals
goal_create(title, priority?) -> Map
goal_bulk_create(goals: Array) -> Array
goal_list(status?) -> Array
goal_get(goal_id) -> Map
goal_update(goal_id, fields: Map) -> Map
goal_delete(goal_id) -> Map
goal_progress(goal_id) -> Map
goal_sessions(goal_id) -> Array
goal_add_milestone(goal_id, title, weight?) -> Map
goal_complete_milestone(milestone_id) -> Map
goal_delete_milestone(milestone_id) -> Map

// Project
project_init(path?) -> Map              // initialize/re-init project context (replaces project(action="start"))
project_info() -> Map                   // current project state (replaces project(action="get"))

// Session
recap() -> Map
current_session() -> Map

// Diff
diff(from_ref?, to_ref?, include_impact?) -> Map

// Index
index_project(skip_embed?) -> Map
index_status() -> Map

// Insights
insights() -> Map
dismiss_insight(id, source) -> Map

// Teams
launch(team, scope?) -> Map
```

## Execution Model

### Script Lifecycle

1. Claude calls `run(code: "...")`
2. Mira creates a fresh Rhai `Engine` with registered bindings (engine construction + registration is lightweight but should be profiled; engine pooling is an option if it becomes a bottleneck — add timing metrics to `run()` to inform this decision)
3. Engine configured with limits: max operations (100k), max call stack depth (32), max string length (1MB)
4. Entire script execution wrapped in `tokio::time::timeout` (30s wall-clock limit) to guard against slow binding calls (DB queries, embedding API). The Rhai operation limit catches tight loops; the wall-clock timeout catches slow I/O.
5. Script executes, return value captured
6. Return value serialized as JSON in `CallToolResult.structured_content`

**Output format:** The `run()` tool does not declare a fixed `output_schema` — the return shape is determined by the script. `structured_content` contains the script's return value as arbitrary `serde_json::Value`. Claude knows the shape because it wrote the script. The `content` field contains a text summary (the `format()` output if called, or a JSON pretty-print fallback).

### Binding Internals

Each Rhai function is a thin wrapper that:
- Converts Rhai `Dynamic` args to Rust types
- Calls existing tool logic (same functions `tools/core/*.rs` uses today)
- Converts Rust result to Rhai `Dynamic` (maps become Rhai `Map`, vectors become Rhai `Array`)

## Error Handling

**Script errors** (syntax, runtime, type mismatches):
- Rhai returns `EvalAltResult` with line/column info
- Mira wraps in structured error response: `{ "error": "...", "line": N, "suggestion": "..." }`
- Common mistakes (wrong arg types, unknown function names) get a suggestion field

**Tool-level errors** (DB unavailable, search returns nothing):
- Bindings return Rhai `Result` types — scripts can handle with try/catch or let them propagate
- Unhandled error becomes the script's error response with context about which function failed

**Resource limits exceeded:**
- Rhai operation limit: "Script exceeded operation limit (100k ops)" — catches tight loops
- Wall-clock timeout (30s): "Script timed out after 30s" — catches slow binding calls (DB, embeddings API)
- Both are necessary: operation limit alone doesn't cover slow I/O in bindings

## What Gets Removed

### MCP Tools Removed

`project`, `code`, `diff`, `goal`, `index`, `session`, `insights`, `launch`

### Hooks Removed

| Hook | What It Did |
|------|-------------|
| `UserPromptSubmit` | Reactive context injection based on user message |
| `PreToolUse` | Symbol hints, reread advisory, change pattern warnings |
| `SubagentStart` | Auto-injected project map and search hints |

### Hooks Kept

| Hook | What It Does |
|------|-------------|
| `SessionStart` | Session init, resume detection |
| `Stop` | Session snapshot |
| `SessionEnd` | Task snapshot on interrupt |
| `PreCompact` | Extract decisions/TODOs before summarization |
| `PostToolUse` | Track file modifications (fires on `run()`) |
| `SubagentStop` | Capture discoveries |
| `PostToolUseFailure` | Track failures |
| `TaskCompleted` | Log completions |
| `TeammateIdle` | Team tracking |

### Code Removed/Simplified

| Location | What Goes Away |
|----------|---------------|
| `mcp/router.rs` | 7+ `#[tool]` methods shrink to 1 |
| `mcp/requests.rs` | Most request types removed |
| `mcp/responses/` | Most response types removed |
| `context/` | Reactive context injection manager (entire proactive injection pipeline) |
| `hooks/pre_tool.rs` | File read cache, symbol hints, change pattern warnings |
| `hooks/user_prompt.rs` | Reactive context, dedup, budget management |
| `hooks/subagent.rs` | Project map injection (start portion) |

## Module Structure

```
crates/mira-server/src/
  scripting/
    mod.rs          -- public API: execute_script(server, code) -> Result<Value>
    engine.rs       -- Rhai Engine construction, sandboxing, limits
    bindings/
      mod.rs        -- registration entry point
      code.rs       -- search, symbols, callers, callees
      goals.rs      -- goal CRUD + milestones
      project.rs    -- project_init, project_info
      session.rs    -- recap, current_session
      diff.rs       -- diff analysis
      index.rs      -- indexing operations
      insights.rs   -- insights + dismiss
      teams.rs      -- launch
      helpers.rs    -- format, summarize, pick, help
```

## Discoverability

### Tool Description

The `run()` MCP tool description includes a condensed reference:

```
Execute a Rhai script with access to Mira's API.

Available functions: search(query), symbols(path), callers(fn), callees(fn),
goal_create(title), goal_list(), recap(), diff(), insights(), help().

Scripts can chain calls, filter results, and shape output.
Call help() for full reference, help("search") for specific functions.
```

### CLAUDE.md

The "Code Navigation Quick Reference" table gets replaced with script examples:

```rhai
// Find auth code and show its structure
let hits = search("authentication");
let syms = symbols(hits[0].file_path);
format(#{ search_results: hits, symbols: syms })
```

```rhai
// Trace a function's usage across the codebase
let who = callers("verify_credentials");
let what = callees("verify_credentials");
#{ called_by: who, calls_into: what }
```

```rhai
// Check goal progress
let goals = goal_list("in_progress");
pick(goals, ["title", "progress", "priority"])
```

## Testing Strategy

- **Unit tests per binding:** Each `bindings/*.rs` module gets tests that verify arg conversion, error propagation, and return shapes against the existing tool logic.
- **Integration tests:** Full script execution tests — multi-call scripts, error handling scripts, resource limit scripts.
- **Parity tests:** For each existing MCP tool, a script that replicates its behavior and asserts equivalent results. These validate that the migration doesn't lose functionality.
- **Concurrency test:** Multiple concurrent `run()` calls to validate the async bridge under load.
- **Timeout test:** Script with a deliberately slow binding call to verify wall-clock timeout fires.

## Migration Path

1. Add `rhai` dependency, build `scripting/` module with Phase 1 bindings (code navigation) + Phase 2 bindings (goals, session, project, diff, index, insights, teams). All bindings land before the cutover.
2. Register `run()` as the sole MCP tool, remove all existing tools from router.
3. Remove navigation hooks (`UserPromptSubmit`, `PreToolUse`, `SubagentStart`).
4. Remove `context/` injection pipeline and associated hook logic.
5. Update CLAUDE.md, plugin skills, and tool descriptions.
6. Clean up now-dead request/response types.

Note: `documentation` and `team` tools (already removed from MCP surface, CLI-only) remain accessible via `mira tool <name>` CLI unchanged.
