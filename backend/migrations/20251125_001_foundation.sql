-- backend/migrations/20251125_001_foundation.sql
-- Foundation Tables: Users, Authentication, Projects, Memory, Personal Context

-- ============================================================================
-- USER & AUTHENTICATION
-- ============================================================================

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_login INTEGER
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    last_active INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token);
CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);

CREATE TABLE IF NOT EXISTS user_profile (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL UNIQUE,
    coding_preferences TEXT,
    communication_style TEXT,
    tech_stack TEXT,
    experience_level TEXT,
    learning_goals TEXT,
    profanity_comfort TEXT DEFAULT 'none',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- ============================================================================
-- PROJECTS & FILES
-- ============================================================================

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    description TEXT,
    language TEXT,
    framework TEXT,
    tags TEXT,
    owner_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_accessed INTEGER,
    modification_count INTEGER DEFAULT 0,
    FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_projects_owner ON projects(owner_id);
CREATE INDEX IF NOT EXISTS idx_projects_language ON projects(language);

CREATE TABLE IF NOT EXISTS git_repo_attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    repo_url TEXT,
    repo_path TEXT NOT NULL,
    branch TEXT,
    commit_hash TEXT,
    import_status TEXT DEFAULT 'pending',
    error_message TEXT,
    imported_at INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX IF NOT EXISTS idx_git_repo_session ON git_repo_attachments(session_id);

CREATE TABLE IF NOT EXISTS repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    language TEXT,
    size_bytes INTEGER,
    line_count INTEGER,
    last_modified INTEGER NOT NULL,
    ast_analyzed BOOLEAN DEFAULT FALSE,
    complexity_score REAL,
    created_at INTEGER NOT NULL,
    UNIQUE(project_id, file_path),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_repository_files_project ON repository_files(project_id);
CREATE INDEX IF NOT EXISTS idx_repository_files_language ON repository_files(language);
CREATE INDEX IF NOT EXISTS idx_repository_files_hash ON repository_files(content_hash);

CREATE TABLE IF NOT EXISTS local_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    change_type TEXT NOT NULL,
    old_hash TEXT,
    new_hash TEXT,
    diff TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_local_changes_project ON local_changes(project_id);
CREATE INDEX IF NOT EXISTS idx_local_changes_file ON local_changes(file_path);

-- ============================================================================
-- MEMORY & CONVERSATION
-- ============================================================================

CREATE TABLE IF NOT EXISTS memory_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    user_id TEXT,
    parent_id INTEGER,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    model TEXT,
    tokens INTEGER,
    cost REAL,
    reasoning_effort TEXT,
    tags TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES memory_entries(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_session ON memory_entries(session_id);
CREATE INDEX IF NOT EXISTS idx_memory_user ON memory_entries(user_id);
CREATE INDEX IF NOT EXISTS idx_memory_created ON memory_entries(created_at);
CREATE INDEX IF NOT EXISTS idx_memory_parent ON memory_entries(parent_id);

CREATE TABLE IF NOT EXISTS message_analysis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_entry_id INTEGER NOT NULL UNIQUE,
    mood TEXT,
    salience REAL DEFAULT 0.5,
    intent TEXT,
    topics TEXT,
    contains_error BOOLEAN DEFAULT FALSE,
    error_type TEXT,
    error_severity TEXT,
    error_file TEXT,
    error_line INTEGER,
    programming_language TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (memory_entry_id) REFERENCES memory_entries(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_message_analysis_salience ON message_analysis(salience);
CREATE INDEX IF NOT EXISTS idx_message_analysis_intent ON message_analysis(intent);
CREATE INDEX IF NOT EXISTS idx_message_analysis_error ON message_analysis(contains_error);
CREATE INDEX IF NOT EXISTS idx_message_analysis_language ON message_analysis(programming_language);

CREATE TABLE IF NOT EXISTS rolling_summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    user_id TEXT,
    summary_type TEXT NOT NULL,
    content TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    start_message_id INTEGER,
    end_message_id INTEGER,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (start_message_id) REFERENCES memory_entries(id) ON DELETE SET NULL,
    FOREIGN KEY (end_message_id) REFERENCES memory_entries(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_rolling_summaries_session ON rolling_summaries(session_id);
CREATE INDEX IF NOT EXISTS idx_rolling_summaries_user ON rolling_summaries(user_id);
CREATE INDEX IF NOT EXISTS idx_rolling_summaries_type ON rolling_summaries(summary_type);

-- ============================================================================
-- PERSONAL CONTEXT (User Facts & Patterns)
-- ============================================================================

CREATE TABLE IF NOT EXISTS memory_facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    category TEXT,
    relevance_score REAL DEFAULT 1.0,
    source_message_id INTEGER,
    deprecated BOOLEAN DEFAULT FALSE,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_accessed INTEGER,
    UNIQUE(user_id, key),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (source_message_id) REFERENCES memory_entries(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_facts_user ON memory_facts(user_id);
CREATE INDEX IF NOT EXISTS idx_memory_facts_category ON memory_facts(category);
CREATE INDEX IF NOT EXISTS idx_memory_facts_relevance ON memory_facts(relevance_score);
CREATE INDEX IF NOT EXISTS idx_memory_facts_deprecated ON memory_facts(deprecated);

CREATE TABLE IF NOT EXISTS learned_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    description TEXT NOT NULL,
    confidence REAL DEFAULT 0.5,
    applies_when TEXT,
    examples TEXT,
    times_observed INTEGER DEFAULT 1,
    times_applied INTEGER DEFAULT 0,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_observed INTEGER,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_learned_patterns_user ON learned_patterns(user_id);
CREATE INDEX IF NOT EXISTS idx_learned_patterns_type ON learned_patterns(pattern_type);
CREATE INDEX IF NOT EXISTS idx_learned_patterns_confidence ON learned_patterns(confidence);

-- ============================================================================
-- EMBEDDING TRACKING
-- ============================================================================

CREATE TABLE IF NOT EXISTS message_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_entry_id INTEGER NOT NULL,
    embedding_type TEXT NOT NULL,
    collection_name TEXT NOT NULL,
    point_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (memory_entry_id) REFERENCES memory_entries(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_message_embeddings_entry ON message_embeddings(memory_entry_id);
CREATE INDEX IF NOT EXISTS idx_message_embeddings_type ON message_embeddings(embedding_type);
CREATE INDEX IF NOT EXISTS idx_message_embeddings_point ON message_embeddings(point_id);
