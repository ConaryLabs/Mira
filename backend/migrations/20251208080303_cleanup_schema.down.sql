-- backend/migrations/20251208080303_cleanup_schema.down.sql
-- Rollback: Restore dropped columns and tables

-- ============================================================================
-- PART 1: Restore duplicate columns
-- ============================================================================

ALTER TABLE message_analysis ADD COLUMN programming_language TEXT;
CREATE INDEX IF NOT EXISTS idx_message_analysis_language ON message_analysis(programming_language);

ALTER TABLE code_elements ADD COLUMN line_start INTEGER;
ALTER TABLE code_elements ADD COLUMN line_end INTEGER;

ALTER TABLE external_dependencies ADD COLUMN imported_items TEXT;

ALTER TABLE documents ADD COLUMN file_size INTEGER;

ALTER TABLE artifacts ADD COLUMN diff TEXT;

-- ============================================================================
-- PART 2: Restore operations.kind column
-- ============================================================================

ALTER TABLE operations ADD COLUMN kind TEXT;
UPDATE operations SET kind = operation_kind WHERE operation_kind IS NOT NULL AND operation_kind != '';

-- ============================================================================
-- PART 3: Table restoration (DEFERRED)
-- ============================================================================
-- No tables were dropped in this migration, so nothing to restore here.
