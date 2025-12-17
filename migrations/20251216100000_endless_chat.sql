-- Migrate from conversation-based chat to endless chat model
-- Chat is one continuous stream, no sessions/conversations

-- Drop old conversation-based tables
DROP TABLE IF EXISTS studio_messages;
DROP TABLE IF EXISTS studio_conversations;
DROP TABLE IF EXISTS studio_cache_metrics;

-- Single endless chat history
CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    blocks TEXT NOT NULL,  -- JSON array of MessageBlock
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- For pagination (newest first, load more = older)
CREATE INDEX IF NOT EXISTS idx_chat_messages_created ON chat_messages(created_at DESC);

-- Key-value store for chat state
CREATE TABLE IF NOT EXISTS chat_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Keys: last_response_id (for GPT-5.2 continuity)
