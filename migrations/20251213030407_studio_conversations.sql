-- Studio chat persistence
-- Conversations and messages for cross-device history

-- Conversations (chat sessions)
CREATE TABLE IF NOT EXISTS studio_conversations (
    id TEXT PRIMARY KEY,
    title TEXT,  -- Auto-generated from first message or user-set
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Messages within conversations
CREATE TABLE IF NOT EXISTS studio_messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES studio_conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- Indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_studio_messages_conversation ON studio_messages(conversation_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_studio_conversations_updated ON studio_conversations(updated_at DESC);

-- Rolling summaries for conversations (every ~100 messages)
-- Already have rolling_summaries table, add conversation_id support
-- We'll use session_id as conversation_id for compatibility
