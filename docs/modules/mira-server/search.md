# search

Unified search functionality shared between MCP tools and chat. Provides semantic search, cross-reference queries, keyword search, and context expansion.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `semantic` | Hybrid and pure semantic search using embeddings |
| `crossref` | Cross-reference search (callers/callees) |
| `keyword` | FTS5-powered keyword search with AND-first queries, symbol matching, and LIKE fallback |
| `tree` | Tree-guided scope narrowing — scores query terms against cartographer module tree and boosts results in relevant subtrees (1.3x) |
| `context` | Context expansion around search results |
| `utils` | Search formatting and utility functions |

## Key Functions

- `hybrid_search()` - Combined semantic + keyword search
- `semantic_search()` - Pure embedding-based similarity search
- `find_callers()` / `find_callees()` — Call graph traversal (exposed as `code(action="callers")` / `code(action="callees")`)
- `format_crossref_results()` - Format cross-reference results for display
