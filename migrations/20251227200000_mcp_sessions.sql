-- MCP Sessions: Track Claude Code session lifecycle and phase
-- Enables session-aware context delivery and cross-session continuity

CREATE TABLE IF NOT EXISTS mcp_sessions (
    id TEXT PRIMARY KEY,                      -- MCP session ID from initialize
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    phase TEXT NOT NULL DEFAULT 'early',      -- 'early', 'middle', 'late', 'wrapping'
    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_activity INTEGER NOT NULL DEFAULT (unixepoch()),

    -- Activity metrics for phase detection
    tool_call_count INTEGER NOT NULL DEFAULT 0,
    read_count INTEGER NOT NULL DEFAULT 0,    -- Read-only operations
    write_count INTEGER NOT NULL DEFAULT 0,   -- Mutations (remember, edit, etc.)
    build_count INTEGER NOT NULL DEFAULT 0,   -- Build/test operations
    error_count INTEGER NOT NULL DEFAULT 0,   -- Errors encountered
    commit_count INTEGER NOT NULL DEFAULT 0,  -- Git commits during session

    -- Progress tracking
    estimated_progress REAL DEFAULT 0.0,      -- 0.0-1.0 based on heuristics
    active_goal_id TEXT,                      -- Primary goal being worked on

    -- Session state
    status TEXT NOT NULL DEFAULT 'active',    -- 'active', 'idle', 'ended'
    end_reason TEXT,                          -- Why session ended (if ended)

    -- Context for resume
    touched_files TEXT,                       -- JSON array of files touched
    topics TEXT                               -- JSON array of detected topics
);

-- Fast lookup by project + status
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_project
    ON mcp_sessions(project_id, status, last_activity DESC);

-- Find recent active sessions for resume
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_active
    ON mcp_sessions(status, last_activity DESC);

-- Link existing mcp_history session_id to new table
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_lookup
    ON mcp_sessions(id, started_at);

-- Add session tracking to work_context if not present
-- (session_id column for associating work state with sessions)
ALTER TABLE work_context ADD COLUMN session_id TEXT REFERENCES mcp_sessions(id) ON DELETE SET NULL;

-- Add session_id to mcp_history for foreign key constraint
-- Note: SQLite doesn't enforce FK on existing column, this is for documentation
