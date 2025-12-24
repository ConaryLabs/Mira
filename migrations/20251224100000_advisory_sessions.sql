-- Advisory sessions: Multi-turn conversations with external LLMs
-- Supports tiered memory: recent verbatim + summaries + pinned constraints

-- Advisory sessions track multi-turn conversations
CREATE TABLE IF NOT EXISTS advisory_sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    topic TEXT,                           -- Summary topic for the session
    mode TEXT NOT NULL DEFAULT 'council', -- 'single' or 'council'
    provider TEXT,                        -- If single mode: 'gpt-5.2', 'opus-4.5', 'gemini-3-pro'
    status TEXT NOT NULL DEFAULT 'active', -- 'active', 'summarized', 'archived'
    total_turns INTEGER NOT NULL DEFAULT 0,
    total_input_tokens INTEGER NOT NULL DEFAULT 0,
    total_output_tokens INTEGER NOT NULL DEFAULT 0,
    context_hash TEXT,                    -- Hash of current context for embedding cache
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER                    -- Auto-cleanup after N hours (NULL = never)
);

CREATE INDEX IF NOT EXISTS idx_advisory_sessions_project ON advisory_sessions(project_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_advisory_sessions_status ON advisory_sessions(status, updated_at DESC);

-- Individual messages in advisory sessions
CREATE TABLE IF NOT EXISTS advisory_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES advisory_sessions(id) ON DELETE CASCADE,
    turn_number INTEGER NOT NULL,
    role TEXT NOT NULL,                   -- 'user', 'assistant', 'synthesis'
    provider TEXT,                        -- Which model responded (for 'assistant' role)
    content TEXT NOT NULL,                -- Raw message content
    content_hash TEXT,                    -- Hash for deduplication
    token_count INTEGER,                  -- Estimated tokens
    synthesis_data TEXT,                  -- JSON: consensus, disagreements, insights (for 'synthesis' role)
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_advisory_messages_session ON advisory_messages(session_id, turn_number);
CREATE INDEX IF NOT EXISTS idx_advisory_messages_role ON advisory_messages(session_id, role);

-- Session summaries (when older turns are compressed)
CREATE TABLE IF NOT EXISTS advisory_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES advisory_sessions(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,                -- Compressed summary of older turns
    turn_range_start INTEGER NOT NULL,    -- First turn summarized
    turn_range_end INTEGER NOT NULL,      -- Last turn summarized
    message_ids TEXT NOT NULL,            -- JSON array of summarized message IDs
    token_estimate INTEGER,               -- Estimated tokens in summary
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_advisory_summaries_session ON advisory_summaries(session_id, turn_range_end DESC);

-- Pinned facts/constraints for a session (explicit items to always include)
CREATE TABLE IF NOT EXISTS advisory_pins (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES advisory_sessions(id) ON DELETE CASCADE,
    content TEXT NOT NULL,                -- The pinned fact/constraint
    pin_type TEXT NOT NULL DEFAULT 'constraint', -- 'constraint', 'decision', 'requirement'
    source_turn INTEGER,                  -- Turn where this was established (optional)
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_advisory_pins_session ON advisory_pins(session_id);

-- Decision log for a session (what was decided/rejected)
CREATE TABLE IF NOT EXISTS advisory_decisions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES advisory_sessions(id) ON DELETE CASCADE,
    decision_type TEXT NOT NULL,          -- 'accepted', 'rejected', 'deferred'
    topic TEXT NOT NULL,                  -- What was decided
    rationale TEXT,                       -- Why this decision was made
    source_turn INTEGER,                  -- Turn where this was decided
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_advisory_decisions_session ON advisory_decisions(session_id, decision_type);
