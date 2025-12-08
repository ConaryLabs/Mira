-- backend/migrations/20251208053335_project_session_tracking.sql
-- Add branch tracking and status to chat_sessions for project-based sessions

-- Add new columns for project session tracking
-- branch: Track which git branch the session is for
-- status: Track session lifecycle (active, committed, archived)
-- last_commit_hash: Track the last commit made in this session
ALTER TABLE chat_sessions ADD COLUMN branch TEXT;
ALTER TABLE chat_sessions ADD COLUMN status TEXT DEFAULT 'active';
ALTER TABLE chat_sessions ADD COLUMN last_commit_hash TEXT;

-- Indices for efficient lookup
CREATE INDEX IF NOT EXISTS idx_chat_sessions_user_project_branch
ON chat_sessions(user_id, project_path, branch);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_status ON chat_sessions(status);

-- ============================================================================
-- SESSION CHECKPOINTS
-- Track commit checkpoints within a session
-- ============================================================================

CREATE TABLE IF NOT EXISTS session_checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    commit_message TEXT,
    files_changed INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_session_checkpoints_session ON session_checkpoints(session_id);
CREATE INDEX IF NOT EXISTS idx_session_checkpoints_commit ON session_checkpoints(commit_hash);
