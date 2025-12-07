-- backend/migrations/20251125_001_foundation.sql
-- Foundation Tables: Users, Authentication, Projects, Memory, Personal Context

-- ============================================================================
-- USER & AUTHENTICATION
-- ============================================================================

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_login_at INTEGER,
    is_active INTEGER DEFAULT 1,
    theme_preference TEXT
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
    -- Coding Preferences
    preferred_languages TEXT,
    coding_style TEXT,
    code_verbosity TEXT,
    testing_philosophy TEXT,
    architecture_preferences TEXT,
    -- Communication Style
    explanation_depth TEXT,
    conversation_style TEXT,
    profanity_comfort TEXT DEFAULT 'none',
    -- Tech Context
    tech_stack TEXT,
    learning_goals TEXT,
    -- Metadata
    relationship_started INTEGER NOT NULL,
    last_active INTEGER,
    total_sessions INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
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
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    session_id TEXT,
    repo_url TEXT,
    local_path TEXT NOT NULL,
    local_path_override TEXT,
    repo_path TEXT,
    branch TEXT,
    commit_hash TEXT,
    attachment_type TEXT DEFAULT 'git',
    import_status TEXT DEFAULT 'pending',
    error_message TEXT,
    imported_at INTEGER,
    last_imported_at INTEGER,
    last_sync_at INTEGER,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    UNIQUE(project_id, repo_url),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX IF NOT EXISTS idx_git_repo_session ON git_repo_attachments(session_id);

CREATE TABLE IF NOT EXISTS repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT,
    attachment_id TEXT,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    language TEXT,
    size_bytes INTEGER,
    line_count INTEGER,
    function_count INTEGER,
    element_count INTEGER DEFAULT 0,
    last_modified INTEGER,
    last_indexed INTEGER,
    last_analyzed INTEGER,
    ast_analyzed BOOLEAN DEFAULT FALSE,
    complexity_score REAL,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    UNIQUE(attachment_id, file_path),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (attachment_id) REFERENCES git_repo_attachments(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_repository_files_project ON repository_files(project_id);
CREATE INDEX IF NOT EXISTS idx_repository_files_attachment ON repository_files(attachment_id);
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
    response_id TEXT,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    model TEXT,
    tokens INTEGER,
    cost REAL,
    reasoning_effort TEXT,
    tags TEXT,
    timestamp INTEGER NOT NULL,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
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
    message_id INTEGER,
    mood TEXT,
    intensity REAL,
    salience REAL DEFAULT 0.5,
    original_salience REAL,
    intent TEXT,
    topics TEXT,
    summary TEXT,
    relationship_impact TEXT,
    language TEXT,
    contains_code BOOLEAN DEFAULT FALSE,
    contains_error BOOLEAN DEFAULT FALSE,
    error_type TEXT,
    error_severity TEXT,
    error_file TEXT,
    error_line INTEGER,
    programming_language TEXT,
    programming_lang TEXT,
    routed_to_heads TEXT,
    analyzed_at INTEGER,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (memory_entry_id) REFERENCES memory_entries(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES memory_entries(id) ON DELETE CASCADE
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
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    fact_key TEXT NOT NULL,
    fact_value TEXT NOT NULL,
    fact_category TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    source TEXT,
    learned_at INTEGER NOT NULL,
    last_confirmed INTEGER,
    times_referenced INTEGER DEFAULT 0,
    UNIQUE(user_id, fact_key)
);

CREATE INDEX IF NOT EXISTS idx_memory_facts_user ON memory_facts(user_id);
CREATE INDEX IF NOT EXISTS idx_memory_facts_category ON memory_facts(fact_category);
CREATE INDEX IF NOT EXISTS idx_memory_facts_key ON memory_facts(fact_key);

CREATE TABLE IF NOT EXISTS learned_patterns (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    pattern_name TEXT NOT NULL,
    pattern_description TEXT NOT NULL,
    examples TEXT,
    confidence REAL DEFAULT 0.5,
    times_observed INTEGER DEFAULT 1,
    times_applied INTEGER DEFAULT 0,
    applies_when TEXT,
    deprecated INTEGER DEFAULT 0,
    first_observed INTEGER NOT NULL,
    last_observed INTEGER NOT NULL,
    last_applied INTEGER
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
