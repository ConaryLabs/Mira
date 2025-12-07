-- backend/migrations/20251207000001_fix_document_chunks_ids.sql
-- Backfill document_chunks.id from qdrant_point_id and add index on qdrant_point_id

-- Ensure every chunk has a primary key id; prefer existing id, else qdrant_point_id
UPDATE document_chunks
SET id = COALESCE(id, qdrant_point_id)
WHERE id IS NULL AND qdrant_point_id IS NOT NULL;

-- Optional: normalize legacy rows where id was left blank but qdrant_point_id exists
UPDATE document_chunks
SET id = qdrant_point_id
WHERE (id IS NULL OR id = '') AND qdrant_point_id IS NOT NULL;

-- Add an index on qdrant_point_id for faster lookups and deletes
CREATE INDEX IF NOT EXISTS idx_document_chunks_qdrant_point
    ON document_chunks(qdrant_point_id);
