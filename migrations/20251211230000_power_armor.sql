-- Power Armor Upgrade: Proactive Intelligence Layer for Mira
-- Adds corrections tracking, goal management, and proactive context delivery

-- ============================================================================
-- CORRECTIONS: Learn from user corrections to avoid repeated mistakes
-- ============================================================================

CREATE TABLE IF NOT EXISTS corrections (
    id TEXT PRIMARY KEY,
    correction_type TEXT NOT NULL,              -- 'style', 'approach', 'pattern', 'preference', 'anti_pattern'
    what_was_wrong TEXT NOT NULL,               -- What Claude did wrong
    what_is_right TEXT NOT NULL,                -- What Claude should do instead
    rationale TEXT,                             -- Why this is the right approach
    scope TEXT NOT NULL DEFAULT 'project',      -- 'global', 'project', 'file', 'topic'
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    file_patterns TEXT,                         -- JSON array of file patterns (e.g., ["*.rs", "src/auth/*"])
    topic_tags TEXT,                            -- JSON array of topic tags (e.g., ["authentication", "error-handling"])
    keywords TEXT,                              -- JSON array of trigger keywords
    confidence REAL DEFAULT 1.0,                -- 0.0-1.0, decays if not validated
    times_applied INTEGER DEFAULT 0,            -- How often this correction was surfaced
    times_validated INTEGER DEFAULT 0,          -- How often user confirmed it was helpful
    status TEXT DEFAULT 'active',               -- 'active', 'deprecated', 'superseded'
    superseded_by TEXT REFERENCES corrections(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_corrections_project ON corrections(project_id);
CREATE INDEX IF NOT EXISTS idx_corrections_type ON corrections(correction_type);
CREATE INDEX IF NOT EXISTS idx_corrections_scope ON corrections(scope);
CREATE INDEX IF NOT EXISTS idx_corrections_status ON corrections(status);

-- Track when corrections are applied and outcomes
CREATE TABLE IF NOT EXISTS correction_applications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    correction_id TEXT NOT NULL REFERENCES corrections(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL,                      -- 'applied', 'ignored', 'validated', 'overridden'
    file_path TEXT,                             -- Context: which file
    task_context TEXT,                          -- Context: what task
    applied_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_correction_apps_correction ON correction_applications(correction_id);
CREATE INDEX IF NOT EXISTS idx_correction_apps_outcome ON correction_applications(outcome);

-- ============================================================================
-- GOALS: High-level objectives spanning multiple sessions
-- ============================================================================

CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    success_criteria TEXT,                      -- What does "done" look like?
    status TEXT NOT NULL DEFAULT 'planning',    -- 'planning', 'in_progress', 'blocked', 'completed', 'abandoned'
    priority TEXT DEFAULT 'medium',             -- 'low', 'medium', 'high', 'critical'
    progress_percent INTEGER DEFAULT 0,         -- 0-100
    progress_mode TEXT DEFAULT 'auto',          -- 'auto' (calculated from milestones) or 'manual'
    blockers TEXT,                              -- JSON array of blocker descriptions
    notes TEXT,                                 -- Free-form notes
    tags TEXT,                                  -- JSON array of tags
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    started_at INTEGER,                         -- When work began
    target_date INTEGER,                        -- Optional deadline
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_goals_project ON goals(project_id);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_priority ON goals(priority);

-- ============================================================================
-- MILESTONES: Checkpoints within goals
-- ============================================================================

CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',     -- 'pending', 'in_progress', 'completed', 'skipped'
    weight INTEGER DEFAULT 1,                   -- Relative weight for progress calculation
    order_index INTEGER DEFAULT 0,              -- For ordering within goal
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_milestones_goal ON milestones(goal_id);
CREATE INDEX IF NOT EXISTS idx_milestones_status ON milestones(status);

-- ============================================================================
-- REJECTED APPROACHES: Track what was tried and why it failed
-- ============================================================================

CREATE TABLE IF NOT EXISTS rejected_approaches (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    problem_context TEXT NOT NULL,              -- What problem this was trying to solve
    approach TEXT NOT NULL,                     -- What was tried
    rejection_reason TEXT NOT NULL,             -- Why it was rejected
    related_files TEXT,                         -- JSON array of file paths
    related_topics TEXT,                        -- JSON array of topics
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_rejected_project ON rejected_approaches(project_id);

-- ============================================================================
-- TASK EXTENSIONS: Link tasks to goals and milestones
-- ============================================================================

-- Add goal/milestone linking to existing tasks table
-- Using ALTER TABLE with IF NOT EXISTS pattern for SQLite compatibility
CREATE TABLE IF NOT EXISTS _tasks_migration_check (migrated INTEGER);
INSERT OR IGNORE INTO _tasks_migration_check VALUES (0);

-- Only add columns if they don't exist (SQLite doesn't have IF NOT EXISTS for ALTER TABLE)
-- We'll handle this in code by catching the "duplicate column" error
