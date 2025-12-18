-- Add chain and tool tracking to usage for better "what happened" analysis

-- Track response chain (for detecting resets)
ALTER TABLE chat_usage ADD COLUMN response_id TEXT;
ALTER TABLE chat_usage ADD COLUMN previous_response_id TEXT;

-- Track tool usage per turn
ALTER TABLE chat_usage ADD COLUMN tool_count INTEGER DEFAULT 0;
ALTER TABLE chat_usage ADD COLUMN tool_names TEXT;  -- comma-separated list
