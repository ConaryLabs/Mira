-- Migration: Add operation_tasks table for task tracking
-- Enables decomposition of operations into trackable sub-tasks
-- Provides visibility into multi-step operation progress

CREATE TABLE IF NOT EXISTS operation_tasks (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    description TEXT NOT NULL,
    active_form TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    error_message TEXT,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

-- Index for querying tasks by operation
CREATE INDEX IF NOT EXISTS idx_operation_tasks_operation_id
    ON operation_tasks(operation_id, sequence);

-- Index for querying tasks by status
CREATE INDEX IF NOT EXISTS idx_operation_tasks_status
    ON operation_tasks(status);

-- Index for querying in-progress tasks
CREATE INDEX IF NOT EXISTS idx_operation_tasks_in_progress
    ON operation_tasks(operation_id, status)
    WHERE status = 'in_progress';
