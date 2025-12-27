-- Add session_id to instruction_queue for tracking which session executes an instruction
ALTER TABLE instruction_queue ADD COLUMN session_id TEXT;

-- Index for looking up instructions by session
CREATE INDEX IF NOT EXISTS idx_instruction_queue_session ON instruction_queue(session_id);
