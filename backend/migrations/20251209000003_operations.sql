-- backend/migrations/20251209000003_operations.sql
-- Operations, Artifacts, Git Intelligence, Documents

-- ============================================================================
-- GIT INTELLIGENCE: COMMIT TRACKING
-- ============================================================================

CREATE TABLE IF NOT EXISTS git_commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    author_name TEXT NOT NULL,
    author_email TEXT NOT NULL,
    commit_message TEXT NOT NULL,
    message_summary TEXT NOT NULL,
    authored_at INTEGER NOT NULL,
    committed_at INTEGER NOT NULL,
    parent_hashes TEXT,
    file_changes TEXT NOT NULL,
    insertions INTEGER DEFAULT 0,
    deletions INTEGER DEFAULT 0,
    embedding_point_id TEXT,
    indexed_at INTEGER NOT NULL,
    UNIQUE(project_id, commit_hash),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_git_commits_project ON git_commits(project_id);
CREATE INDEX IF NOT EXISTS idx_git_commits_hash ON git_commits(commit_hash);
CREATE INDEX IF NOT EXISTS idx_git_commits_author_email ON git_commits(author_email);
CREATE INDEX IF NOT EXISTS idx_git_commits_authored_at ON git_commits(authored_at);

-- ============================================================================
-- GIT INTELLIGENCE: CO-CHANGE PATTERNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS file_cochange_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path_a TEXT NOT NULL,
    file_path_b TEXT NOT NULL,
    cochange_count INTEGER NOT NULL,
    total_changes_a INTEGER NOT NULL,
    total_changes_b INTEGER NOT NULL,
    confidence_score REAL NOT NULL,
    last_cochange INTEGER NOT NULL,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, file_path_a, file_path_b),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cochange_project ON file_cochange_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_cochange_file_a ON file_cochange_patterns(file_path_a);
CREATE INDEX IF NOT EXISTS idx_cochange_file_b ON file_cochange_patterns(file_path_b);
CREATE INDEX IF NOT EXISTS idx_cochange_confidence ON file_cochange_patterns(confidence_score);

-- ============================================================================
-- GIT INTELLIGENCE: BLAME
-- ============================================================================

CREATE TABLE IF NOT EXISTS blame_annotations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line_number INTEGER NOT NULL,
    commit_hash TEXT NOT NULL,
    author_name TEXT NOT NULL,
    author_email TEXT NOT NULL,
    authored_at INTEGER NOT NULL,
    line_content TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(project_id, file_path, line_number, file_hash),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_blame_project ON blame_annotations(project_id);
CREATE INDEX IF NOT EXISTS idx_blame_file ON blame_annotations(file_path);
CREATE INDEX IF NOT EXISTS idx_blame_commit ON blame_annotations(commit_hash);
CREATE INDEX IF NOT EXISTS idx_blame_author ON blame_annotations(author_email);

-- ============================================================================
-- GIT INTELLIGENCE: AUTHOR EXPERTISE
-- ============================================================================

CREATE TABLE IF NOT EXISTS author_expertise (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    author_email TEXT NOT NULL,
    author_name TEXT NOT NULL,
    file_pattern TEXT NOT NULL,
    domain TEXT,
    commit_count INTEGER NOT NULL,
    line_count INTEGER NOT NULL,
    last_contribution INTEGER NOT NULL,
    first_contribution INTEGER NOT NULL,
    expertise_score REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, author_email, file_pattern),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_author_expertise_project ON author_expertise(project_id);
CREATE INDEX IF NOT EXISTS idx_author_expertise_email ON author_expertise(author_email);
CREATE INDEX IF NOT EXISTS idx_author_expertise_pattern ON author_expertise(file_pattern);
CREATE INDEX IF NOT EXISTS idx_author_expertise_domain ON author_expertise(domain);
CREATE INDEX IF NOT EXISTS idx_author_expertise_score ON author_expertise(expertise_score);

-- ============================================================================
-- GIT INTELLIGENCE: HISTORICAL FIXES
-- ============================================================================

CREATE TABLE IF NOT EXISTS historical_fixes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    error_pattern TEXT NOT NULL,
    error_category TEXT NOT NULL,
    fix_commit_hash TEXT NOT NULL,
    files_modified TEXT NOT NULL,
    fix_description TEXT,
    fixed_at INTEGER NOT NULL,
    similarity_hash TEXT NOT NULL,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_historical_fixes_project ON historical_fixes(project_id);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_error_pattern ON historical_fixes(error_pattern);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_category ON historical_fixes(error_category);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_commit ON historical_fixes(fix_commit_hash);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_similarity ON historical_fixes(similarity_hash);

-- ============================================================================
-- OPERATIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    user_id TEXT,
    project_id TEXT,
    parent_operation_id TEXT,
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

-- ============================================================================
-- DOCUMENTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    user_id TEXT,
    original_name TEXT,
    file_name TEXT,
    file_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    size_bytes INTEGER,
    file_hash TEXT,
    content_hash TEXT,
    metadata TEXT,
    page_count INTEGER,
    chunk_count INTEGER DEFAULT 0,
    status TEXT DEFAULT 'pending',
    processing_started_at INTEGER,
    processing_completed_at INTEGER,
    uploaded_at INTEGER DEFAULT (strftime('%s', 'now')),
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_documents_user ON documents(user_id);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(file_hash);
CREATE INDEX IF NOT EXISTS idx_documents_type ON documents(file_type);

-- ============================================================================
-- DOCUMENT CHUNKS
-- ============================================================================

CREATE TABLE IF NOT EXISTS document_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    page_number INTEGER,
    char_start INTEGER,
    char_end INTEGER,
    qdrant_point_id TEXT,
    embedding_point_id TEXT,
    collection_name TEXT DEFAULT 'conversation',
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    UNIQUE(document_id, chunk_index),
    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_document_chunks_document ON document_chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_document_chunks_index ON document_chunks(chunk_index);
CREATE INDEX IF NOT EXISTS idx_document_chunks_point ON document_chunks(embedding_point_id);
CREATE INDEX IF NOT EXISTS idx_document_chunks_qdrant_point ON document_chunks(qdrant_point_id);

-- ============================================================================
-- PROJECT CONTEXT: GUIDELINES
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
-- PROJECT CONTEXT: TASKS
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
-- PROJECT CONTEXT: TASK SESSIONS
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
-- PROJECT CONTEXT: TASK CONTEXT
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
