-- Session heartbeat support for zombie detection
-- Adds last_heartbeat column and session reaper support

-- Add last_heartbeat column to track session liveness
ALTER TABLE claude_sessions ADD COLUMN last_heartbeat INTEGER;

-- Create index for efficient reaper queries
CREATE INDEX IF NOT EXISTS idx_claude_sessions_heartbeat ON claude_sessions(status, last_heartbeat);
