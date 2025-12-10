-- backend/migrations/20251209000004_infrastructure.sql
-- Infrastructure: Build System, Budget, Caching, Tools, Sudo, Checkpoints

-- ============================================================================
-- BUILD SYSTEM: BUILD RUNS
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
-- BUILD SYSTEM: BUILD ERRORS
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
-- BUILD SYSTEM: ERROR RESOLUTIONS
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
-- BUILD SYSTEM: CONTEXT INJECTIONS
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

-- ============================================================================
-- BUDGET TRACKING (No user FK - uses session_id like "{username}-eternal")
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
    tokens_cached INTEGER DEFAULT 0,
    cost_usd REAL NOT NULL,
    from_cache BOOLEAN DEFAULT FALSE,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_budget_tracking_user ON budget_tracking(user_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_operation ON budget_tracking(operation_id);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_provider ON budget_tracking(provider);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_model ON budget_tracking(model);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_timestamp ON budget_tracking(timestamp);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_from_cache ON budget_tracking(from_cache);
CREATE INDEX IF NOT EXISTS idx_budget_tracking_cached ON budget_tracking(tokens_cached);

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
    UNIQUE(user_id, period_type, period_start)
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
-- SESSION CACHE STATE (LLM-side prompt caching optimization)
-- ============================================================================

CREATE TABLE IF NOT EXISTS session_cache_state (
    session_id TEXT PRIMARY KEY,
    static_prefix_hash TEXT NOT NULL,
    last_call_at INTEGER NOT NULL,
    project_context_hash TEXT,
    memory_context_hash TEXT,
    code_intelligence_hash TEXT,
    file_context_hash TEXT,
    static_prefix_tokens INTEGER DEFAULT 0,
    last_cached_tokens INTEGER DEFAULT 0,
    total_requests INTEGER DEFAULT 0,
    total_cached_tokens INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_cache_state_last_call ON session_cache_state(last_call_at);

CREATE TABLE IF NOT EXISTS session_file_hashes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    sent_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session_cache_state(session_id) ON DELETE CASCADE,
    UNIQUE(session_id, file_path)
);

CREATE INDEX IF NOT EXISTS idx_session_file_hashes_session ON session_file_hashes(session_id);

-- ============================================================================
-- REASONING PATTERNS
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

-- ============================================================================
-- TOOL SYNTHESIS: PATTERNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    pattern_name TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    description TEXT NOT NULL,
    detected_occurrences INTEGER NOT NULL DEFAULT 1,
    example_locations TEXT NOT NULL,
    confidence_score REAL NOT NULL DEFAULT 0.0,
    should_synthesize BOOLEAN DEFAULT FALSE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_patterns_project ON tool_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_name ON tool_patterns(pattern_name);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_type ON tool_patterns(pattern_type);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_confidence ON tool_patterns(confidence_score);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_should_synthesize ON tool_patterns(should_synthesize);

-- ============================================================================
-- TOOL SYNTHESIS: SYNTHESIZED TOOLS
-- ============================================================================

CREATE TABLE IF NOT EXISTS synthesized_tools (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    tool_pattern_id INTEGER,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    source_code TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'rust',
    compilation_status TEXT NOT NULL DEFAULT 'pending',
    compilation_error TEXT,
    binary_path TEXT,
    enabled BOOLEAN DEFAULT TRUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (tool_pattern_id) REFERENCES tool_patterns(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_synthesized_tools_project ON synthesized_tools(project_id);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_pattern ON synthesized_tools(tool_pattern_id);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_name ON synthesized_tools(name);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_status ON synthesized_tools(compilation_status);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_enabled ON synthesized_tools(enabled);

-- ============================================================================
-- TOOL SYNTHESIS: EXECUTIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    operation_id TEXT,
    session_id TEXT NOT NULL,
    user_id TEXT,
    arguments TEXT,
    success BOOLEAN NOT NULL,
    output TEXT,
    error_message TEXT,
    duration_ms INTEGER NOT NULL,
    executed_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_executions_tool ON tool_executions(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_operation ON tool_executions(operation_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_session ON tool_executions(session_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_user ON tool_executions(user_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_success ON tool_executions(success);
CREATE INDEX IF NOT EXISTS idx_tool_executions_executed ON tool_executions(executed_at);

-- ============================================================================
-- TOOL SYNTHESIS: EFFECTIVENESS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_effectiveness (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL UNIQUE,
    total_executions INTEGER NOT NULL DEFAULT 0,
    successful_executions INTEGER NOT NULL DEFAULT 0,
    failed_executions INTEGER NOT NULL DEFAULT 0,
    average_duration_ms REAL,
    total_time_saved_ms INTEGER DEFAULT 0,
    last_executed INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_effectiveness_tool ON tool_effectiveness(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_effectiveness_success_rate ON tool_effectiveness(successful_executions);

-- ============================================================================
-- TOOL SYNTHESIS: FEEDBACK
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    execution_id INTEGER,
    user_id TEXT NOT NULL,
    rating INTEGER,
    comment TEXT,
    issue_type TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_id) REFERENCES tool_executions(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_feedback_tool ON tool_feedback(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_execution ON tool_feedback(execution_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_user ON tool_feedback(user_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_rating ON tool_feedback(rating);

-- ============================================================================
-- TOOL SYNTHESIS: EVOLUTION
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_evolution_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    old_version INTEGER NOT NULL,
    new_version INTEGER NOT NULL,
    change_description TEXT NOT NULL,
    motivation TEXT,
    source_code_diff TEXT,
    evolved_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_evolution_tool ON tool_evolution_history(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_old_version ON tool_evolution_history(old_version);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_new_version ON tool_evolution_history(new_version);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_evolved ON tool_evolution_history(evolved_at);

-- ============================================================================
-- CHECKPOINTS (File state snapshots for rewind)
-- ============================================================================

CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    operation_id TEXT,
    tool_name TEXT,
    description TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_session ON checkpoints(session_id, created_at DESC);

CREATE TABLE IF NOT EXISTS checkpoint_files (
    id TEXT PRIMARY KEY,
    checkpoint_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    content BLOB,
    existed INTEGER NOT NULL DEFAULT 1,
    file_hash TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (checkpoint_id) REFERENCES checkpoints(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_checkpoint_files_checkpoint ON checkpoint_files(checkpoint_id);
CREATE INDEX IF NOT EXISTS idx_checkpoint_files_path ON checkpoint_files(file_path);

-- ============================================================================
-- SUDO SYSTEM: PERMISSIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    command_exact TEXT,
    command_pattern TEXT,
    command_prefix TEXT,
    requires_approval INTEGER NOT NULL DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    created_by TEXT,
    last_used_at INTEGER,
    use_count INTEGER NOT NULL DEFAULT 0,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_sudo_permissions_enabled ON sudo_permissions(enabled);
CREATE INDEX IF NOT EXISTS idx_sudo_permissions_name ON sudo_permissions(name);

-- ============================================================================
-- SUDO SYSTEM: BLOCKLIST
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_blocklist (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    pattern_exact TEXT,
    pattern_regex TEXT,
    pattern_prefix TEXT,
    severity TEXT NOT NULL DEFAULT 'high',
    is_default INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_sudo_blocklist_enabled ON sudo_blocklist(enabled);
CREATE INDEX IF NOT EXISTS idx_sudo_blocklist_severity ON sudo_blocklist(severity);

-- ============================================================================
-- SUDO SYSTEM: APPROVAL REQUESTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_approval_requests (
    id TEXT PRIMARY KEY,
    command TEXT NOT NULL,
    working_dir TEXT,
    operation_id TEXT,
    session_id TEXT,
    permission_id INTEGER,
    requested_by TEXT NOT NULL DEFAULT 'llm',
    reason TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    expires_at INTEGER,
    responded_at INTEGER,
    approved_by TEXT,
    denial_reason TEXT,
    executed_at INTEGER,
    exit_code INTEGER,
    output TEXT,
    error TEXT,
    FOREIGN KEY (permission_id) REFERENCES sudo_permissions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_sudo_approval_session ON sudo_approval_requests(session_id);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_status ON sudo_approval_requests(status);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_operation ON sudo_approval_requests(operation_id);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_expires ON sudo_approval_requests(expires_at);

-- ============================================================================
-- SUDO SYSTEM: AUDIT LOG
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command TEXT NOT NULL,
    working_dir TEXT,
    permission_id INTEGER,
    approval_request_id TEXT,
    authorization_type TEXT NOT NULL,
    operation_id TEXT,
    session_id TEXT,
    executed_by TEXT NOT NULL,
    started_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    completed_at INTEGER,
    exit_code INTEGER,
    stdout TEXT,
    stderr TEXT,
    success INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    FOREIGN KEY (permission_id) REFERENCES sudo_permissions(id) ON DELETE SET NULL,
    FOREIGN KEY (approval_request_id) REFERENCES sudo_approval_requests(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_sudo_audit_session ON sudo_audit_log(session_id);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_operation ON sudo_audit_log(operation_id);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_started ON sudo_audit_log(started_at);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_success ON sudo_audit_log(success);

-- ============================================================================
-- SUDO SYSTEM: DEFAULT BLOCKLIST
-- ============================================================================

INSERT INTO sudo_blocklist (name, pattern_regex, severity, is_default, description) VALUES
    ('Recursive root deletion', 'rm\s+(-rf|-fr|-r\s+-f|-f\s+-r)\s+/', 'critical', 1, 'Catastrophic file deletion from root'),
    ('DD to disk device', 'dd\s+.*of=/dev/[a-z]+$', 'critical', 1, 'Overwrite disk device'),
    ('DD to partition', 'dd\s+.*of=/dev/[a-z]+[0-9]', 'high', 1, 'Write to raw partition'),
    ('Format filesystem', 'mkfs', 'critical', 1, 'Format filesystem'),
    ('Destructive fdisk', 'fdisk.*-w', 'critical', 1, 'Destructive partition changes'),
    ('Curl pipe to shell', 'curl\s+.*\|\s*(ba)?sh', 'high', 1, 'Remote code execution via curl'),
    ('Wget pipe to shell', 'wget\s+.*\|\s*(ba)?sh', 'high', 1, 'Remote code execution via wget'),
    ('Fork bomb', ':\(\)\{:\|:&\};:', 'critical', 1, 'Fork bomb denial of service'),
    ('Overwrite MBR', 'dd.*of=/dev/[sh]da$', 'critical', 1, 'Overwrite boot sector'),
    ('Chmod 777 root', 'chmod\s+(777|a\+rwx)\s+/', 'high', 1, 'Insecure root permissions'),
    ('Delete boot', 'rm\s+.*(/boot|/etc/passwd|/etc/shadow)', 'critical', 1, 'Delete critical system files');

-- ============================================================================
-- SUDO SYSTEM: DEFAULT PERMISSIONS
-- ============================================================================

INSERT INTO sudo_permissions (name, command_prefix, requires_approval, description) VALUES
    ('Mira service restart', 'systemctl restart mira-', 0, 'Restart Mira services without approval'),
    ('Mira service status', 'systemctl status mira-', 0, 'Check Mira service status without approval'),
    ('Mira service stop', 'systemctl stop mira-', 0, 'Stop Mira services without approval'),
    ('Mira service start', 'systemctl start mira-', 0, 'Start Mira services without approval'),
    ('Apt update', 'apt update', 0, 'Update package lists without approval'),
    ('Apt upgrade', 'apt upgrade', 1, 'Upgrade packages (requires approval)'),
    ('Apt install', 'apt install ', 1, 'Install packages (requires approval)'),
    ('Systemctl restart', 'systemctl restart ', 1, 'Restart any service (requires approval)'),
    ('Systemctl enable', 'systemctl enable ', 1, 'Enable services (requires approval)'),
    ('Systemctl disable', 'systemctl disable ', 1, 'Disable services (requires approval)');
