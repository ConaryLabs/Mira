-- Add token usage tracking and message archival support
-- Fixes: deleted messages on summarization, lost token counts

-- Add archival columns to chat_messages
-- When a message is summarized, it's archived (not deleted)
ALTER TABLE chat_messages ADD COLUMN archived_at INTEGER;
ALTER TABLE chat_messages ADD COLUMN summary_id TEXT REFERENCES chat_summaries(id);

-- Index for efficient querying of non-archived messages
CREATE INDEX IF NOT EXISTS idx_chat_messages_active
    ON chat_messages(created_at DESC) WHERE archived_at IS NULL;

-- Token usage per message (assistant messages)
-- Stored separately to avoid bloating the main table
CREATE TABLE IF NOT EXISTS chat_usage (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES chat_messages(id),
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    reasoning_effort TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_chat_usage_message ON chat_usage(message_id);
