<!-- docs/modules/mira-server/search.md -->
# search

Unified search functionality shared between MCP tools and chat.

## Overview

Provides multiple search strategies (semantic, keyword, cross-reference) that can be used individually or combined via hybrid search. Results include file paths, line numbers, content snippets, and relevance scores. The module is used by the `code` MCP tool and the context injection system.

## Key Functions

- `hybrid_search()` -- Combined semantic + keyword search with parallel execution and score-based deduplication
- `semantic_search()` -- Pure embedding-based similarity search via sqlite-vec
- `keyword_search()` -- FTS5-powered keyword search with AND-first queries, symbol matching, and LIKE fallback
- `find_callers()` / `find_callees()` -- Call graph traversal
- `crossref_search()` -- Cross-reference search combining callers and callees
- `expand_context()` -- Expand search results with surrounding code lines

## Sub-modules

| Module | Purpose |
|--------|---------|
| `semantic` | Hybrid and pure semantic search using embeddings |
| `crossref` | Cross-reference search (callers/callees via call_graph table) |
| `keyword` | FTS5-powered keyword search with AND-first queries, symbol matching, and LIKE fallback |
| `tree` | Tree-guided scope narrowing -- scores query terms against cartographer module tree and boosts results in relevant subtrees |
| `context` | Context expansion around search results (reads source files for surrounding lines) |
| `utils` | Search formatting, distance-to-score conversion, embedding byte conversion |
