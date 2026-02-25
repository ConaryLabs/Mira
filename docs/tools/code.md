<!-- docs/tools/code.md -->
# Code

Code intelligence: semantic search, call graph tracing, and static analysis.

> **MCP actions:** `search`, `symbols`, `callers`, `callees`, `bundle`
> **CLI-only actions:** `dependencies`, `patterns`, `tech_debt`, `dead_code`, `conventions`, `debt_delta`, `diff`
> CLI-only actions are available via `mira tool code '<json>'`.
> **Note:** `diff` was extracted into a standalone MCP tool. See [diff](./diff.md).

## Actions

### search

Find code by meaning using hybrid semantic + keyword search. Also detects cross-reference queries (e.g., "who calls X") and routes them to the call graph automatically.

**Parameters:**
- `action` (string, required) - `"search"`
- `query` (string, required) - Natural language search query
- `limit` (integer, optional) - Max results (default: 10)

**Returns:** Matching code snippets with file paths, similarity scores, symbol info, and expanded context.

**Search pipeline:** Cross-reference detection, parallel semantic + FTS5 search, symbol matching, tree-guided scope boost, intent reranking, graceful fallback to keyword/fuzzy when embeddings are unavailable.

### symbols

List all definitions (functions, structs, traits, etc.) in a file using tree-sitter parsing.

**Parameters:**
- `action` (string, required) - `"symbols"`
- `file_path` (string, required) - Absolute path to the file (must be within the project directory)
- `symbol_type` (string, optional) - Filter by type (e.g., `function`, `struct`, `trait`)

**Returns:** List of symbols with names, types, and line ranges.

### callers

Find all functions that call a given function.

**Parameters:**
- `action` (string, required) - `"callers"`
- `function_name` (string, required) - Function name to search for
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** List of calling functions with file paths.

### callees

Find all functions called by a given function.

**Parameters:**
- `action` (string, required) - `"callees"`
- `function_name` (string, required) - Function name to search for
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** List of called functions with file paths.

### dependencies (CLI-only)

Analyze module dependency graph and detect circular dependencies.

**Parameters:**
- `action` (string, required) - `"dependencies"`

**Returns:** Dependency edges with call/import counts and circular dependency warnings. Auto-queues a health scan if no data exists.

### patterns (CLI-only)

Detect architectural patterns (repository, builder, factory, etc.) across modules.

**Parameters:**
- `action` (string, required) - `"patterns"`

**Returns:** Per-module pattern detections with confidence scores and evidence. Auto-queues a health scan if no data exists.

### tech_debt (CLI-only)

Compute per-module tech debt scores with tier rankings (A-F).

**Parameters:**
- `action` (string, required) - `"tech_debt"`

**Returns:** Modules sorted worst-first with tier, overall score, line count, finding count, and top contributing factors for D/F tier modules. Auto-queues a health scan if no data exists.

### dead_code (CLI-only)

Find potentially unused functions and methods across the codebase.

**Parameters:**
- `action` (string, required) - `"dead_code"`
- `limit` (integer, optional) - Max results (default: 50)

**Returns:** Functions/methods with no detected callers, sorted by file. Auto-queues a health scan if no data exists.

### conventions (CLI-only)

Show detected coding conventions for the module containing a file.

**Parameters:**
- `action` (string, required) - `"conventions"`
- `file_path` (string, required) - Path to a file within the module to inspect

**Returns:** Error handling patterns, test patterns, naming conventions, key imports, and detected architectural patterns for the containing module. Auto-queues a health scan if no data exists.

### debt_delta (CLI-only)

Compare tech debt scores between the two most recent health snapshots.

**Parameters:**
- `action` (string, required) - `"debt_delta"`

**Returns:** Per-module tier changes (improved/regressed/unchanged), average score delta, and summary. Requires at least 2 health snapshots.

### diff (CLI-only, backward compat)

> **Prefer the standalone `diff` tool.** See [diff](./diff.md).
> The CLI still accepts `mira tool code '{"action":"diff"}'` for backward compatibility.

Analyze git changes semantically with impact and risk assessment.

**Parameters:**
- `action` (string, required) - `"diff"`
- `from_ref` (string, optional) - Starting git ref (commit, branch, tag). Max 256 characters
- `to_ref` (string, optional) - Ending git ref. Max 256 characters
- `include_impact` (boolean, optional) - Include impact analysis finding affected callers (default: true)

**Returns:** Files changed, lines added/removed, change summary, risk level, and historical risk data from mined change patterns.

**Auto-detection:** When no refs are provided, checks staged changes first, then working directory changes, then falls back to HEAD~1..HEAD.

## Examples

```json
{"action": "search", "query": "authentication handling"}
```

```json
{"action": "symbols", "file_path": "/home/user/project/src/main.rs"}
```

```json
{"action": "callers", "function_name": "handle_memory"}
```

```json
{"action": "tech_debt"}
```

## Prerequisites

- `search`, `callers`, `callees` require the project to be indexed via `index(action="project")`
- `symbols` requires the `parsers` compile-time feature
- `dependencies`, `patterns`, `tech_debt`, `dead_code`, `conventions`, `debt_delta` require a health scan (auto-queued if missing)

## Errors

- **"query is required"** - `search` needs a query
- **"file_path is required"** - `symbols` needs a file path
- **"function_name is required"** - `callers`/`callees` need a function name
- **"File not found"** - The specified file does not exist
- **"File path must be within the project directory"** - Security check for `symbols`
- **"No active project"** - `dependencies`, `patterns`, `tech_debt` require a project

## See Also

- [index](./index.md) - Build the code index that powers search and call graph
- [memory](./memory.md) - Search memories by meaning
