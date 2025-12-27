-- Claude Code Spawner tables
-- Tracks spawned Claude Code sessions and question relay

-- ============================================================================
-- Claude Sessions
-- ============================================================================

CREATE TABLE IF NOT EXISTS claude_sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),

    -- Process info
    pid INTEGER,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'starting', 'running', 'paused', 'completed', 'failed')),

    -- Spawning config
    initial_prompt TEXT NOT NULL,
    context_snapshot TEXT,  -- JSON: pre-computed context from Mira

    -- Lifecycle timestamps (unix epoch)
    spawned_at INTEGER,
    started_at INTEGER,
    completed_at INTEGER,
    exit_code INTEGER,

    -- Output
    summary TEXT,

    -- Metadata
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_claude_sessions_status ON claude_sessions(status);
CREATE INDEX IF NOT EXISTS idx_claude_sessions_project ON claude_sessions(project_id);

-- ============================================================================
-- Question Queue
-- ============================================================================

CREATE TABLE IF NOT EXISTS question_queue (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES claude_sessions(id) ON DELETE CASCADE,

    -- Question content
    question TEXT NOT NULL,
    options TEXT,  -- JSON array of {label, description}
    context TEXT,

    -- Status
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'answered', 'expired')),
    answer TEXT,

    -- Timestamps
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    answered_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_question_queue_session ON question_queue(session_id);
CREATE INDEX IF NOT EXISTS idx_question_queue_status ON question_queue(status);

-- ============================================================================
-- Session Output (for review)
-- ============================================================================

CREATE TABLE IF NOT EXISTS session_output (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES claude_sessions(id) ON DELETE CASCADE,

    -- Chunk info
    chunk_type TEXT NOT NULL,  -- stdout, stderr, tool_call, thinking, completion
    content TEXT NOT NULL,
    sequence_num INTEGER NOT NULL,

    -- Timestamp
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_session_output_session ON session_output(session_id, sequence_num);

-- ============================================================================
-- Session Reviews
-- ============================================================================

CREATE TABLE IF NOT EXISTS session_reviews (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES claude_sessions(id) ON DELETE CASCADE,

    -- Review result
    status TEXT NOT NULL
        CHECK (status IN ('approved', 'needs_changes', 'failed')),
    summary TEXT NOT NULL,

    -- Extracted info (JSON arrays)
    decisions_extracted TEXT,
    files_changed TEXT,

    -- Feedback
    feedback TEXT,
    follow_up_instructions TEXT,  -- JSON array

    -- Timestamp
    reviewed_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_session_reviews_session ON session_reviews(session_id);
