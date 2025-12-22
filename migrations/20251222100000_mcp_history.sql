-- MCP interaction history for Claude Code sessions
-- Captures tool calls so they can be recalled like chat history

CREATE TABLE IF NOT EXISTS mcp_history (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    session_id TEXT,                    -- Claude Code session ID
    project_id INTEGER REFERENCES projects(id),
    tool_name TEXT NOT NULL,            -- e.g., "remember", "recall", "task"
    arguments TEXT,                     -- JSON blob of tool input
    result_summary TEXT,                -- Brief summary of result (not full JSON)
    success INTEGER DEFAULT 1,          -- 1 = success, 0 = error
    duration_ms INTEGER,                -- Execution time
    created_at TEXT DEFAULT (datetime('now'))
);

-- Index for session lookups
CREATE INDEX IF NOT EXISTS idx_mcp_history_session ON mcp_history(session_id, created_at DESC);

-- Index for project-scoped queries
CREATE INDEX IF NOT EXISTS idx_mcp_history_project ON mcp_history(project_id, created_at DESC);

-- Index for tool analysis
CREATE INDEX IF NOT EXISTS idx_mcp_history_tool ON mcp_history(tool_name, created_at DESC);

-- Embeddings for semantic search of MCP interactions
CREATE TABLE IF NOT EXISTS mcp_history_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    history_id TEXT NOT NULL REFERENCES mcp_history(id) ON DELETE CASCADE,
    qdrant_point_id TEXT,               -- Qdrant vector ID
    content_hash TEXT,                  -- To detect changes
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE(history_id)
);

CREATE INDEX IF NOT EXISTS idx_mcp_embeddings_point ON mcp_history_embeddings(qdrant_point_id);
