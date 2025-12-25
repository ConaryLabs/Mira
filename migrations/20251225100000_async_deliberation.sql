-- Async deliberation support
-- Adds progress tracking for background council deliberation

-- Add deliberation progress column to track async council deliberation
ALTER TABLE advisory_sessions ADD COLUMN deliberation_progress TEXT;
-- JSON format:
-- {
--   "current_round": 2,
--   "max_rounds": 4,
--   "status": "round_in_progress" | "moderator_analyzing" | "synthesizing" | "complete" | "failed",
--   "models_responded": ["gpt-5.2", "gemini-3-pro"],
--   "error": null,
--   "started_at": 1735100000,
--   "result": null  -- Final DeliberatedSynthesis when complete
-- }

-- Index for finding active deliberations
CREATE INDEX IF NOT EXISTS idx_advisory_sessions_deliberating
ON advisory_sessions(status) WHERE status = 'deliberating';
