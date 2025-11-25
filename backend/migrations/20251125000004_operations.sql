-- backend/migrations/20251125_004_operations.sql
-- Operations & Workflow: Operations, Artifacts, Tasks, File Modifications

-- ============================================================================
-- OPERATIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    user_id TEXT,
    project_id TEXT,
    parent_operation_id TEXT,
    kind TEXT,
    operation_kind TEXT DEFAULT '',
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    user_message TEXT,
    delegate_calls INTEGER DEFAULT 0,
    plan_text TEXT,
    plan_generated_at INTEGER,
    planning_tokens_reasoning INTEGER,
    result TEXT,
    error TEXT,
    complexity_score REAL,
    model_used TEXT,
    reasoning_effort TEXT,
    tokens_input INTEGER,
    tokens_output INTEGER,
    cost REAL,
    error_message TEXT,
    result_summary TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    updated_at INTEGER,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL,
    FOREIGN KEY (parent_operation_id) REFERENCES operations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_operations_session ON operations(session_id);
CREATE INDEX IF NOT EXISTS idx_operations_user ON operations(user_id);
CREATE INDEX IF NOT EXISTS idx_operations_project ON operations(project_id);
CREATE INDEX IF NOT EXISTS idx_operations_status ON operations(status);
CREATE INDEX IF NOT EXISTS idx_operations_kind ON operations(operation_kind);
CREATE INDEX IF NOT EXISTS idx_operations_parent ON operations(parent_operation_id);
CREATE INDEX IF NOT EXISTS idx_operations_created ON operations(created_at);

-- ============================================================================
-- OPERATION EVENTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS operation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    sequence_number INTEGER,
    data TEXT,
    event_data TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_operation_events_operation ON operation_events(operation_id);
CREATE INDEX IF NOT EXISTS idx_operation_events_type ON operation_events(event_type);
CREATE INDEX IF NOT EXISTS idx_operation_events_created ON operation_events(created_at);

-- ============================================================================
-- OPERATION TASKS
-- ============================================================================

CREATE TABLE IF NOT EXISTS operation_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL,
    parent_task_id INTEGER,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    priority INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_task_id) REFERENCES operation_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_operation_tasks_operation ON operation_tasks(operation_id);
CREATE INDEX IF NOT EXISTS idx_operation_tasks_parent ON operation_tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_operation_tasks_status ON operation_tasks(status);
CREATE INDEX IF NOT EXISTS idx_operation_tasks_priority ON operation_tasks(priority);

-- ============================================================================
-- ARTIFACTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    session_id TEXT,
    user_id TEXT,
    project_id TEXT,
    kind TEXT NOT NULL,
    file_path TEXT,
    language TEXT,
    title TEXT,
    content TEXT NOT NULL,
    content_hash TEXT,
    original_content TEXT,
    diff TEXT,
    diff_from_previous TEXT,
    dependencies TEXT,
    context TEXT,
    applied BOOLEAN DEFAULT FALSE,
    applied_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_artifacts_operation ON artifacts(operation_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_user ON artifacts(user_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_project ON artifacts(project_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_kind ON artifacts(kind);
CREATE INDEX IF NOT EXISTS idx_artifacts_file_path ON artifacts(file_path);
CREATE INDEX IF NOT EXISTS idx_artifacts_applied ON artifacts(applied);

-- ============================================================================
-- FILE MODIFICATIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS file_modifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    artifact_id TEXT,
    file_path TEXT NOT NULL,
    original_content TEXT,
    modified_content TEXT,
    modification_time INTEGER DEFAULT (strftime('%s', 'now')),
    reverted INTEGER DEFAULT 0,
    diff TEXT,
    created_by TEXT,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_file_modifications_project ON file_modifications(project_id);
CREATE INDEX IF NOT EXISTS idx_file_modifications_artifact ON file_modifications(artifact_id);
CREATE INDEX IF NOT EXISTS idx_file_modifications_file ON file_modifications(file_path);
CREATE INDEX IF NOT EXISTS idx_file_modifications_reverted ON file_modifications(reverted);
CREATE INDEX IF NOT EXISTS idx_file_modifications_time ON file_modifications(modification_time);

-- ============================================================================
-- TERMINAL SESSIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS terminal_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    conversation_session_id TEXT,
    working_directory TEXT NOT NULL,
    shell TEXT,
    created_at INTEGER NOT NULL,
    closed_at INTEGER,
    exit_code INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_terminal_sessions_project ON terminal_sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_conversation ON terminal_sessions(conversation_session_id);

CREATE TABLE IF NOT EXISTS terminal_commands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    command TEXT NOT NULL,
    output TEXT,
    exit_code INTEGER,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    FOREIGN KEY (session_id) REFERENCES terminal_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_terminal_commands_session ON terminal_commands(session_id);
