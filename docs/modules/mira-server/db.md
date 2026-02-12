<!-- docs/modules/mira-server/db.md -->
# db

Unified database layer providing sync operations for all Mira features.

## Overview

All data persistence goes through this module. Built on rusqlite with the sqlite-vec extension for vector similarity search. The legacy `Database` struct has been removed; all access now goes through `DatabasePool` which provides async connection pooling via deadpool-sqlite.

**Pattern:** Every database operation is a `*_sync` function taking `&rusqlite::Connection` directly. These are called from async code via `pool.interact()`, `pool.run()`, or `pool.run_with_retry()`.

## Key Types

- `DatabasePool` -- Connection pool with WAL mode, retry-with-backoff, and secure file permissions
- `StatusFilter` -- Parsed status filter with negation support (e.g., `"!completed"`)
- `PRIORITY_ORDER_SQL` -- Shared SQL fragment for consistent priority ordering

## Sub-modules

| Module | Purpose |
|--------|---------|
| `pool` | `DatabasePool` connection pooling (deadpool-sqlite) |
| `schema` | Schema creation and versioned migrations |
| `memory` | Memory storage/retrieval with semantic search and entity boost |
| `embeddings` | Pending embedding queue management |
| `index` | Code symbol, chunk, and import indexing |
| `search` | Code search (semantic via sqlite-vec, FTS5, call graph) |
| `project` | Project CRUD, server state, session management |
| `session` | Session history, recap, tool call logging |
| `session_tasks` | Claude Code task persistence bridge |
| `tasks` | Goal and task CRUD |
| `milestones` | Goal milestone tracking with weighted progress |
| `observations` | System observations with TTL-based cleanup |
| `config` | Database-backed configuration settings |
| `documentation` | Documentation gap tracking |
| `team` | Team membership, file ownership, conflict detection |
| `usage` | LLM and embedding usage tracking |
| `chat` | Chat message and summary tracking |
| `background` | Background task support (permissions, summaries, health) |
| `cartographer` | Module/dependency mapping |
| `diff_analysis` | Git diff semantic analysis caching |
| `diff_outcomes` | Diff outcome tracking |
| `entities` | Entity storage and retrieval |
| `insights` | Insight storage from pondering/proactive analysis |
| `tech_debt` | Per-module tech debt scores |
| `dependencies` | Module dependency tracking |
| `retention` | Data retention and cleanup policies |
| `types` | Shared database types |
| `migration_helpers` | Schema migration utilities |
| `test_support` | Test database helpers (test-only) |

## Architecture Notes

The server uses two separate databases: the main database (`~/.mira/mira.db`) for memory, sessions, and goals; and a code database (`~/.mira/code.db`) for indexed symbols, chunks, and code search vectors. Both are accessed through separate `DatabasePool` instances. Write contention is handled via SQLite's busy_timeout (5s) and optional retry-with-backoff for critical writes.
