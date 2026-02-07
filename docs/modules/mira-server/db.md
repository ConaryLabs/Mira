# db

Unified database layer providing sync operations for all Mira features. Built on rusqlite with sqlite-vec for vector operations.

## Database

**Engine:** SQLite with sqlite-vec extension for vector similarity search.
**Connection pooling:** All access goes through `DatabasePool` (in `db::pool`). The legacy `Database` struct has been removed.
**Pattern:** All operations are `*_sync` functions taking `&rusqlite::Connection` directly, called via `pool.run()` or `pool.interact()`.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `pool` | `DatabasePool` connection pooling |
| `schema` | Database schema creation and migrations |
| `memory` | Memory storage/retrieval with semantic search |
| `embeddings` | Embedding queue management |
| `index` | Code symbol indexing (symbols, calls, imports) |
| `search` | Code search (semantic, FTS, call graph) |
| `project` | Project and session management |
| `session` | Session history and recap |
| `tasks` | Task and goal management |
| `milestones` | Goal milestone tracking |
| `reviews` | Code review findings |
| `documentation` | Documentation gap tracking |
| `team` | Team and shared memory |
| `usage` | LLM/embedding usage tracking |
| `chat` | Chat tracking |
| `background` | Background task support |
| `cartographer` | Module/dependency mapping |
| `diff_analysis` | Git diff semantic analysis |
| `diff_outcomes` | Diff outcome tracking |
| `entities` | Entity storage and retrieval |
| `insights` | Insight storage from pondering/proactive |
| `tech_debt` | Tech debt scores per module |
| `dependencies` | Module dependency tracking |
| `session_tasks` | Claude Code task persistence bridge |
| `migration_helpers` | Schema migration utilities |
| `types` | Shared database types |
