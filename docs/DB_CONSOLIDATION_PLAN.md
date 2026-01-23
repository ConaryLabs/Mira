# Database Consolidation Plan

## Overview

60 SQL operations scattered across 15 files need to move into the `db/` module.
This is Phase 1 of enabling PostgreSQL support.

## Scattered Operations by Domain

### 1. Memory Operations (14 calls → `db/memory.rs`)

**Files:** `tools/core/memory.rs`, `background/capabilities.rs`, `background/code_health/mod.rs`, `tools/core/claude_local.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| tools/core/memory.rs | UPDATE memory_facts (upsert) | `store_memory_with_session()` ✅ exists |
| tools/core/memory.rs | INSERT memory_facts | `store_memory()` ✅ exists |
| tools/core/memory.rs | INSERT vec_memory | `store_fact_embedding()` ✅ exists |
| tools/core/memory.rs | SELECT vec_memory (semantic) | **NEW:** `recall_semantic()` |
| tools/core/memory.rs | UPDATE session_count | `record_memory_access()` ✅ exists |
| background/capabilities.rs | INSERT vec_memory | `store_fact_embedding()` ✅ exists |
| background/capabilities.rs | DELETE memory_facts (caps) | **NEW:** `clear_capabilities()` |
| background/capabilities.rs | DELETE vec_memory orphans | **NEW:** `clear_orphaned_embeddings()` |
| background/code_health | DELETE/INSERT scan flags | **NEW:** `mark_health_scanned()` |
| background/code_health | DELETE health issues | **NEW:** `clear_health_issues()` |
| tools/core/claude_local.rs | INSERT memory_facts | `store_memory()` ✅ exists |

**Status:** Most functions exist in `db/memory.rs` but callers aren't using them!

---

### 2. Code Index Operations (13 calls → `db/index.rs` NEW)

**Files:** `indexer/mod.rs`, `background/watcher.rs`, `tools/core/code.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| indexer/mod.rs | DELETE call_graph, symbols, vec_code, imports, modules | **NEW:** `clear_project_index()` |
| background/watcher.rs | DELETE symbols, vec_code, imports for file | **NEW:** `clear_file_index()` |
| tools/core/code.rs | COUNT code_symbols | **NEW:** `count_symbols()` |
| tools/core/code.rs | COUNT vec_code | **NEW:** `count_embedded_chunks()` |

---

### 3. Codebase Map Operations (10 calls → `db/cartographer.rs` NEW)

**Files:** `cartographer/map.rs`, `cartographer/summaries.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| cartographer/map.rs | COUNT codebase_modules | **NEW:** `count_cached_modules()` |
| cartographer/map.rs | SELECT codebase_modules | **NEW:** `get_cached_modules()` |
| cartographer/map.rs | SELECT code_symbols (exports) | **NEW:** `get_module_exports()` |
| cartographer/map.rs | COUNT code_symbols (path) | **NEW:** `count_symbols_in_path()` |
| cartographer/map.rs | SELECT imports (deps) | **NEW:** `get_module_dependencies()` |
| cartographer/map.rs | INSERT/REPLACE codebase_modules | **NEW:** `upsert_module()` |
| cartographer/summaries.rs | SELECT modules needing summaries | **NEW:** `get_modules_needing_summaries()` |
| cartographer/summaries.rs | UPDATE module purposes | **NEW:** `update_module_purposes()` |

---

### 4. Search Operations (10 calls → `db/search.rs` NEW)

**Files:** `search/crossref.rs`, `search/keyword.rs`, `search/context.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| search/crossref.rs | SELECT call_graph (callers) | **NEW:** `find_callers()` |
| search/crossref.rs | SELECT call_graph (callees) | **NEW:** `find_callees()` |
| search/keyword.rs | SELECT code_fts (FTS) | **NEW:** `fts_search()` |
| search/keyword.rs | SELECT vec_code (LIKE) | **NEW:** `chunk_like_search()` |
| search/keyword.rs | SELECT code_symbols (LIKE) | **NEW:** `symbol_like_search()` |
| search/context.rs | SELECT symbol bounds | **NEW:** `get_symbol_bounds()` |

---

### 5. Symbol Operations (4 calls → `db/symbols.rs` NEW)

**Files:** `background/diff_analysis.rs`, `background/documentation/*.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| background/diff_analysis.rs | SELECT code_symbols for files | **NEW:** `get_symbols_for_files()` |
| background/documentation/* | SELECT code_symbols for file | **NEW:** `get_symbols_for_file()` |
| background/documentation/* | SELECT projects (indexed) | **NEW:** `get_indexed_projects()` |

---

### 6. Session/Project Operations (3 calls → `db/session.rs`, `db/project.rs`)

**Files:** `tools/core/project.rs`, `tools/core/documentation.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| tools/core/project.rs | INSERT sessions | **NEW:** `create_session()` |
| tools/core/project.rs | UPDATE projects (name) | **NEW:** `update_project_name()` |
| tools/core/documentation.rs | SELECT projects (path) | **NEW:** `get_project_path()` |

---

### 7. Stats/Misc Operations (4 calls)

**Files:** `main.rs`, `hooks/permission.rs`, `tools/core/code.rs`

| Current Location | Operation | Target Function |
|-----------------|-----------|-----------------|
| main.rs | SELECT proxy_usage stats | Move to `db/proxy.rs` |
| hooks/permission.rs | SELECT permission_rules | **NEW:** `get_permission_rules()` |
| tools/core/code.rs | DELETE codebase_modules (no purpose) | Move to `db/cartographer.rs` |
| tools/core/code.rs | SELECT vec_memory (capabilities) | Same as memory semantic search |

---

## Summary

| New/Updated Module | Functions to Add | Calls Consolidated |
|-------------------|------------------|-------------------|
| `db/memory.rs` | 5 new functions | 14 calls |
| `db/index.rs` (NEW) | 4 functions | 13 calls |
| `db/cartographer.rs` (NEW) | 8 functions | 10 calls |
| `db/search.rs` (NEW) | 6 functions | 10 calls |
| `db/symbols.rs` (NEW) | 3 functions | 4 calls |
| `db/session.rs` | 1 function | 1 call |
| `db/project.rs` | 2 functions | 2 calls |
| `db/proxy.rs` | 1 function | 2 calls |
| `db/permissions.rs` (NEW) | 1 function | 1 call |
| **TOTAL** | ~31 functions | 60 calls |

## Implementation Order

1. **Start with `db/memory.rs`** - Update callers to use existing functions (no new code needed)
2. **Create `db/index.rs`** - Critical for indexer operations
3. **Create `db/search.rs`** - Used by all search operations
4. **Create `db/cartographer.rs`** - Isolates codebase mapping
5. **Remaining modules** - Lower priority

## Notes

- Several operations in `tools/core/memory.rs` already have equivalents in `db/memory.rs` but aren't being used
- The `pool.interact()` pattern should be used consistently for async contexts
- Consider whether functions should take `&Connection` or use `&Database` (latter is more flexible for pooling)
