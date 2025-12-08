-- backend/migrations/20251208080303_cleanup_schema.up.sql
-- Cleanup migration: Remove duplicate columns and unused tables

-- ============================================================================
-- PART 1: Remove duplicate columns
-- ============================================================================

-- message_analysis: remove duplicate programming_language (keep programming_lang)
-- First drop the index on the old column
DROP INDEX IF EXISTS idx_message_analysis_language;
ALTER TABLE message_analysis DROP COLUMN programming_language;

-- code_elements: remove duplicate line_start/line_end (keep start_line/end_line)
ALTER TABLE code_elements DROP COLUMN line_start;
ALTER TABLE code_elements DROP COLUMN line_end;

-- external_dependencies: remove duplicate imported_items (keep imported_symbols)
ALTER TABLE external_dependencies DROP COLUMN imported_items;

-- documents: remove duplicate file_size (keep size_bytes)
ALTER TABLE documents DROP COLUMN file_size;

-- artifacts: remove duplicate diff (keep diff_from_previous)
ALTER TABLE artifacts DROP COLUMN diff;

-- ============================================================================
-- PART 2: Consolidate operations.kind -> operation_kind
-- ============================================================================

-- First migrate any data from kind to operation_kind
UPDATE operations
SET operation_kind = kind
WHERE (operation_kind IS NULL OR operation_kind = '')
  AND kind IS NOT NULL AND kind != '';

-- Then drop the old column
ALTER TABLE operations DROP COLUMN kind;

-- ============================================================================
-- PART 3: Drop unused tables (DEFERRED)
-- ============================================================================
-- These tables are referenced by Rust code that needs cleanup first.
-- Will be dropped in a separate migration after code is updated:
--   - semantic_nodes
--   - semantic_edges
--   - concept_index
--   - semantic_analysis_cache
--   - design_patterns
