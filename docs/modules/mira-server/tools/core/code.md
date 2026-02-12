<!-- docs/modules/mira-server/tools/core/code.md -->
# tools/core/code

Unified code intelligence tools covering semantic search, symbol listing, call graph traversal, and static analysis.

## Overview

Implements the `code` MCP tool with actions for searching code by meaning, listing symbols in files, tracing caller/callee relationships, analyzing module dependencies, detecting architectural patterns, and computing tech debt scores. Also implements the `index` tool for project indexing operations.

## Key Functions

- `handle_code()` - MCP dispatcher for all `code(action=...)` requests
- `search_code()` - Semantic code search via hybrid (embedding + keyword) search
- `get_symbols()` - List symbols in a file using tree-sitter
- `find_function_callers()` / `find_function_callees()` - Call graph traversal
- `index()` - Project indexing dispatcher
- `summarize_codebase()` - Generate module summaries

### Query Core (for programmatic use)

- `query_search_code()` - Raw search results without MCP formatting
- `query_callers()` / `query_callees()` - Raw crossref results

## Sub-modules

| Module | Purpose |
|--------|---------|
| `search` | Semantic search and symbol listing |
| `index` | Project/file indexing, status, compact, summarize, health |
| `analysis` | Dependencies, patterns, tech debt queries |

## Architecture Notes

Search uses the hybrid search engine from `crate::search`, combining semantic (embedding-based) and keyword (FTS) results. File path validation ensures symbol requests stay within the project directory. The `Diff` action is intercepted by the MCP router and handled by `tools/core/diff.rs` instead.
