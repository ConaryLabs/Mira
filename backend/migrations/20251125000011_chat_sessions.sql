-- backend/migrations/20251125000012_chat_sessions.sql
-- Chat Session Metadata: Unified session management for CLI and Frontend

-- ============================================================================
-- CHAT SESSION METADATA
-- ============================================================================
-- Note: This is separate from auth sessions. Chat sessions are tracked via
-- session_id in memory_entries. This table provides metadata for session
-- management UI (naming, project association, previews).

CREATE TABLE IF NOT EXISTS chat_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT,
    name TEXT,
    project_path TEXT,
    last_message_preview TEXT,
    message_count INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_active INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_user ON chat_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_project ON chat_sessions(project_path);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_active ON chat_sessions(last_active DESC);

-- ============================================================================
-- SESSION FORKS
-- ============================================================================
-- Track session fork relationships for history navigation

CREATE TABLE IF NOT EXISTS session_forks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_session_id TEXT NOT NULL,
    forked_session_id TEXT NOT NULL,
    fork_point_message_id INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (source_session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (forked_session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (fork_point_message_id) REFERENCES memory_entries(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_session_forks_source ON session_forks(source_session_id);
CREATE INDEX IF NOT EXISTS idx_session_forks_forked ON session_forks(forked_session_id);
