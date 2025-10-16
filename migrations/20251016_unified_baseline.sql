-- migrations/20251016_unified_baseline.sql
-- Unified baseline schema: Clean slate with zero cruft
-- Operations (coding) + Messages (conversation) + Relationship + Code Intelligence

BEGIN TRANSACTION;
PRAGMA foreign_keys=OFF;

-- ============================================================================
-- CORE CONVERSATION TABLES
-- ============================================================================

-- Core message storage (non-coding conversations)
CREATE TABLE IF NOT EXISTS memory_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    response_id TEXT,
    parent_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system', 'code', 'document')),
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    tags TEXT  -- JSON array
);

CREATE INDEX IF NOT EXISTS idx_memory_session_timestamp ON memory_entries(session_id, timestamp, id);
CREATE INDEX IF NOT EXISTS idx_memory_timestamp ON memory_entries(timestamp);
CREATE INDEX IF NOT EXISTS idx_memory_response_id ON memory_entries(response_id);
CREATE INDEX IF NOT EXISTS idx_memory_parent_id ON memory_entries(parent_id);

-- Analysis results for messages
CREATE TABLE IF NOT EXISTS message_analysis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL UNIQUE REFERENCES memory_entries(id) ON DELETE CASCADE,
    mood TEXT,
    intensity REAL CHECK(intensity >= 0 AND intensity <= 1),
    salience REAL CHECK(salience >= 0 AND salience <= 1),
    original_salience REAL,
    intent TEXT,
    topics TEXT NOT NULL DEFAULT '[]',
    summary TEXT,
    relationship_impact TEXT,
    contains_code BOOLEAN DEFAULT FALSE,
    language TEXT DEFAULT 'en',
    programming_lang TEXT CHECK(programming_lang IN ('rust', 'typescript', 'javascript', 'python', 'go', 'java') OR programming_lang IS NULL),
    analyzed_at INTEGER DEFAULT (strftime('%s','now')),
    analysis_version TEXT,
    routed_to_heads TEXT NOT NULL DEFAULT '[]',
    last_recalled INTEGER,
    recall_count INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_analysis_mood ON message_analysis(mood);
CREATE INDEX IF NOT EXISTS idx_analysis_salience ON message_analysis(salience);
CREATE INDEX IF NOT EXISTS idx_analysis_original_salience ON message_analysis(original_salience);
CREATE INDEX IF NOT EXISTS idx_analysis_contains_code ON message_analysis(contains_code);
CREATE INDEX IF NOT EXISTS idx_analysis_message_id ON message_analysis(message_id);

-- ============================================================================
-- EMBEDDINGS
-- ============================================================================

CREATE TABLE IF NOT EXISTS message_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    qdrant_point_id TEXT NOT NULL,
    collection_name TEXT NOT NULL,
    embedding_head TEXT NOT NULL CHECK(embedding_head IN ('semantic','code','summary','documents')),
    generated_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE INDEX IF NOT EXISTS idx_embedding_message ON message_embeddings(message_id);
CREATE INDEX IF NOT EXISTS idx_embedding_head ON message_embeddings(embedding_head);
CREATE INDEX IF NOT EXISTS idx_embedding_collection ON message_embeddings(collection_name, qdrant_point_id);

-- ============================================================================
-- SUMMARY SYSTEM
-- ============================================================================

CREATE TABLE IF NOT EXISTS rolling_summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    summary_type TEXT NOT NULL CHECK(summary_type IN ('rolling_10','rolling_100','snapshot')),
    summary_text TEXT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    first_message_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    last_message_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    embedding_generated BOOLEAN DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_summary_session ON rolling_summaries(session_id);
CREATE INDEX IF NOT EXISTS idx_summary_type ON rolling_summaries(summary_type);

-- ============================================================================
-- PROJECTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    tags TEXT,
    owner TEXT,
    modification_count INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at);

CREATE TRIGGER IF NOT EXISTS update_projects_timestamp
AFTER UPDATE ON projects
FOR EACH ROW BEGIN
    UPDATE projects SET updated_at = strftime('%s','now') WHERE id = NEW.id;
END;

-- ============================================================================
-- DOCUMENTS
-- ============================================================================

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    file_type TEXT,
    file_hash TEXT,
    size_bytes INTEGER DEFAULT 0,
    content_hash TEXT,
    original_name TEXT,
    uploaded_at INTEGER DEFAULT (strftime('%s','now')),
    last_indexed INTEGER,
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(file_path);
CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_hash_project ON documents(file_hash, project_id) WHERE file_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_documents_project_created ON documents(project_id, uploaded_at DESC);

CREATE TABLE IF NOT EXISTS document_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    qdrant_point_id TEXT NOT NULL,
    content TEXT NOT NULL,
    char_start INTEGER DEFAULT 0,
    char_end INTEGER DEFAULT 0,
    UNIQUE(document_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_chunks_document ON document_chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_chunks_qdrant ON document_chunks(qdrant_point_id);
CREATE INDEX IF NOT EXISTS idx_document_chunks_doc_id ON document_chunks(document_id, chunk_index);

-- ============================================================================
-- GIT & LOCAL DIRECTORIES
-- ============================================================================

CREATE TABLE IF NOT EXISTS git_repo_attachments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_url TEXT NOT NULL,
    local_path TEXT NOT NULL,
    import_status TEXT NOT NULL,
    last_imported_at INTEGER,
    last_sync_at INTEGER,
    attachment_type TEXT DEFAULT 'git_repository',
    local_path_override TEXT,
    UNIQUE(project_id, repo_url)
);

CREATE INDEX IF NOT EXISTS idx_git_repo_project ON git_repo_attachments(project_id);
CREATE INDEX IF NOT EXISTS idx_git_repo_url ON git_repo_attachments(repo_url);

CREATE TABLE IF NOT EXISTS local_changes (
    attachment_id TEXT NOT NULL REFERENCES git_repo_attachments(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    has_changes BOOLEAN DEFAULT TRUE,
    modified_at INTEGER DEFAULT (strftime('%s','now')),
    PRIMARY KEY (attachment_id, file_path)
);

CREATE INDEX IF NOT EXISTS idx_local_changes_attachment ON local_changes(attachment_id);

CREATE TABLE IF NOT EXISTS file_modifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    original_content TEXT NOT NULL,
    modified_content TEXT NOT NULL,
    modification_time INTEGER DEFAULT (strftime('%s','now')),
    reverted BOOLEAN DEFAULT FALSE,
    UNIQUE(project_id, file_path, modification_time)
);

CREATE INDEX IF NOT EXISTS idx_file_mods_project ON file_modifications(project_id, file_path);
CREATE INDEX IF NOT EXISTS idx_file_mods_time ON file_modifications(modification_time DESC);
CREATE INDEX IF NOT EXISTS idx_file_mods_reverted ON file_modifications(reverted);

-- ============================================================================
-- CODE INTELLIGENCE
-- ============================================================================

CREATE TABLE IF NOT EXISTS language_configs (
    language TEXT PRIMARY KEY,
    file_extensions TEXT NOT NULL,
    parser_type TEXT NOT NULL,
    complexity_rules TEXT,
    dependency_patterns TEXT,
    created_at INTEGER
);

INSERT OR IGNORE INTO language_configs (language, file_extensions, parser_type, complexity_rules, dependency_patterns)
VALUES
('rust', '["rs"]', 'rust_syn', '{"max_cyclomatic":10,"max_nesting":4,"max_function_length":50}', NULL),
('typescript', '["ts","tsx"]', 'typescript_swc', '{"max_cyclomatic":15,"max_nesting":5,"max_component_props":8}', NULL),
('javascript', '["js","jsx"]', 'javascript_babel', '{"max_cyclomatic":15,"max_nesting":5,"max_component_props":8}', NULL);

CREATE TABLE IF NOT EXISTS repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attachment_id TEXT NOT NULL REFERENCES git_repo_attachments(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    language TEXT,
    last_indexed INTEGER DEFAULT (strftime('%s','now')),
    line_count INTEGER DEFAULT 0,
    function_count INTEGER DEFAULT 0,
    ast_analyzed BOOLEAN DEFAULT FALSE,
    ast_hash TEXT,
    element_count INTEGER DEFAULT 0,
    complexity_score INTEGER DEFAULT 0,
    last_analyzed INTEGER,
    UNIQUE(attachment_id, file_path)
);

CREATE INDEX IF NOT EXISTS idx_repo_files_attachment ON repository_files(attachment_id);
CREATE INDEX IF NOT EXISTS idx_repo_files_hash ON repository_files(content_hash);
CREATE INDEX IF NOT EXISTS idx_repo_files_attachment_language ON repository_files(attachment_id, language);
CREATE INDEX IF NOT EXISTS idx_repo_files_analyzed ON repository_files(ast_analyzed);
CREATE INDEX IF NOT EXISTS idx_repo_files_language ON repository_files(language);

CREATE TABLE IF NOT EXISTS code_elements (
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

CREATE INDEX IF NOT EXISTS idx_code_elements_file ON code_elements(file_id);
CREATE INDEX IF NOT EXISTS idx_code_elements_language ON code_elements(language);
CREATE INDEX IF NOT EXISTS idx_code_elements_type ON code_elements(element_type);
CREATE INDEX IF NOT EXISTS idx_code_elements_name ON code_elements(name);
CREATE INDEX IF NOT EXISTS idx_code_elements_complexity ON code_elements(complexity_score);

CREATE TABLE IF NOT EXISTS external_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER NOT NULL REFERENCES code_elements(id) ON DELETE CASCADE,
    import_path TEXT NOT NULL,
    imported_symbols TEXT,
    dependency_type TEXT NOT NULL,
    created_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_external_deps_element ON external_dependencies(element_id);
CREATE INDEX IF NOT EXISTS idx_external_deps_path ON external_dependencies(import_path);

CREATE TABLE IF NOT EXISTS code_quality_issues (
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

CREATE INDEX IF NOT EXISTS idx_quality_issues_element ON code_quality_issues(element_id);
CREATE INDEX IF NOT EXISTS idx_quality_issues_severity ON code_quality_issues(severity);

-- ============================================================================
-- OPERATIONS SYSTEM (NEW - for coding workflow)
-- ============================================================================

CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    
    -- Timing
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    
    -- Input
    user_message TEXT NOT NULL,
    context_snapshot TEXT,
    
    -- Analysis & Routing
    complexity_score REAL,
    delegated_to TEXT,
    primary_model TEXT,
    delegation_reason TEXT,
    
    -- GPT-5 Responses API Tracking
    response_id TEXT,
    parent_response_id TEXT,
    parent_operation_id TEXT,
    
    -- Code-specific context
    target_language TEXT,
    target_framework TEXT,
    operation_intent TEXT,
    files_affected TEXT,
    
    -- Results
    result TEXT,
    error TEXT,
    
    -- Cost Tracking
    tokens_input INTEGER,
    tokens_output INTEGER,
    tokens_reasoning INTEGER,
    cost_usd REAL,
    delegate_calls INTEGER DEFAULT 0,
    
    -- Metadata
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_operations_session ON operations(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_operations_status ON operations(status, created_at);
CREATE INDEX IF NOT EXISTS idx_operations_response ON operations(response_id);
CREATE INDEX IF NOT EXISTS idx_operations_parent_response ON operations(parent_response_id);
CREATE INDEX IF NOT EXISTS idx_operations_kind ON operations(kind, created_at);
CREATE INDEX IF NOT EXISTS idx_operations_language ON operations(target_language, created_at);
CREATE INDEX IF NOT EXISTS idx_operations_intent ON operations(operation_intent, created_at);

CREATE TABLE IF NOT EXISTS operation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    event_data TEXT,
    sequence_number INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_operation_events_lookup ON operation_events(operation_id, sequence_number);
CREATE INDEX IF NOT EXISTS idx_operation_events_type ON operation_events(event_type, created_at);

-- Artifacts = generated code files (like Claude artifacts)
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    
    -- Artifact content
    kind TEXT NOT NULL,
    file_path TEXT,
    content TEXT NOT NULL,
    preview TEXT,
    
    -- Code-specific fields
    language TEXT,
    
    -- Change tracking
    content_hash TEXT,
    previous_artifact_id TEXT,
    is_new_file INTEGER DEFAULT 1,
    diff_from_previous TEXT,
    
    -- Context used for generation
    related_files TEXT,
    dependencies TEXT,
    project_context TEXT,
    user_requirements TEXT,
    constraints TEXT,
    
    -- Timing
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    applied_at INTEGER,
    
    -- Generation metadata
    generated_by TEXT,
    generation_time_ms INTEGER,
    context_tokens INTEGER,
    output_tokens INTEGER,
    
    -- Metadata
    metadata TEXT,
    
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE,
    FOREIGN KEY (previous_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_artifacts_operation ON artifacts(operation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_artifacts_path ON artifacts(file_path);
CREATE INDEX IF NOT EXISTS idx_artifacts_hash ON artifacts(content_hash);
CREATE INDEX IF NOT EXISTS idx_artifacts_language ON artifacts(language, created_at);
CREATE INDEX IF NOT EXISTS idx_artifacts_kind ON artifacts(kind, created_at);
CREATE INDEX IF NOT EXISTS idx_artifacts_previous ON artifacts(previous_artifact_id);

-- ============================================================================
-- RELATIONSHIP SYSTEM (NEW - personal context layer)
-- ============================================================================

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
    profanity_comfort TEXT,
    
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

CREATE INDEX IF NOT EXISTS idx_user_profile_user_id ON user_profile(user_id);

CREATE TABLE IF NOT EXISTS learned_patterns (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    
    -- Pattern Identity
    pattern_type TEXT NOT NULL,
    pattern_name TEXT NOT NULL,
    pattern_description TEXT NOT NULL,
    
    -- Evidence
    examples TEXT,
    
    -- Confidence & Validation
    confidence REAL NOT NULL,
    times_observed INTEGER DEFAULT 1,
    times_applied INTEGER DEFAULT 0,
    
    -- Context
    applies_when TEXT,
    deprecated INTEGER DEFAULT 0,
    
    -- Timing
    first_observed INTEGER NOT NULL,
    last_observed INTEGER NOT NULL,
    last_applied INTEGER,
    
    FOREIGN KEY (user_id) REFERENCES user_profile(user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_patterns_user ON learned_patterns(user_id);
CREATE INDEX IF NOT EXISTS idx_patterns_type_confidence ON learned_patterns(pattern_type, confidence DESC);
CREATE INDEX IF NOT EXISTS idx_patterns_deprecated ON learned_patterns(deprecated, confidence DESC);

CREATE TABLE IF NOT EXISTS memory_facts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    
    -- Fact Identity
    fact_key TEXT NOT NULL,
    fact_value TEXT NOT NULL,
    fact_category TEXT NOT NULL,
    
    -- Context
    context TEXT,
    confidence REAL DEFAULT 1.0,
    
    -- Relevance
    last_referenced INTEGER,
    reference_count INTEGER DEFAULT 0,
    still_relevant INTEGER DEFAULT 1,
    
    -- Timing
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    
    FOREIGN KEY (user_id) REFERENCES user_profile(user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_facts_user ON memory_facts(user_id);
CREATE INDEX IF NOT EXISTS idx_facts_key ON memory_facts(fact_key);
CREATE INDEX IF NOT EXISTS idx_facts_category ON memory_facts(fact_category);
CREATE INDEX IF NOT EXISTS idx_facts_relevance ON memory_facts(still_relevant, confidence DESC);
CREATE INDEX IF NOT EXISTS idx_facts_last_referenced ON memory_facts(last_referenced DESC);

-- ============================================================================
-- MIGRATION METADATA
-- ============================================================================

CREATE TABLE IF NOT EXISTS schema_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version TEXT NOT NULL,
    description TEXT,
    applied_at INTEGER DEFAULT (strftime('%s','now'))
);

INSERT INTO schema_metadata (version, description)
VALUES ('3.0.0', 'Clean baseline: Operations (coding) + Messages (conversation) + Relationship + Code Intelligence');

PRAGMA foreign_keys=ON;
COMMIT;
