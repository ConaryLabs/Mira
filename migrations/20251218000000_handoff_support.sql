-- Add handoff support for smooth conversation resets
-- When previous_response_id is cleared, next turn gets a handoff blob

-- Track whether next request needs handoff context
ALTER TABLE chat_context ADD COLUMN needs_handoff INTEGER NOT NULL DEFAULT 0;

-- Store pre-computed handoff blob (generated at reset time)
ALTER TABLE chat_context ADD COLUMN handoff_blob TEXT;
