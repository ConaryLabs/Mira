-- Instruction queue for Studio -> Claude Code communication
-- Studio queues instructions, Claude Code polls and executes them

CREATE TABLE IF NOT EXISTS instruction_queue (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    instruction TEXT NOT NULL,
    context TEXT,  -- Optional additional context/reasoning
    priority TEXT DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'in_progress', 'completed', 'failed', 'cancelled')),
    created_at TEXT DEFAULT (datetime('now')),
    delivered_at TEXT,  -- When Claude Code picked it up
    started_at TEXT,    -- When Claude Code started working on it
    completed_at TEXT,  -- When Claude Code finished
    result TEXT,        -- Outcome/notes from Claude Code
    error TEXT          -- Error message if failed
);

-- Index for polling pending instructions
CREATE INDEX IF NOT EXISTS idx_instruction_queue_pending
ON instruction_queue(project_id, status, priority, created_at)
WHERE status = 'pending';

-- Index for listing by project
CREATE INDEX IF NOT EXISTS idx_instruction_queue_project
ON instruction_queue(project_id, created_at DESC);
