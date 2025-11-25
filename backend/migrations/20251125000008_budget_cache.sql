-- backend/migrations/20251125_008_budget_cache.sql
-- Budget Management, LLM Caching, Reasoning Pattern Learning

-- ============================================================================
-- BUDGET TRACKING
-- ============================================================================

CREATE TABLE IF NOT EXISTS budget_tracking (
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
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_budget_tracking_user ON budget_tracking(user_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_operation ON budget_tracking(operation_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_provider ON budget_tracking(provider);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_model ON budget_tracking(model);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_timestamp ON budget_tracking(timestamp);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_from_cache ON budget_tracking(from_cache);

CREATE TABLE IF NOT EXISTS budget_summary (
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
    UNIQUE(user_id, period_type, period_start),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_budget_summary_user ON budget_summary(user_id);
CREATE INDEX IF NOT EXISTS idx_budget_summary_period ON budget_summary(period_type);
CREATE INDEX IF NOT EXISTS idx_budget_summary_start ON budget_summary(period_start);

-- ============================================================================
-- LLM CACHE
-- ============================================================================

CREATE TABLE IF NOT EXISTS llm_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key_hash TEXT NOT NULL UNIQUE,
    request_data TEXT NOT NULL,
    response TEXT NOT NULL,
    model TEXT NOT NULL,
    reasoning_effort TEXT,
    tokens_input INTEGER,
    tokens_output INTEGER,
    cost_usd REAL,
    created_at INTEGER NOT NULL,
    last_accessed INTEGER NOT NULL,
    access_count INTEGER DEFAULT 1,
    ttl_seconds INTEGER,
    expires_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_llm_cache_key ON llm_cache(key_hash);
CREATE INDEX IF NOT EXISTS idx_llm_cache_model ON llm_cache(model);
CREATE INDEX IF NOT EXISTS idx_llm_cache_created ON llm_cache(created_at);
CREATE INDEX IF NOT EXISTS idx_llm_cache_last_accessed ON llm_cache(last_accessed);
CREATE INDEX IF NOT EXISTS idx_llm_cache_expires ON llm_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_llm_cache_access_count ON llm_cache(access_count);

-- ============================================================================
-- REASONING PATTERNS (Coding Patterns)
-- ============================================================================

CREATE TABLE IF NOT EXISTS reasoning_patterns (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    trigger_type TEXT NOT NULL,
    reasoning_chain TEXT NOT NULL,
    solution_template TEXT,
    applicable_contexts TEXT,
    success_rate REAL DEFAULT 1.0,
    use_count INTEGER DEFAULT 1,
    success_count INTEGER DEFAULT 0,
    cost_savings_usd REAL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_reasoning_patterns_project ON reasoning_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_reasoning_patterns_trigger ON reasoning_patterns(trigger_type);
CREATE INDEX IF NOT EXISTS idx_reasoning_patterns_success_rate ON reasoning_patterns(success_rate);
CREATE INDEX IF NOT EXISTS idx_reasoning_patterns_use_count ON reasoning_patterns(use_count);

CREATE TABLE IF NOT EXISTS reasoning_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_id TEXT NOT NULL,
    step_number INTEGER NOT NULL,
    step_type TEXT NOT NULL,
    description TEXT NOT NULL,
    rationale TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (pattern_id) REFERENCES reasoning_patterns(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_reasoning_steps_pattern ON reasoning_steps(pattern_id);
CREATE INDEX IF NOT EXISTS idx_reasoning_steps_number ON reasoning_steps(step_number);

CREATE TABLE IF NOT EXISTS pattern_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_id TEXT NOT NULL,
    operation_id TEXT,
    user_id TEXT,
    context_match_score REAL,
    applied_successfully BOOLEAN NOT NULL,
    outcome_notes TEXT,
    time_saved_ms INTEGER,
    cost_saved_usd REAL,
    used_at INTEGER NOT NULL,
    FOREIGN KEY (pattern_id) REFERENCES reasoning_patterns(id) ON DELETE CASCADE,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pattern_usage_pattern ON pattern_usage(pattern_id);
CREATE INDEX IF NOT EXISTS idx_pattern_usage_operation ON pattern_usage(operation_id);
CREATE INDEX IF NOT EXISTS idx_pattern_usage_user ON pattern_usage(user_id);
CREATE INDEX IF NOT EXISTS idx_pattern_usage_success ON pattern_usage(applied_successfully);
CREATE INDEX IF NOT EXISTS idx_pattern_usage_used ON pattern_usage(used_at);
