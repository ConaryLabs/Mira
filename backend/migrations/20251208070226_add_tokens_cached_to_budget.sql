-- backend/migrations/20251208070226_add_tokens_cached_to_budget.sql
-- Add tokens_cached column to track OpenAI cached tokens for cost optimization

-- Add tokens_cached column to budget_tracking table
-- This tracks how many input tokens were cached by OpenAI (90% discount)
ALTER TABLE budget_tracking ADD COLUMN tokens_cached INTEGER DEFAULT 0;

-- Create index for analyzing cache effectiveness
CREATE INDEX IF NOT EXISTS idx_budget_tracking_cached ON budget_tracking(tokens_cached);
