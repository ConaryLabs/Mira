# index

Index code and git history. Parses source files into symbols and chunks, generates embeddings for semantic search, and summarizes modules.

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
| action | String | Yes | Action to perform: `project`, `file`, or `status` |
| path | String | No | Path to index (defaults to current project path) |
| skip_embed | Boolean | No | Skip embedding generation for faster indexing (default: false) |

### Actions

| Action | Description |
|--------|-------------|
| `project` | Index the entire project: parse files, extract symbols, generate chunks and embeddings, summarize modules |
| `file` | Index a specific path (currently uses the same code path as `project`) |
| `status` | Show current index statistics without re-indexing |

## Returns

### `project` / `file`

```
Indexed 150 files, 1200 symbols, 3500 chunks, summarized 25 modules
```

### `status`

```
Index status: 1200 symbols, 3500 embedded chunks
```

## Examples

**Example 1: Index the whole project**
```json
{
  "name": "index",
  "arguments": { "action": "project" }
}
```

**Example 2: Quick re-index without embeddings**
```json
{
  "name": "index",
  "arguments": { "action": "project", "skip_embed": true }
}
```

**Example 3: Check index status**
```json
{
  "name": "index",
  "arguments": { "action": "status" }
}
```

## Errors

- **"Path not found: {path}"**: The specified path does not exist.
- **"No project path specified"**: No path provided and no active project to default to.
- **Database errors**: Failed to write index data.

## FTS5 Tokenizer

Indexing creates an FTS5 full-text search index (`code_fts`) with a code-aware tokenizer:

- **Tokenizer**: `unicode61` with `remove_diacritics 1` and `tokenchars '_'`
- **No stemming** — identifiers like `database_pool` are indexed as single tokens, preserving exact matches
- **Migration**: If the tokenizer config changes between versions, `migrate_fts_tokenizer` automatically rebuilds the FTS index from `vec_code` chunks

This affects how keyword search matches work — snake_case identifiers are searchable as whole tokens, and multi-term queries use AND-first logic with OR fallback.

## See Also

- **search_code**: Semantic code search (uses the index)
- **get_symbols**: Get symbols from a specific file (uses the index)
- **find_callers** / **find_callees**: Call graph queries (use the index)
- **summarize_codebase**: Generate module summaries (triggered automatically during indexing)
