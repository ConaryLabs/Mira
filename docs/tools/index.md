<!-- docs/tools/index.md -->
# Index

Index codebase for semantic search and analysis. Builds embeddings, symbol tables, and module summaries.

> **MCP actions:** `project`, `file`, `status`
> Actions marked (CLI-only) below are available via `mira tool index '<json>'`.

## Actions

### project

Full project index. Parses files, extracts symbols, generates chunks and embeddings, auto-summarizes modules, and queues a health scan.

**Parameters:**
- `action` (string, required) - `"project"`
- `path` (string, optional) - Project root path (defaults to active project)
- `skip_embed` (boolean, optional) - Skip embedding generation for faster indexing (default: false)

**Returns:** File count, symbol count, chunk count, and number of modules summarized.

### file

Index a single file.

**Parameters:**
- `action` (string, required) - `"file"`
- `path` (string, required) - Path to the file to index

**Returns:** Indexing statistics for the file.

### status

Show index statistics.

**Parameters:**
- `action` (string, required) - `"status"`

**Returns:** Symbol count and embedded chunk count.

### compact (CLI-only)

Compact vec_code storage and VACUUM the database to reclaim space from deleted embeddings.

**Parameters:**
- `action` (string, required) - `"compact"`

**Returns:** Rows preserved and estimated storage savings in MB.

### summarize (CLI-only)

Generate LLM-powered summaries for modules that lack descriptions. Falls back to heuristic analysis when no LLM is configured. Also triggered automatically after `project` indexing.

**Parameters:**
- `action` (string, required) - `"summarize"`

**Returns:** Number of modules summarized and their summaries.

### health (CLI-only)

Run a full code health scan: dependency graphs, architectural pattern detection, tech debt scoring, and convention checking.

**Parameters:**
- `action` (string, required) - `"health"`

**Returns:** Number of issues found.

**Prerequisites:** Requires the project to be indexed first (`action="project"`).

## Examples

```json
{"action": "project"}
```

```json
{"action": "project", "skip_embed": true}
```

```json
{"action": "status"}
```

```json
{"action": "compact"}
```

```json
{"action": "health"}
```

## Notes

- The `project` and `file` actions require the `parsers` compile-time feature.
- After indexing, module summaries are auto-generated if an LLM provider is configured.
- A health scan is auto-queued after project indexing.
- File watching provides automatic incremental re-indexing when files change.
- The FTS5 index uses a code-aware tokenizer (`unicode61` with `tokenchars '_'`, no stemming).

## Errors

- **"Path not found"** - The specified path does not exist
- **"No active project"** - No path provided and no active project
- **"No code indexed yet"** - `health` requires prior indexing
- **"Code indexing requires the 'parsers' feature"** - Feature not enabled at compile time

## See Also

- [code](./code.md) - Search, symbols, and call graph (uses the index)
- [session](./session.md) - Track background index tasks via `tasks_list`/`tasks_get`
- [project](./project.md) - Project context (codebase map uses module summaries)
