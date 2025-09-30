-- migrations/20250917020459_fresh_schema.sql
-- Complete fresh database schema for Mira memory system

-- ============================================
-- CORE MEMORY TABLES
-- ============================================

-- Core message storage
CREATE TABLE memory_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    response_id TEXT,
    parent_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system', 'code', 'document')),
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    tags TEXT  -- JSON array for tags like "summary", "reinforced"
);

CREATE INDEX idx_memory_session_timestamp ON memory_entries(session_id, timestamp, id);
CREATE INDEX idx_memory_timestamp ON memory_entries(timestamp);
CREATE INDEX idx_memory_response_id ON memory_entries(response_id);
CREATE INDEX idx_memory_parent_id ON memory_entries(parent_id);

-- Analysis results with classification for routing
CREATE TABLE message_analysis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL UNIQUE REFERENCES memory_entries(id) ON DELETE CASCADE,
    mood TEXT,
    intensity REAL CHECK(intensity >= 0 AND intensity <= 1),
    salience REAL CHECK(salience >= 0 AND salience <= 10),
    intent TEXT,
    topics TEXT,  -- JSON array stored as TEXT
    summary TEXT,
    relationship_impact TEXT,
    contains_code BOOLEAN DEFAULT FALSE,
    language TEXT DEFAULT 'en',
    programming_lang TEXT CHECK(programming_lang IN ('rust', 'typescript', 'javascript', 'python', 'go', 'java') OR programming_lang IS NULL),
    analyzed_at INTEGER DEFAULT (strftime('%s', 'now')),
    analysis_version TEXT,
    routed_to_heads TEXT,  -- JSON array of head names
    last_recalled INTEGER,
    recall_count INTEGER DEFAULT 0
);

CREATE INDEX idx_analysis_mood ON message_analysis(mood);
CREATE INDEX idx_analysis_salience ON message_analysis(salience);
CREATE INDEX idx_analysis_message ON message_analysis(message_id);
CREATE INDEX idx_analysis_contains_code ON message_analysis(contains_code);
CREATE INDEX idx_analysis_last_recalled ON message_analysis(last_recalled);

-- GPT-5 response metadata
CREATE TABLE gpt5_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL UNIQUE REFERENCES memory_entries(id) ON DELETE CASCADE,
    model_version TEXT,
    prompt_tokens INTEGER DEFAULT 0,
    completion_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    latency_ms INTEGER DEFAULT 0,
    generation_time_ms INTEGER DEFAULT 0,
    finish_reason TEXT,
    tool_calls TEXT,  -- JSON array stored as TEXT
    temperature REAL DEFAULT 0.7,
    max_tokens INTEGER DEFAULT 4096,
    reasoning_effort TEXT CHECK(reasoning_effort IN ('minimal', 'low', 'medium', 'high') OR reasoning_effort IS NULL),
    verbosity TEXT CHECK(verbosity IN ('low', 'medium', 'high') OR verbosity IS NULL)
);

CREATE INDEX idx_gpt5_message ON gpt5_metadata(message_id);
CREATE INDEX idx_gpt5_tokens ON gpt5_metadata(total_tokens);

-- ============================================
-- EMBEDDING & VECTOR STORAGE
-- ============================================

-- Multi-head embedding references
CREATE TABLE message_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    embedding_head TEXT NOT NULL CHECK(embedding_head IN ('semantic', 'code', 'summary', 'documents')),
    qdrant_point_id TEXT NOT NULL,
    collection_name TEXT NOT NULL,
    embedding_model TEXT NOT NULL DEFAULT 'text-embedding-3-large',
    dimensions INTEGER NOT NULL DEFAULT 3072,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    UNIQUE(message_id, embedding_head),
    UNIQUE(qdrant_point_id)
);

CREATE INDEX idx_embedding_message ON message_embeddings(message_id);
CREATE INDEX idx_embedding_head ON message_embeddings(embedding_head);
CREATE INDEX idx_embedding_collection ON message_embeddings(collection_name, qdrant_point_id);

-- ============================================
-- SUMMARY & CACHE TABLES
-- ============================================

-- Rolling summary tracking
CREATE TABLE rolling_summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    summary_type TEXT NOT NULL CHECK(summary_type IN ('rolling_10', 'rolling_100', 'snapshot')),
    summary_text TEXT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    first_message_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    last_message_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    embedding_generated BOOLEAN DEFAULT FALSE
);

CREATE INDEX idx_summary_session ON rolling_summaries(session_id);
CREATE INDEX idx_summary_type ON rolling_summaries(summary_type);

-- Recent message cache for fast recall
CREATE TABLE recent_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id INTEGER NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    cached_at INTEGER DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX idx_cache_session ON recent_cache(session_id);
CREATE INDEX idx_cache_time ON recent_cache(cached_at);

-- ============================================
-- PROJECT MANAGEMENT
-- ============================================

CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    tags TEXT,
    owner TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX idx_projects_updated_at ON projects(updated_at);

-- Trigger to update projects.updated_at
CREATE TRIGGER update_projects_timestamp 
    AFTER UPDATE ON projects
    FOR EACH ROW
BEGIN
    UPDATE projects SET updated_at = strftime('%s', 'now') WHERE id = NEW.id;
END;

CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('code', 'image', 'log', 'note', 'markdown')),
    content TEXT,
    version INTEGER DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX idx_artifacts_project_id ON artifacts(project_id);

-- Trigger to update artifacts.updated_at
CREATE TRIGGER update_artifacts_timestamp 
    AFTER UPDATE ON artifacts
    FOR EACH ROW
BEGIN
    UPDATE artifacts SET updated_at = strftime('%s', 'now') WHERE id = NEW.id;
END;

-- ============================================
-- GIT INTEGRATION
-- ============================================

CREATE TABLE git_repo_attachments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_url TEXT NOT NULL,
    local_path TEXT NOT NULL,
    import_status TEXT NOT NULL,
    last_imported_at INTEGER,
    last_sync_at INTEGER,
    UNIQUE(project_id, repo_url)
);

CREATE INDEX idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX idx_git_repo_url ON git_repo_attachments(repo_url);

-- Track files with local changes
CREATE TABLE local_changes (
    attachment_id TEXT NOT NULL REFERENCES git_repo_attachments(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    has_changes BOOLEAN DEFAULT TRUE,
    modified_at INTEGER DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (attachment_id, file_path)
);

CREATE INDEX idx_local_changes_attachment ON local_changes(attachment_id);

-- Track repository code files
CREATE TABLE repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attachment_id TEXT NOT NULL REFERENCES git_repo_attachments(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    language TEXT,
    last_indexed INTEGER DEFAULT (strftime('%s', 'now')),
    line_count INTEGER DEFAULT 0,
    function_count INTEGER DEFAULT 0,
    UNIQUE(attachment_id, file_path)
);

CREATE INDEX idx_repo_files_attachment ON repository_files(attachment_id);
CREATE INDEX idx_repo_files_hash ON repository_files(content_hash);
CREATE INDEX idx_repo_files_attachment_language ON repository_files(attachment_id, language);

-- ============================================
-- DOCUMENT STORAGE
-- ============================================

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    file_type TEXT,
    size_bytes INTEGER DEFAULT 0,
    content_hash TEXT,
    original_name TEXT,
    uploaded_at INTEGER DEFAULT (strftime('%s', 'now')),
    last_indexed INTEGER,
    metadata TEXT  -- JSON
);

CREATE INDEX idx_documents_project ON documents(project_id);
CREATE INDEX idx_documents_path ON documents(file_path);

CREATE TABLE document_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    qdrant_point_id TEXT NOT NULL,
    content TEXT NOT NULL,
    char_start INTEGER DEFAULT 0,
    char_end INTEGER DEFAULT 0,
    UNIQUE(document_id, chunk_index)
);

CREATE INDEX idx_chunks_document ON document_chunks(document_id);
CREATE INDEX idx_chunks_qdrant ON document_chunks(qdrant_point_id);

-- ============================================
-- MIGRATION METADATA
-- ============================================

-- Track schema version and migration history
CREATE TABLE schema_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version TEXT NOT NULL,
    description TEXT,
    applied_at INTEGER DEFAULT (strftime('%s', 'now'))
);

-- Insert initial version
INSERT INTO schema_metadata (version, description) 
VALUES ('1.0.0', 'Fresh multi-head memory system with documents support');
