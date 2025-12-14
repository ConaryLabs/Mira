-- Studio cache metrics tracking for Anthropic prompt caching
-- Tracks cache hits/misses to monitor cost savings

CREATE TABLE IF NOT EXISTS studio_cache_metrics (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    message_id TEXT,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    uncached_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    model TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES studio_conversations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cache_metrics_conversation ON studio_cache_metrics(conversation_id);
CREATE INDEX IF NOT EXISTS idx_cache_metrics_created ON studio_cache_metrics(created_at);
