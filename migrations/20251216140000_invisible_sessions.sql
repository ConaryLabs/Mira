-- Invisible sessions: seamless context across restarts
-- No explicit session start/end - just continuous memory

-- Code compaction blobs from OpenAI /responses/compact
-- These are encrypted, opaque tokens that preserve code-relevant state
CREATE TABLE IF NOT EXISTS code_compaction (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    encrypted_content TEXT NOT NULL,  -- The opaque blob from OpenAI
    token_count INTEGER,              -- Approximate tokens saved
    files_included TEXT,              -- JSON array of file paths included
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER                -- Optional TTL
);

CREATE INDEX IF NOT EXISTS idx_code_compaction_project ON code_compaction(project_path, created_at DESC);

-- Extend chat_messages with project scoping and summary references
-- Note: SQLite doesn't support ADD COLUMN IF NOT EXISTS, so we check first
-- This is handled by the application layer

-- Chat context state per project
CREATE TABLE IF NOT EXISTS chat_context (
    project_path TEXT PRIMARY KEY,
    last_response_id TEXT,            -- OpenAI response ID for continuity
    last_compaction_id TEXT,          -- Most recent compaction blob
    window_start_id TEXT,             -- Oldest message in current window
    total_messages INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Message summaries (when window slides)
CREATE TABLE IF NOT EXISTS chat_summaries (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    summary TEXT NOT NULL,
    message_ids TEXT NOT NULL,        -- JSON array of summarized message IDs
    message_count INTEGER NOT NULL,
    token_estimate INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_chat_summaries_project ON chat_summaries(project_path, created_at DESC);
