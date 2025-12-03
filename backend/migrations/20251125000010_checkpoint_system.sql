-- migrations/20251125000010_checkpoint_system.sql
-- Checkpoint/Rewind System for file state snapshots

-- Checkpoints table - tracks each snapshot point
CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    operation_id TEXT,
    tool_name TEXT,
    description TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Index for fast session-based lookups
CREATE INDEX IF NOT EXISTS idx_checkpoints_session ON checkpoints(session_id, created_at DESC);

-- Checkpoint files - stores file content at each checkpoint
CREATE TABLE IF NOT EXISTS checkpoint_files (
    id TEXT PRIMARY KEY,
    checkpoint_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    content BLOB,
    existed INTEGER NOT NULL DEFAULT 1,
    file_hash TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (checkpoint_id) REFERENCES checkpoints(id) ON DELETE CASCADE
);

-- Index for checkpoint file lookups
CREATE INDEX IF NOT EXISTS idx_checkpoint_files_checkpoint ON checkpoint_files(checkpoint_id);
CREATE INDEX IF NOT EXISTS idx_checkpoint_files_path ON checkpoint_files(file_path);
