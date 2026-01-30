# db/schema

Database schema creation and migration system.

## Key Function

`run_all_migrations()` - Creates tables and applies incremental migrations in order.

## Sub-modules

| Module | Tables Managed |
|--------|---------------|
| `code` | Code database migrations (symbols, chunks, modules) |
| `fts` | Full-text search indexes |
| `intelligence` | Proactive/evolutionary/cross-project tables |
| `memory` | Facts, corrections, docs, users, teams |
| `reviews` | Findings, corrections, embeddings, diffs, LLM usage |
| `session` | Sessions, tool history, chat |
| `system` | System prompts |
| `vectors` | sqlite-vec virtual tables for vector search |
