-- backend/migrations/20251125000013_remove_budget_user_fk.sql
-- Remove FK constraint on budget_tracking.user_id
-- The user_id field now stores session_id (like "{username}-eternal") which doesn't map to users table

-- SQLite doesn't support DROP FOREIGN KEY, so we recreate the table

-- Create new table without user_id FK (keep operation_id FK)
CREATE TABLE IF NOT EXISTS budget_tracking_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    operation_id TEXT,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    reasoning_effort TEXT,
    tokens_input INTEGER,
    tokens_output INTEGER,
    cost_usd REAL NOT NULL,
    from_cache BOOLEAN DEFAULT FALSE,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL
);

-- Copy existing data
INSERT INTO budget_tracking_new
SELECT * FROM budget_tracking;

-- Drop old table
DROP TABLE budget_tracking;

-- Rename new table
ALTER TABLE budget_tracking_new RENAME TO budget_tracking;

-- Recreate indexes
CREATE INDEX IF NOT EXISTS idx_budget_tracking_user ON budget_tracking(user_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_operation ON budget_tracking(operation_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_provider ON budget_tracking(provider);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_model ON budget_tracking(model);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_timestamp ON budget_tracking(timestamp);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_from_cache ON budget_tracking(from_cache);

-- Also fix budget_summary FK for consistency
CREATE TABLE IF NOT EXISTS budget_summary_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    period_type TEXT NOT NULL,
    period_start INTEGER NOT NULL,
    period_end INTEGER NOT NULL,
    total_requests INTEGER NOT NULL,
    cached_requests INTEGER NOT NULL,
    total_tokens_input INTEGER NOT NULL,
    total_tokens_output INTEGER NOT NULL,
    total_cost_usd REAL NOT NULL,
    cache_hit_rate REAL NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(user_id, period_type, period_start)
);

-- Copy existing data
INSERT INTO budget_summary_new
SELECT * FROM budget_summary;

-- Drop old table
DROP TABLE budget_summary;

-- Rename new table
ALTER TABLE budget_summary_new RENAME TO budget_summary;

-- Recreate indexes
CREATE INDEX IF NOT EXISTS idx_budget_summary_user ON budget_summary(user_id);
CREATE INDEX IF NOT EXISTS idx_budget_summary_period ON budget_summary(period_type);
CREATE INDEX IF NOT EXISTS idx_budget_summary_start ON budget_summary(period_start);
