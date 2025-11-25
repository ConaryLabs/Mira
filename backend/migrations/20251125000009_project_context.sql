-- backend/migrations/20251125_009_project_context.sql
-- Project Context: Guidelines, Tasks, Task Sessions

-- ============================================================================
-- PROJECT GUIDELINES
-- ============================================================================

CREATE TABLE IF NOT EXISTS project_guidelines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL UNIQUE,
    file_path TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    last_loaded INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_project_guidelines_project ON project_guidelines(project_id);
CREATE INDEX IF NOT EXISTS idx_project_guidelines_hash ON project_guidelines(content_hash);

-- ============================================================================
-- PROJECT TASKS
-- ============================================================================

CREATE TABLE IF NOT EXISTS project_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    parent_task_id INTEGER,
    user_id TEXT,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority INTEGER DEFAULT 0,
    complexity_estimate REAL,
    time_estimate_minutes INTEGER,
    tags TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_task_id) REFERENCES project_tasks(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_project_tasks_project ON project_tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_project_tasks_parent ON project_tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_project_tasks_user ON project_tasks(user_id);
CREATE INDEX IF NOT EXISTS idx_project_tasks_status ON project_tasks(status);
CREATE INDEX IF NOT EXISTS idx_project_tasks_priority ON project_tasks(priority);

-- ============================================================================
-- TASK SESSIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS task_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    user_id TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    progress_notes TEXT,
    files_modified TEXT,
    commits TEXT,
    FOREIGN KEY (task_id) REFERENCES project_tasks(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_sessions_task ON task_sessions(task_id);
CREATE INDEX IF NOT EXISTS idx_task_sessions_session ON task_sessions(session_id);
CREATE INDEX IF NOT EXISTS idx_task_sessions_user ON task_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_task_sessions_started ON task_sessions(started_at);

-- ============================================================================
-- TASK CONTEXT
-- ============================================================================

CREATE TABLE IF NOT EXISTS task_context (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    context_type TEXT NOT NULL,
    context_data TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES project_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_context_task ON task_context(task_id);
CREATE INDEX IF NOT EXISTS idx_task_context_type ON task_context(context_type);
