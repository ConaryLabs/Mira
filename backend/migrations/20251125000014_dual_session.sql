-- migrations/20251125000014_dual_session.sql
-- Dual Session Architecture: Voice (eternal) vs Codex (discrete) sessions
--
-- Voice sessions: Eternal rolling sessions with personality continuity
-- Codex sessions: Discrete task-scoped sessions for code work with compaction

-- ============================================================================
-- EXTEND CHAT_SESSIONS FOR SESSION TYPES
-- ============================================================================

-- Session type: 'voice' (eternal, default) or 'codex' (discrete task)
ALTER TABLE chat_sessions ADD COLUMN session_type TEXT DEFAULT 'voice';

-- For Codex sessions: references the Voice session that spawned it
ALTER TABLE chat_sessions ADD COLUMN parent_session_id TEXT REFERENCES chat_sessions(id);

-- For Codex sessions: current status ('running', 'completed', 'failed', 'cancelled')
ALTER TABLE chat_sessions ADD COLUMN codex_status TEXT DEFAULT NULL;

-- Brief description of what Codex session is doing
ALTER TABLE chat_sessions ADD COLUMN codex_task_description TEXT DEFAULT NULL;

-- OpenAI response_id for compaction continuity (used by both Voice and Codex)
ALTER TABLE chat_sessions ADD COLUMN openai_response_id TEXT DEFAULT NULL;

-- When Codex session started working
ALTER TABLE chat_sessions ADD COLUMN started_at INTEGER DEFAULT NULL;

-- When Codex session finished
ALTER TABLE chat_sessions ADD COLUMN completed_at INTEGER DEFAULT NULL;

-- Indexes for efficient lookups
CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent ON chat_sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_type ON chat_sessions(session_type);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_codex_status ON chat_sessions(codex_status);

-- ============================================================================
-- CODEX SESSION LINKS
-- ============================================================================
-- Tracks the relationship and data flow between Voice and Codex sessions

CREATE TABLE IF NOT EXISTS codex_session_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Session relationship
    voice_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    codex_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,

    -- Spawn information
    spawn_trigger TEXT NOT NULL,  -- 'router_detection', 'user_request', 'complex_task'
    spawn_confidence REAL,        -- Confidence score if auto-detected

    -- Context flow
    voice_context_summary TEXT,   -- Summary of Voice context given to Codex at spawn
    completion_summary TEXT,      -- Summary injected back to Voice on completion

    -- Usage tracking
    tokens_used_input INTEGER DEFAULT 0,
    tokens_used_output INTEGER DEFAULT 0,
    cost_usd REAL DEFAULT 0,
    compaction_count INTEGER DEFAULT 0,  -- How many times compaction was triggered

    -- Timestamps
    created_at INTEGER NOT NULL,
    completed_at INTEGER,

    UNIQUE(codex_session_id)  -- Each Codex session has exactly one parent Voice session
);

CREATE INDEX IF NOT EXISTS idx_codex_links_voice ON codex_session_links(voice_session_id);
CREATE INDEX IF NOT EXISTS idx_codex_links_codex ON codex_session_links(codex_session_id);
CREATE INDEX IF NOT EXISTS idx_codex_links_active ON codex_session_links(completed_at) WHERE completed_at IS NULL;

-- ============================================================================
-- SESSION INJECTIONS
-- ============================================================================
-- Stores summaries and notifications injected from Codex into Voice sessions

CREATE TABLE IF NOT EXISTS session_injections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Session relationship
    target_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    source_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,

    -- Injection content
    injection_type TEXT NOT NULL,  -- 'codex_completion', 'codex_progress', 'codex_error'
    content TEXT NOT NULL,         -- The injected content/summary
    metadata TEXT,                 -- JSON: files changed, tokens, duration, error details, etc.

    -- Timestamps and state
    injected_at INTEGER NOT NULL,
    acknowledged INTEGER DEFAULT 0,  -- Whether Voice session has "seen" this
    acknowledged_at INTEGER,

    -- For ordering progress updates
    sequence_num INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_injections_target ON session_injections(target_session_id);
CREATE INDEX IF NOT EXISTS idx_injections_source ON session_injections(source_session_id);
CREATE INDEX IF NOT EXISTS idx_injections_pending ON session_injections(target_session_id, acknowledged)
    WHERE acknowledged = 0;
CREATE INDEX IF NOT EXISTS idx_injections_type ON session_injections(injection_type);
