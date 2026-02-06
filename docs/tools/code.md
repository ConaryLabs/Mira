# code

Code intelligence. Actions: `search` (semantic), `symbols` (file structure), `callers`/`callees` (call graph), `dependencies` (module graph), `patterns` (architectural detection), `tech_debt` (per-module scores), `diff` (semantic git diff analysis).

## Usage

```json
{
  "name": "code",
  "arguments": {
    "action": "search",
    "query": "authentication middleware"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `search`, `symbols`, `callers`, `callees`, `dependencies`, `patterns`, `tech_debt`, `diff` |
| query | String | For search | Natural language query or identifier name |
| file_path | String | For symbols | Path to the file to analyze |
| function_name | String | For callers/callees | Function name to trace |
| symbol_type | String | No | Filter symbols by type (e.g. `function`, `struct`) |
| language | String | No | Language filter |
| limit | Integer | No | Max results (default varies by action) |
| from_ref | String | No | Starting git ref for `diff` (commit, branch, tag). Default: `HEAD~1` or staged/working changes if present |
| to_ref | String | No | Ending git ref for `diff`. Default: `HEAD` |
| include_impact | Boolean | No | Include impact analysis in `diff` (find affected callers). Default: `true` |

## Actions

### `search` — Hybrid semantic + keyword code search

Runs semantic (vector) and keyword (FTS5) searches in parallel, merges and deduplicates, then applies intent-based reranking.

```json
{ "action": "search", "query": "error handling patterns", "limit": 10 }
```

**How it works:**
- **Cross-reference detection**: Routes "who calls X" queries directly to call graph
- **FTS5 search**: AND-first query with OR fallback, code-aware tokenizer (`unicode61`, `tokenchars '_'`)
- **Symbol matching**: Exact (0.95), substring (0.85), partial (0.55–0.75)
- **Tree-guided scope**: Top 3 matching modules get 1.3x boost
- **Intent reranking**: Documentation (1.2x), implementation (1.15x), examples (1.25x)
- **Graceful degradation**: Falls back to keyword + fuzzy search when embeddings are unavailable (fuzzy optional)

### `symbols` — Get symbols from a file

Parses a file with tree-sitter and returns functions, structs, classes, etc.

```json
{ "action": "symbols", "file_path": "src/main.rs", "symbol_type": "function" }
```

Returns: Formatted list of symbols with types and line ranges.

### `callers` — Find what calls a function

```json
{ "action": "callers", "function_name": "handle_login", "limit": 20 }
```

Returns: List of calling functions with file paths and call counts, sorted by frequency.

### `callees` — Find what a function calls

```json
{ "action": "callees", "function_name": "process_request", "limit": 20 }
```

Returns: List of called functions with file locations.

### `dependencies` — Module dependency graph

Analyzes module dependencies and detects circular dependencies.

```json
{ "action": "dependencies" }
```

Returns: Dependency graph with circular dependency warnings.

### `patterns` — Architectural pattern detection

Detects common patterns (repository, builder, factory, etc.) in the codebase.

```json
{ "action": "patterns" }
```

Returns: Detected patterns with locations and confidence.

### `tech_debt` — Per-module tech debt scores

Computes tech debt scores per module based on complexity, test coverage gaps, and code health indicators.

```json
{ "action": "tech_debt" }
```

Returns: Ranked list of modules by tech debt score.

### `diff` — Semantic git diff analysis

Analyzes git changes for change types, impact, and risk. Uses LLM-powered analysis with heuristic fallback.

```json
{ "action": "diff", "from_ref": "HEAD~1", "to_ref": "HEAD" }
```

**Behavior:**
- If no refs are provided, Mira analyzes staged changes first, then working tree changes, otherwise `HEAD~1..HEAD`.
- Impact analysis uses the call graph (if indexed) to surface affected callers.

## Dependencies

- **Embeddings** (`OPENAI_API_KEY`) — Required for semantic search; without them Mira uses keyword + fuzzy fallback
- **Code index** — Run `index(action="project")` first for FTS5 and symbol data
- **Cartographer** — Module tree for scope narrowing (populated by background workers)

## Errors

- **"File not found"**: For `symbols`, the specified file doesn't exist
- **"function_name is required"**: For `callers`/`callees`
- **No results**: Function/file may not be indexed yet — run `index(action="project")`

## See Also

- [**index**](./index.md): Index project to build the code intelligence database
- [**memory**](./memory.md): Search memories by meaning
- [**expert**](./expert.md): Expert consultations that use code intelligence
