# `search_code` Tool

Search code by meaning using hybrid semantic + keyword search.

## Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | — | Natural language query or identifier name |
| `language` | string | No | — | Language filter (currently unused, reserved) |
| `limit` | integer | No | 10 | Maximum number of results |

## How It Works

### Cross-Reference Detection

Before searching, the tool checks if the query matches cross-reference patterns like "who calls X", "callers of X", or "callees of X". If detected, it routes directly to the call graph instead of running a general search.

### Hybrid Search Pipeline

For general queries, the tool runs a parallel hybrid search:

```
Query
  ├── Semantic Search (vector similarity via embeddings)
  └── Keyword Search (FTS5 + symbol + LIKE)
         ├── FTS5: AND-first query, OR fallback
         ├── Symbol name matching (always runs)
         ├── LIKE chunk search (supplements sparse results)
         ├── Tree-guided scope boost (1.3x for relevant modules)
         └── Proximity boost (1.2x for NEAR matches)
  ↓
Merge & Deduplicate (by file_path + start_line, keep higher score)
  ↓
Intent-Based Reranking
  ↓
Results (with context expansion)
```

### Keyword Search Details

The keyword search uses three parallel strategies:

1. **FTS5 full-text search** — Builds an AND-first query (all terms must match). If AND yields no results, falls back to OR (any term matches). Uses a code-aware tokenizer (`unicode61` with `tokenchars '_'`) that preserves snake_case identifiers as single tokens. No stemming — exact identifiers are preserved.

2. **Symbol name matching** — Always runs alongside FTS5. Scores symbols by match quality: exact match (0.95), substring (0.85), all terms present (0.75), partial (0.55–0.70).

3. **LIKE chunk search** — Supplements when FTS5 + symbol results are sparse.

### Tree-Guided Scope Narrowing

When the cartographer module tree is available, the query terms are scored against module names, purposes, exports, and paths. The top 3 matching modules become "scope paths", and results within those modules receive a 1.3x score boost.

### Proximity Boost

For multi-term queries, an FTS5 `NEAR(term1 term2, 10)` query identifies results where terms appear within 10 tokens of each other. These results receive a 1.2x score boost.

### Intent Detection & Reranking

After merging, results are reranked based on detected query intent:

| Intent | Trigger phrases | Boost |
|--------|----------------|-------|
| Documentation | "docs for", "explain", "what is" | 1.2x for documented code |
| Implementation | "how does", "definition of" | 1.15x for function definitions |
| Examples | "example of", "how to use" | 1.25x for test/example files |
| General | (default) | No extra boost |

Additional boosts: complete symbols (1.1x), documented code (1.1x), recently modified files (up to 1.2x).

### Graceful Degradation

If embeddings are unavailable, the tool falls back to keyword-only search without error. Semantic search failures are logged as warnings and do not prevent keyword results from returning.

## Output Format

Results include a project context header followed by numbered results with file path, line number, search type indicator, score, and a code snippet with surrounding context.

## Dependencies

- **Embeddings** (`GEMINI_API_KEY`) — Required for semantic search, optional for keyword-only
- **Code index** — Must run `index` tool first for FTS5 and symbol data
- **Cartographer** — Module tree required for scope narrowing (populated by background workers)

## Examples

```
# Semantic query — finds code by meaning
search_code(query="authentication middleware", limit=5)

# Identifier search — keyword search excels here
search_code(query="database_pool", limit=5)

# Cross-reference detection — routes to call graph
search_code(query="who calls hybrid_search", limit=5)
```
