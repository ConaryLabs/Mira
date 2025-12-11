-- Mira Power Suit for Claude Code
-- Fresh simplified schema focused on augmenting Claude Code capabilities
--
-- Core value propositions:
-- 1. Persistent memory across sessions (facts, decisions, preferences)
-- 2. Task management that persists
-- 3. Code intelligence (symbols, call graph, dependencies)
-- 4. Git intelligence (cochange patterns, error->fix learning)
-- 5. Build error tracking and fix learning
-- 6. Document storage for RAG

-- ============================================================================
-- MEMORY: Persistent facts, decisions, preferences
-- ============================================================================

-- Facts that Claude Code should remember across sessions
CREATE TABLE memory_facts (
    id TEXT PRIMARY KEY,
    fact_type TEXT NOT NULL DEFAULT 'general',  -- 'preference', 'decision', 'context', 'general'
    key TEXT NOT NULL UNIQUE,                   -- unique identifier for upsert
    value TEXT NOT NULL,                        -- the actual fact content
    category TEXT,                              -- optional categorization
    source TEXT,                                -- where this fact came from
    confidence REAL DEFAULT 1.0,                -- how confident we are (0-1)
    times_used INTEGER DEFAULT 0,               -- how often this fact has been recalled
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX idx_facts_type ON memory_facts(fact_type);
CREATE INDEX idx_facts_category ON memory_facts(category);
CREATE INDEX idx_facts_key ON memory_facts(key);

-- Project-specific coding guidelines and conventions
CREATE TABLE coding_guidelines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT,                          -- optional: specific to a project path
    category TEXT NOT NULL,                     -- 'naming', 'style', 'architecture', 'testing', etc.
    content TEXT NOT NULL,                      -- the guideline itself
    priority INTEGER DEFAULT 0,                 -- higher = more important
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_guidelines_project ON coding_guidelines(project_path);
CREATE INDEX idx_guidelines_category ON coding_guidelines(category);

-- ============================================================================
-- TASKS: Persistent task management
-- ============================================================================

-- Tasks/todos that persist across sessions
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    parent_id TEXT,                             -- for subtasks
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',     -- 'pending', 'in_progress', 'completed', 'blocked'
    priority TEXT DEFAULT 'medium',             -- 'low', 'medium', 'high', 'urgent'
    project_path TEXT,                          -- optional: associate with a project
    tags TEXT,                                  -- JSON array of tags
    due_date INTEGER,                           -- optional due date
    completed_at INTEGER,
    completion_notes TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (parent_id) REFERENCES tasks(id) ON DELETE CASCADE
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
CREATE INDEX idx_tasks_project ON tasks(project_path);
CREATE INDEX idx_tasks_priority ON tasks(priority);

-- ============================================================================
-- CODE INTELLIGENCE: Symbol tracking and relationships
-- ============================================================================

-- Parsed code symbols (functions, classes, structs, etc.)
CREATE TABLE code_symbols (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT,                        -- full path like 'module::Class::method'
    symbol_type TEXT NOT NULL,                  -- 'function', 'class', 'struct', 'trait', 'const', etc.
    language TEXT,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    signature TEXT,                             -- function signature or type definition
    visibility TEXT,                            -- 'public', 'private', 'protected'
    documentation TEXT,                         -- extracted doc comments
    content_hash TEXT,                          -- for change detection
    parent_id INTEGER,                          -- for nested symbols
    is_test BOOLEAN DEFAULT FALSE,
    is_async BOOLEAN DEFAULT FALSE,
    complexity_score REAL,                      -- cyclomatic complexity
    analyzed_at INTEGER NOT NULL,
    FOREIGN KEY (parent_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(file_path, name, start_line)
);

CREATE INDEX idx_symbols_file ON code_symbols(file_path);
CREATE INDEX idx_symbols_name ON code_symbols(name);
CREATE INDEX idx_symbols_type ON code_symbols(symbol_type);
CREATE INDEX idx_symbols_parent ON code_symbols(parent_id);
CREATE INDEX idx_symbols_hash ON code_symbols(content_hash);

-- Function call relationships
CREATE TABLE call_graph (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    caller_id INTEGER NOT NULL,
    callee_id INTEGER NOT NULL,
    call_type TEXT DEFAULT 'direct',            -- 'direct', 'indirect', 'async'
    call_line INTEGER,
    FOREIGN KEY (caller_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    FOREIGN KEY (callee_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(caller_id, callee_id, call_line)
);

CREATE INDEX idx_callgraph_caller ON call_graph(caller_id);
CREATE INDEX idx_callgraph_callee ON call_graph(callee_id);

-- Import/dependency tracking
CREATE TABLE imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    import_path TEXT NOT NULL,                  -- 'std::collections::HashMap'
    imported_symbols TEXT,                      -- JSON array of specific symbols, or null for glob
    is_external BOOLEAN DEFAULT FALSE,          -- true if from external crate/package
    analyzed_at INTEGER NOT NULL,
    UNIQUE(file_path, import_path)
);

CREATE INDEX idx_imports_file ON imports(file_path);
CREATE INDEX idx_imports_path ON imports(import_path);
CREATE INDEX idx_imports_external ON imports(is_external);

-- ============================================================================
-- GIT INTELLIGENCE: Learn from git history
-- ============================================================================

-- Files that frequently change together
CREATE TABLE cochange_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    cochange_count INTEGER NOT NULL DEFAULT 1,  -- times changed together
    confidence REAL NOT NULL DEFAULT 0.5,       -- how strong the relationship is
    last_seen INTEGER NOT NULL,
    UNIQUE(file_a, file_b)
);

CREATE INDEX idx_cochange_file_a ON cochange_patterns(file_a);
CREATE INDEX idx_cochange_file_b ON cochange_patterns(file_b);
CREATE INDEX idx_cochange_confidence ON cochange_patterns(confidence);

-- Historical error->fix patterns (learn from past fixes)
CREATE TABLE error_fixes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    error_pattern TEXT NOT NULL,                -- normalized error message pattern
    error_category TEXT,                        -- 'type_error', 'borrow_error', 'import_error', etc.
    language TEXT,
    file_pattern TEXT,                          -- file path pattern where this occurred
    fix_description TEXT,                       -- human-readable fix description
    fix_diff TEXT,                              -- optional: the actual diff that fixed it
    fix_commit TEXT,                            -- optional: commit hash
    times_seen INTEGER DEFAULT 1,
    times_fixed INTEGER DEFAULT 1,
    last_seen INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_errorfixes_pattern ON error_fixes(error_pattern);
CREATE INDEX idx_errorfixes_category ON error_fixes(error_category);
CREATE INDEX idx_errorfixes_language ON error_fixes(language);

-- Recent git commits for context
CREATE TABLE git_commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    commit_hash TEXT NOT NULL UNIQUE,
    author_name TEXT,
    author_email TEXT,
    message TEXT NOT NULL,
    files_changed TEXT,                         -- JSON array of file paths
    insertions INTEGER DEFAULT 0,
    deletions INTEGER DEFAULT 0,
    committed_at INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL
);

CREATE INDEX idx_commits_hash ON git_commits(commit_hash);
CREATE INDEX idx_commits_date ON git_commits(committed_at);
CREATE INDEX idx_commits_author ON git_commits(author_email);

-- ============================================================================
-- BUILD INTELLIGENCE: Learn from build errors
-- ============================================================================

-- Build run history
CREATE TABLE build_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT,
    command TEXT NOT NULL,                      -- 'cargo build', 'npm run build', etc.
    success BOOLEAN NOT NULL,
    duration_ms INTEGER,
    error_count INTEGER DEFAULT 0,
    warning_count INTEGER DEFAULT 0,
    started_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX idx_builds_project ON build_runs(project_path);
CREATE INDEX idx_builds_success ON build_runs(success);
CREATE INDEX idx_builds_started ON build_runs(started_at);

-- Individual build errors
CREATE TABLE build_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_run_id INTEGER,
    error_hash TEXT NOT NULL,                   -- hash for deduplication
    category TEXT,                              -- 'type_error', 'syntax_error', 'linker_error', etc.
    severity TEXT DEFAULT 'error',              -- 'error', 'warning'
    message TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    column_number INTEGER,
    code TEXT,                                  -- error code like E0308
    suggestion TEXT,                            -- compiler suggestion if any
    resolved BOOLEAN DEFAULT FALSE,
    resolved_at INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (build_run_id) REFERENCES build_runs(id) ON DELETE CASCADE
);

CREATE INDEX idx_builderrors_run ON build_errors(build_run_id);
CREATE INDEX idx_builderrors_hash ON build_errors(error_hash);
CREATE INDEX idx_builderrors_category ON build_errors(category);
CREATE INDEX idx_builderrors_file ON build_errors(file_path);
CREATE INDEX idx_builderrors_resolved ON build_errors(resolved);

-- ============================================================================
-- DOCUMENTS: RAG for uploaded documents
-- ============================================================================

-- Uploaded/indexed documents
CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    file_path TEXT,                             -- original file path if from disk
    doc_type TEXT NOT NULL,                     -- 'pdf', 'markdown', 'text', 'code', etc.
    content TEXT,                               -- full content (if small enough)
    summary TEXT,                               -- AI-generated summary
    chunk_count INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    metadata TEXT,                              -- JSON metadata
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_docs_name ON documents(name);
CREATE INDEX idx_docs_type ON documents(doc_type);
CREATE INDEX idx_docs_path ON documents(file_path);

-- Document chunks for RAG search
CREATE TABLE document_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    embedding_id TEXT,                          -- Qdrant point ID
    created_at INTEGER NOT NULL,
    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
);

CREATE INDEX idx_chunks_doc ON document_chunks(document_id);
CREATE INDEX idx_chunks_embedding ON document_chunks(embedding_id);

-- ============================================================================
-- WORKSPACE: Track what Claude Code is working on
-- ============================================================================

-- ============================================================================
-- LEGACY COMPATIBILITY: Tables needed by library code (compile-time SQL checks)
-- These can be removed when the library code is cleaned up
-- ============================================================================

-- Memory entries (legacy - Claude Code manages its own context)
CREATE TABLE memory_entries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    embedding_id TEXT,
    created_at INTEGER NOT NULL
);

-- Rolling summaries (legacy)
CREATE TABLE rolling_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

-- Message embeddings (legacy)
CREATE TABLE message_embeddings (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    embedding BLOB,
    created_at INTEGER NOT NULL
);

-- Message analysis (legacy)
CREATE TABLE message_analysis (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    analysis_type TEXT NOT NULL,
    result TEXT,
    created_at INTEGER NOT NULL
);

-- Repository files (legacy - for tracking indexed files)
CREATE TABLE repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    content_hash TEXT,
    last_indexed INTEGER,
    created_at INTEGER NOT NULL
);

-- Code elements (legacy name for code_symbols - some library code uses this)
CREATE TABLE code_elements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    element_type TEXT NOT NULL,
    start_line INTEGER,
    end_line INTEGER,
    signature TEXT,
    documentation TEXT,
    analyzed_at INTEGER NOT NULL
);

CREATE INDEX idx_code_elements_file ON code_elements(file_path);
CREATE INDEX idx_code_elements_name ON code_elements(name);

-- Code quality issues (legacy)
CREATE TABLE code_quality_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER,
    issue_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (element_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

-- External dependencies (legacy)
CREATE TABLE external_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER,
    dependency_name TEXT NOT NULL,
    dependency_type TEXT,
    FOREIGN KEY (element_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

-- Semantic analysis cache (legacy)
CREATE TABLE semantic_analysis_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id INTEGER,
    code_hash TEXT NOT NULL,
    analysis_result TEXT,
    confidence REAL,
    created_at INTEGER NOT NULL,
    last_used INTEGER,
    hit_count INTEGER DEFAULT 0
);

-- Pattern validation cache (legacy)
CREATE TABLE pattern_validation_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_id INTEGER,
    code_hash TEXT NOT NULL,
    validation_result TEXT,
    confidence REAL,
    created_at INTEGER NOT NULL,
    last_used INTEGER,
    hit_count INTEGER DEFAULT 0
);

-- Semantic nodes (legacy)
CREATE TABLE semantic_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT,
    node_type TEXT NOT NULL,
    name TEXT NOT NULL,
    metadata TEXT,
    created_at INTEGER NOT NULL
);

-- Semantic edges (legacy)
CREATE TABLE semantic_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL,
    target_id INTEGER NOT NULL,
    edge_type TEXT NOT NULL,
    weight REAL DEFAULT 1.0,
    FOREIGN KEY (source_id) REFERENCES semantic_nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (target_id) REFERENCES semantic_nodes(id) ON DELETE CASCADE
);

-- Concept index (legacy)
CREATE TABLE concept_index (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    concept TEXT NOT NULL,
    symbol_id INTEGER,
    score REAL DEFAULT 1.0,
    created_at INTEGER NOT NULL
);

-- Design patterns (legacy)
CREATE TABLE design_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_name TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    description TEXT,
    file_paths TEXT,
    confidence REAL,
    created_at INTEGER NOT NULL
);

-- Domain clusters (legacy)
CREATE TABLE domain_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_name TEXT NOT NULL,
    description TEXT,
    file_paths TEXT,
    created_at INTEGER NOT NULL
);

-- ============================================================================
-- WORKSPACE: Track what Claude Code is working on
-- ============================================================================

-- Recent file activity (what files have been touched)
CREATE TABLE file_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    activity_type TEXT NOT NULL,                -- 'read', 'write', 'error', 'test'
    context TEXT,                               -- optional context (error message, test result)
    session_id TEXT,                            -- Claude Code session if known
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_activity_file ON file_activity(file_path);
CREATE INDEX idx_activity_type ON file_activity(activity_type);
CREATE INDEX idx_activity_session ON file_activity(session_id);
CREATE INDEX idx_activity_created ON file_activity(created_at);

-- Active work context (what Claude Code is currently focused on)
CREATE TABLE work_context (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_type TEXT NOT NULL,                 -- 'active_task', 'recent_error', 'current_file'
    context_key TEXT NOT NULL,
    context_value TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    expires_at INTEGER,                         -- optional expiration
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(context_type, context_key)
);

CREATE INDEX idx_context_type ON work_context(context_type);
CREATE INDEX idx_context_expires ON work_context(expires_at);
