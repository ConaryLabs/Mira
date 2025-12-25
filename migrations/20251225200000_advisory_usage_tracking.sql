-- Add detailed usage tracking to advisory messages
-- Stores per-message token counts including cache hits for accurate cost calculation

ALTER TABLE advisory_messages ADD COLUMN usage_json TEXT;

-- Index for aggregating usage across sessions
CREATE INDEX IF NOT EXISTS idx_advisory_messages_usage ON advisory_messages(session_id, provider) WHERE usage_json IS NOT NULL;
