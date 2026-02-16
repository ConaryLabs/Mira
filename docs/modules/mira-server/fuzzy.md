<!-- docs/modules/mira-server/fuzzy.md -->
# fuzzy

Nucleo-based fuzzy search for code chunks and memories.

## Overview

Provides fuzzy matching as part of the hybrid search pipeline alongside semantic and keyword search. Loads code chunks and memory facts into an in-memory cache from the database, then uses the `nucleo` matcher library for fast fuzzy string matching. The cache refreshes on a TTL basis (60s for code, 30s for memories). The cache warms in the background with mutex-guarded concurrent refresh protection.

## Key Types

- `FuzzyCache` -- Manages cached indexes for both code and memory, with TTL-based refresh and mutex-guarded concurrent refresh protection
- `FuzzyCodeResult` -- Search result with file_path, content, start_line, and normalized score
- `FuzzyMemoryResult` -- Search result with id, content, fact_type, category, and normalized score

## Key Functions

All search and invalidation functions are methods on `FuzzyCache`:

- `FuzzyCache::search_code(&self, code_pool, project_id, query, limit)` -- Fuzzy search across indexed code chunks
- `FuzzyCache::search_memories(&self, pool, project_id, user_id, team_id, query, limit)` -- Fuzzy search across stored memory facts with scope-aware visibility filtering
- `FuzzyCache::invalidate_code(&self, project_id)` / `FuzzyCache::invalidate_memory(&self, project_id)` -- Force cache refresh on next access

## Architecture Notes

Scores are normalized to 0.0-1.0 relative to the maximum score in each result set. The code index pre-computes a combined `"{file_path} {content}"` search text for each chunk to avoid repeated allocation during matching. Memory visibility respects the same scope rules (personal/project/team) as the SQL-backed search paths. Cache sizes are capped at 200K code items and 50K memory items. Enabled via `MIRA_FUZZY_SEARCH` env var (defaults to true).
