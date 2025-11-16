-- backend/migrations/20251115_project_scoped_terminals.sql
-- Add project scoping to operations and create terminal sessions table

-- Add project_id to operations table for project-aware sessions
ALTER TABLE operations ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE CASCADE;

-- Create index for efficient project-based operation queries
CREATE INDEX IF NOT EXISTS idx_operations_project ON operations(project_id, created_at DESC);

-- Create terminal sessions table
CREATE TABLE IF NOT EXISTS terminal_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    conversation_session_id TEXT,
    working_directory TEXT NOT NULL,
    shell TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    closed_at INTEGER,
    exit_code INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

-- Create indexes for efficient terminal session queries
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_project ON terminal_sessions(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_conversation ON terminal_sessions(conversation_session_id);

-- Create index for active terminal sessions (not yet closed)
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_active ON terminal_sessions(project_id, closed_at) WHERE closed_at IS NULL;
