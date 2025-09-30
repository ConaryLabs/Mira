-- migrations/20250930_complete_schema_with_claude.sql
-- Complete consolidated schema for Mira with Claude Sonnet 4.5 support
-- Includes: Core memory, Claude metadata, documents, code intelligence, git integration

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
    original_salience REAL,  -- For decay system
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
CREATE INDEX idx_analysis_original_salience ON message_analysis(original_salience);
CREATE INDEX idx_analysis_topics ON message_analysis(topics);
CREATE INDEX idx_analysis_contains_code ON message_analysis(contains_code);
CREATE INDEX idx_analysis_message_id ON message_analysis(message_id);

-- ============================================
-- CLAUDE SONNET 4.5 METADATA
-- ============================================

-- LLM response metadata (Claude-optimized)
CREATE TABLE llm_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL UNIQUE REFERENCES memory_entries(id) ON DELETE CASCADE,
    
    -- Model info
    model_version TEXT NOT NULL,
    
    -- Token usage
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    thinking_tokens INTEGER DEFAULT 0,  -- Claude extended thinking tokens
    total_tokens INTEGER DEFAULT 0,
    
    -- Performance metrics
    latency_ms INTEGER DEFAULT 0,
    generation_time_ms INTEGER DEFAULT 0,
    
    -- Response metadata
    finish_reason TEXT,
    stop_reason TEXT,  -- Claude's enhanced stop reasons (end_turn, max_tokens, stop_sequence)
    tool_calls TEXT,  -- JSON array stored as TEXT
    
    -- Context management (Claude automatic feature)
    context_management_applied BOOLEAN DEFAULT FALSE,
    cleared_tokens INTEGER DEFAULT 0,
    
    -- Request parameters
    temperature REAL DEFAULT 0.7,
    max_tokens INTEGER DEFAULT 8192,
    
    -- Legacy fields (NULL for Claude)
    reasoning_effort TEXT,
    verbosity TEXT
);

CREATE INDEX idx_llm_message ON llm_metadata(message_id);
CREATE INDEX idx_llm_tokens ON llm_metadata(total_tokens);
CREATE INDEX idx_llm_thinking ON llm_metadata(thinking_tokens);
CREATE INDEX idx_llm_stop_reason ON llm_metadata(stop_reason);
CREATE INDEX idx_llm_model ON llm_metadata(model_version);

-- ============================================
-- EMBEDDINGS
-- ============================================

-- Embedding metadata with head routing
CREATE TABLE message_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    qdrant_point_id TEXT NOT NULL,
    collection_name TEXT NOT NULL,
    embedding_head TEXT NOT NULL CHECK(embedding_head IN ('semantic', 'code', 'summary')),
    generated_at INTEGER DEFAULT (strftime('%s', 'now'))
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

-- ============================================
-- ARTIFACTS
-- ============================================

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
-- DOCUMENT STORAGE
-- ============================================

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    file_type TEXT,
    file_hash TEXT,  -- SHA-256 for duplicate detection
    size_bytes INTEGER DEFAULT 0,
    content_hash TEXT,
    original_name TEXT,
    uploaded_at INTEGER DEFAULT (strftime('%s', 'now')),
    last_indexed INTEGER,
    metadata TEXT  -- JSON
);

CREATE INDEX idx_documents_project ON documents(project_id);
CREATE INDEX idx_documents_path ON documents(file_path);
CREATE UNIQUE INDEX idx_documents_hash_project ON documents(file_hash, project_id) WHERE file_hash IS NOT NULL;
CREATE INDEX idx_documents_project_created ON documents(project_id, uploaded_at DESC);

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
CREATE INDEX idx_document_chunks_doc_id ON document_chunks(document_id, chunk_index);

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
    -- Code intelligence fields
    ast_analyzed BOOLEAN DEFAULT FALSE,
    ast_hash TEXT,
    element_count INTEGER DEFAULT 0,
    complexity_score INTEGER DEFAULT 0,
    last_analyzed INTEGER,
    UNIQUE(attachment_id, file_path)
);

CREATE INDEX idx_repo_files_attachment ON repository_files(attachment_id);
CREATE INDEX idx_repo_files_hash ON repository_files(content_hash);
CREATE INDEX idx_repo_files_attachment_language ON repository_files(attachment_id, language);
CREATE INDEX idx_repo_files_analyzed ON repository_files(ast_analyzed);
CREATE INDEX idx_repo_files_language ON repository_files(language);

-- ============================================
-- CODE INTELLIGENCE
-- ============================================

-- Language configuration and parsing rules
CREATE TABLE language_configs (
    language TEXT PRIMARY KEY,
    file_extensions TEXT NOT NULL,
    parser_type TEXT NOT NULL,
    complexity_rules TEXT,
    dependency_patterns TEXT,
    created_at INTEGER
);

-- Insert initial language configs
INSERT INTO language_configs (language, file_extensions, parser_type, complexity_rules, dependency_patterns) VALUES
('rust', '["rs"]', 'rust_syn', 
 '{"max_cyclomatic": 10, "max_nesting": 4, "max_function_length": 50}',
 '["use\\s+([^;]+);", "mod\\s+([a-zA-Z_][a-zA-Z0-9_]*);"]'),

('typescript', '["ts", "tsx"]', 'typescript_swc',
 '{"max_cyclomatic": 15, "max_nesting": 5, "max_component_props": 8}',
 '["import\\s+[^from]*from\\s+[\"'']([^\"'']+)[\"'']", "import\\s+[\"'']([^\"'']+)[\"'']"]'),

('javascript', '["js", "jsx"]', 'javascript_babel',
 '{"max_cyclomatic": 15, "max_nesting": 5, "max_component_props": 8}',
 '["import\\s+[^from]*from\\s+[\"'']([^\"'']+)[\"'']", "require\\([\"'']([^\"'']+)[\"'']\\)"]');

-- Code elements (functions, structs, components, etc.)
CREATE TABLE code_elements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL REFERENCES repository_files(id) ON DELETE CASCADE,
    language TEXT NOT NULL REFERENCES language_configs(language),
    element_type TEXT NOT NULL,
    name TEXT NOT NULL,
    full_path TEXT NOT NULL,
    visibility TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content TEXT NOT NULL,
    signature_hash TEXT,
    complexity_score INTEGER DEFAULT 0,
    is_test BOOLEAN DEFAULT FALSE,
    is_async BOOLEAN DEFAULT FALSE,
    documentation TEXT,
    metadata TEXT,
    created_at INTEGER,
    analyzed_at INTEGER,
    UNIQUE(file_id, name, start_line)
);

CREATE INDEX idx_code_elements_file ON code_elements(file_id);
CREATE INDEX idx_code_elements_language ON code_elements(language);
CREATE INDEX idx_code_elements_type ON code_elements(element_type);
CREATE INDEX idx_code_elements_name ON code_elements(name);
CREATE INDEX idx_code_elements_complexity ON code_elements(complexity_score);

-- Track external dependencies (imports, uses)
CREATE TABLE external_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER NOT NULL REFERENCES code_elements(id) ON DELETE CASCADE,
    import_path TEXT NOT NULL,
    imported_symbols TEXT,
    dependency_type TEXT NOT NULL,
    created_at INTEGER
);

CREATE INDEX idx_external_deps_element ON external_dependencies(element_id);
CREATE INDEX idx_external_deps_path ON external_dependencies(import_path);

-- Code quality and issues
CREATE TABLE code_quality_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER NOT NULL REFERENCES code_elements(id) ON DELETE CASCADE,
    issue_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    suggested_fix TEXT,
    fix_confidence REAL DEFAULT 0.0,
    is_auto_fixable BOOLEAN DEFAULT FALSE,
    detected_at INTEGER
);

CREATE INDEX idx_quality_issues_element ON code_quality_issues(element_id);
CREATE INDEX idx_quality_issues_severity ON code_quality_issues(severity);

-- ============================================
-- STRUCTURED OPERATIONS
-- ============================================

CREATE TABLE structured_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_type TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT,
    request_data TEXT NOT NULL,
    response_data TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    completed_at INTEGER
);

CREATE INDEX idx_operations_project ON structured_operations(project_id);
CREATE INDEX idx_operations_status ON structured_operations(status);
CREATE INDEX idx_operations_type ON structured_operations(operation_type);

-- ============================================
-- MIGRATION METADATA
-- ============================================

CREATE TABLE schema_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version TEXT NOT NULL,
    description TEXT,
    applied_at INTEGER DEFAULT (strftime('%s', 'now'))
);

-- Record this consolidated migration
INSERT INTO schema_metadata (version, description) 
VALUES ('2.0.0', 'Complete consolidated schema with Claude Sonnet 4.5 support');
