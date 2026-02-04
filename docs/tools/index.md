# index

Index code and git history. Actions: `project` (full index), `file` (single file), `status` (stats), `compact` (vacuum storage), `summarize` (module summaries), `health` (code health scan).

## Usage

```json
{
  "name": "index",
  "arguments": {
    "action": "project"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `project`, `file`, `status`, `compact`, `summarize`, or `health` |
| path | String | No | Path to index (defaults to current project) |
| skip_embed | Boolean | No | Skip embedding generation for faster indexing (default: false) |
| parallel | Boolean | No | Parsed but not yet used |
| max_workers | Integer | No | Parsed but not yet used |
| commit_limit | Integer | No | Parsed but not yet used |

## Actions

### `project` — Full project index

Parses files, extracts symbols, generates chunks and embeddings, summarizes modules. Long-running operations auto-enqueue as background tasks via MCP Tasks.

```json
{ "action": "project" }
{ "action": "project", "skip_embed": true }
```

### `file` — Index a single file

```json
{ "action": "file", "path": "src/main.rs" }
```

### `status` — Show index statistics

```json
{ "action": "status" }
```

Returns: Symbol count, embedded chunk count, and index health info.

### `compact` — Compact storage

VACUUMs vec tables to reclaim space from deleted embeddings and compacts sqlite-vec storage.

```json
{ "action": "compact" }
```

### `summarize` — Generate module summaries

Generates LLM-powered summaries for modules that lack descriptions. Falls back to heuristic analysis (file counts, language distribution, symbols) when no LLM is available. Also triggered automatically after `project` indexing.

```json
{ "action": "summarize" }
```

### `health` — Code health scan

Runs a full code health analysis: dependency graphs, architectural pattern detection, tech debt scoring, and convention checking. Auto-enqueues as a background task.

```json
{ "action": "health" }
```

## FTS5 Tokenizer

Indexing creates an FTS5 full-text search index (`code_fts`) with a code-aware tokenizer:
- **Tokenizer**: `unicode61` with `remove_diacritics 1` and `tokenchars '_'`
- **No stemming** — identifiers like `database_pool` are indexed as single tokens
- **Migration**: Tokenizer config changes trigger automatic FTS index rebuild

## Errors

- **"Path not found"**: The specified path does not exist
- **"No project path specified"**: No path provided and no active project
- **Database errors**: Failed to write index data

## See Also

- [**code**](./code.md): Search, symbols, call graph (uses the index)
- [**tasks**](./tasks.md): Check status of background index operations
- [**project**](./project.md): Initialize project context (uses module summaries)
