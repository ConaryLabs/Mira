-- backend/migrations/20251125_007_build_system.sql
-- Build System Integration: Build Runs, Error Tracking, Resolution Learning

-- ============================================================================
-- BUILD RUNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS build_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    operation_id TEXT,
    build_type TEXT NOT NULL,
    command TEXT NOT NULL,
    exit_code INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    completed_at INTEGER NOT NULL,
    error_count INTEGER DEFAULT 0,
    warning_count INTEGER DEFAULT 0,
    triggered_by TEXT,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_build_runs_project ON build_runs(project_id);
CREATE INDEX IF NOT EXISTS idx_build_runs_operation ON build_runs(operation_id);
CREATE INDEX IF NOT EXISTS idx_build_runs_type ON build_runs(build_type);
CREATE INDEX IF NOT EXISTS idx_build_runs_exit_code ON build_runs(exit_code);
CREATE INDEX IF NOT EXISTS idx_build_runs_started ON build_runs(started_at);

-- ============================================================================
-- BUILD ERRORS
-- ============================================================================

CREATE TABLE IF NOT EXISTS build_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_run_id INTEGER NOT NULL,
    error_hash TEXT NOT NULL,
    severity TEXT NOT NULL,
    error_code TEXT,
    message TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    column_number INTEGER,
    suggestion TEXT,
    code_snippet TEXT,
    category TEXT,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    occurrence_count INTEGER DEFAULT 1,
    resolved_at INTEGER,
    FOREIGN KEY (build_run_id) REFERENCES build_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_build_errors_run ON build_errors(build_run_id);
CREATE INDEX IF NOT EXISTS idx_build_errors_hash ON build_errors(error_hash);
CREATE INDEX IF NOT EXISTS idx_build_errors_severity ON build_errors(severity);
CREATE INDEX IF NOT EXISTS idx_build_errors_file ON build_errors(file_path);
CREATE INDEX IF NOT EXISTS idx_build_errors_category ON build_errors(category);
CREATE INDEX IF NOT EXISTS idx_build_errors_resolved ON build_errors(resolved_at);

-- ============================================================================
-- ERROR RESOLUTIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS error_resolutions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    error_hash TEXT NOT NULL,
    resolution_type TEXT NOT NULL,
    files_changed TEXT,
    commit_hash TEXT,
    resolution_time_ms INTEGER,
    resolved_at INTEGER NOT NULL,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_error_resolutions_hash ON error_resolutions(error_hash);
CREATE INDEX IF NOT EXISTS idx_error_resolutions_type ON error_resolutions(resolution_type);
CREATE INDEX IF NOT EXISTS idx_error_resolutions_commit ON error_resolutions(commit_hash);
CREATE INDEX IF NOT EXISTS idx_error_resolutions_resolved ON error_resolutions(resolved_at);

-- ============================================================================
-- BUILD CONTEXT INJECTIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS build_context_injections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL,
    build_run_id INTEGER NOT NULL,
    error_ids TEXT NOT NULL,
    injected_at INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE,
    FOREIGN KEY (build_run_id) REFERENCES build_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_build_context_operation ON build_context_injections(operation_id);
CREATE INDEX IF NOT EXISTS idx_build_context_run ON build_context_injections(build_run_id);
CREATE INDEX IF NOT EXISTS idx_build_context_injected ON build_context_injections(injected_at);
