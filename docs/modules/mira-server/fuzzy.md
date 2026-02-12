<!-- docs/modules/mira-server/fuzzy.md -->
# fuzzy

Nucleo-based fuzzy fallback search for code chunks and memories.

## Overview

Provides fuzzy matching when embedding-based semantic search is unavailable (no OpenAI API key). Loads code chunks and memory facts into an in-memory cache from the database, then uses the `nucleo` matcher library for fast fuzzy string matching. The cache refreshes on a TTL basis (60s for code, 30s for memories).

## Key Types

- `FuzzyCache` -- Manages cached indexes for both code and memory, with TTL-based refresh and mutex-guarded concurrent refresh protection
- `FuzzyCodeResult` -- Search result with file_path, content, start_line, and normalized score
- `FuzzyMemoryResult` -- Search result with id, content, fact_type, category, and normalized score

## Key Functions

- `search_code(code_pool, project_id, query, limit)` -- Fuzzy search across indexed code chunks
- `search_memories(pool, project_id, user_id, team_id, query, limit)` -- Fuzzy search across stored memory facts with scope-aware visibility filtering
- `invalidate_code(project_id)` / `invalidate_memory(project_id)` -- Force cache refresh on next access

## Architecture Notes

Scores are normalized to 0.0-1.0 relative to the maximum score in each result set. The code index pre-computes a combined `"{file_path} {content}"` search text for each chunk to avoid repeated allocation during matching. Memory visibility respects the same scope rules (personal/project/team) as the SQL-backed search paths. Cache sizes are capped at 200K code items and 50K memory items. Enabled via `MIRA_FUZZY_FALLBACK` env var (defaults to true).
