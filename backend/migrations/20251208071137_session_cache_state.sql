-- backend/migrations/20251208071137_session_cache_state.sql
-- Session cache state for LLM-side prompt caching optimization
--
-- Tracks what was sent to OpenAI per session to enable:
-- 1. Incremental context updates (only send what changed)
-- 2. Cache hit rate monitoring
-- 3. TTL-aware context decisions

-- Main session cache state table
CREATE TABLE IF NOT EXISTS session_cache_state (
    session_id TEXT PRIMARY KEY,

    -- Hash of static prefix (persona + env + tools + guidelines)
    -- If this changes, the entire OpenAI cache is invalidated
    static_prefix_hash TEXT NOT NULL,

    -- When the last LLM call was made (for TTL estimation)
    last_call_at INTEGER NOT NULL,

    -- Hashes of dynamic context sections
    project_context_hash TEXT,
    memory_context_hash TEXT,
    code_intelligence_hash TEXT,
    file_context_hash TEXT,

    -- Token tracking
    static_prefix_tokens INTEGER DEFAULT 0,
    last_cached_tokens INTEGER DEFAULT 0,

    -- Aggregate stats
    total_requests INTEGER DEFAULT 0,
    total_cached_tokens INTEGER DEFAULT 0,

    -- Timestamps
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- File content hashes for incremental file context
CREATE TABLE IF NOT EXISTS session_file_hashes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    sent_at INTEGER NOT NULL,

    FOREIGN KEY (session_id) REFERENCES session_cache_state(session_id) ON DELETE CASCADE,
    UNIQUE(session_id, file_path)
);

-- Indexes for efficient lookup
CREATE INDEX IF NOT EXISTS idx_session_cache_state_last_call ON session_cache_state(last_call_at);
CREATE INDEX IF NOT EXISTS idx_session_file_hashes_session ON session_file_hashes(session_id);
