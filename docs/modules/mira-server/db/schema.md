# db/schema

Database schema creation and migration system.

## Key Function

`run_all_migrations()` - Creates tables and applies incremental migrations in order.

## Sub-modules

| Module | Tables Managed |
|--------|---------------|
| `code` | Code database migrations (symbols, chunks, modules). Includes `migrate_fts_tokenizer` which rebuilds `code_fts` when the FTS5 tokenizer config changes (current: `unicode61` with `tokenchars '_'`, no stemming). |
| `fts` | Full-text search indexes |
| `intelligence` | Proactive/evolutionary/cross-project tables |
| `memory` | Facts, corrections, docs, users, teams |
| `reviews` | Corrections, embeddings, diffs, LLM usage |
| `session` | Sessions, tool history, chat |
| `session_tasks` | Claude Code task persistence bridge |
| `team` | Team schema migrations |
| `entities` | Entity registry and linking tables |
| `system` | System prompts |
| `vectors` | sqlite-vec virtual tables for vector search |
