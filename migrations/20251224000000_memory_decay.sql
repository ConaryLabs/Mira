-- Memory Decay System: Query-time freshness with validity tracking
-- Adds validity enum and file association for memories

-- Add validity status for memories (active, stale, superseded)
ALTER TABLE memory_facts ADD COLUMN validity TEXT DEFAULT 'active';

-- Add superseding support for decisions
ALTER TABLE memory_facts ADD COLUMN superseded_by TEXT;

-- Add optional file association for file-scoped invalidation
ALTER TABLE memory_facts ADD COLUMN file_path TEXT;

-- Index for efficient validity filtering and file lookups
CREATE INDEX IF NOT EXISTS idx_facts_validity ON memory_facts(validity);
CREATE INDEX IF NOT EXISTS idx_facts_file_path ON memory_facts(file_path);
CREATE INDEX IF NOT EXISTS idx_facts_created_at ON memory_facts(created_at);
