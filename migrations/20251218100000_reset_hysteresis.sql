-- Add columns for reset hysteresis and state tracking
-- Prevents flappy resets and improves handoff quality

-- Track consecutive low-cache turns (for hysteresis)
ALTER TABLE chat_context ADD COLUMN consecutive_low_cache_turns INTEGER NOT NULL DEFAULT 0;

-- Track turns since last reset (for cooldown)
ALTER TABLE chat_context ADD COLUMN turns_since_reset INTEGER NOT NULL DEFAULT 0;

-- Store last failure info for handoff context
ALTER TABLE chat_context ADD COLUMN last_failure_command TEXT;
ALTER TABLE chat_context ADD COLUMN last_failure_error TEXT;
ALTER TABLE chat_context ADD COLUMN last_failure_at INTEGER;

-- Store recent artifact IDs for handoff context (JSON array)
ALTER TABLE chat_context ADD COLUMN recent_artifact_ids TEXT;
