-- Migration: Add planning mode support to operations
-- Enables two-phase execution: plan generation → execution
-- Tracks planning phase separately from execution phase

-- Add planning fields to operations table
ALTER TABLE operations ADD COLUMN plan_text TEXT;
ALTER TABLE operations ADD COLUMN plan_generated_at INTEGER;
ALTER TABLE operations ADD COLUMN planning_tokens_input INTEGER DEFAULT 0;
ALTER TABLE operations ADD COLUMN planning_tokens_output INTEGER DEFAULT 0;
ALTER TABLE operations ADD COLUMN planning_tokens_reasoning INTEGER DEFAULT 0;

-- Index for querying operations with plans
CREATE INDEX IF NOT EXISTS idx_operations_plan_generated
    ON operations(plan_generated_at)
    WHERE plan_generated_at IS NOT NULL;
