-- Mira consolidated schema
-- Generated 2025-12-26 - all tables in one migration

-- =============================================================================
-- CORE: Projects
-- =============================================================================

CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    project_type TEXT,
    first_seen INTEGER NOT NULL,
    last_accessed INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_projects_path ON projects(path);
CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);

-- =============================================================================
-- MEMORY: Facts, Guidelines, Corrections
-- =============================================================================

CREATE TABLE IF NOT EXISTS memory_facts (
    id TEXT PRIMARY KEY,
    fact_type TEXT NOT NULL DEFAULT 'general',
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    category TEXT,
    source TEXT,
    confidence REAL DEFAULT 1.0,
    times_used INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used_at INTEGER,
    project_id INTEGER REFERENCES projects(id),
    validity TEXT DEFAULT 'active',
    superseded_by TEXT,
    file_path TEXT
);
CREATE INDEX IF NOT EXISTS idx_facts_type ON memory_facts(fact_type);
CREATE INDEX IF NOT EXISTS idx_facts_category ON memory_facts(category);
CREATE INDEX IF NOT EXISTS idx_facts_key ON memory_facts(key);
CREATE INDEX IF NOT EXISTS idx_facts_project ON memory_facts(project_id);
CREATE INDEX IF NOT EXISTS idx_facts_project_type ON memory_facts(project_id, fact_type);
CREATE INDEX IF NOT EXISTS idx_facts_validity ON memory_facts(validity);
CREATE INDEX IF NOT EXISTS idx_facts_file_path ON memory_facts(file_path);
CREATE INDEX IF NOT EXISTS idx_facts_created_at ON memory_facts(created_at);

CREATE TABLE IF NOT EXISTS coding_guidelines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT,
    category TEXT NOT NULL,
    content TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_guidelines_project ON coding_guidelines(project_path);
CREATE INDEX IF NOT EXISTS idx_guidelines_category ON coding_guidelines(category);

CREATE TABLE IF NOT EXISTS corrections (
    id TEXT PRIMARY KEY,
    correction_type TEXT NOT NULL,
    what_was_wrong TEXT NOT NULL,
    what_is_right TEXT NOT NULL,
    rationale TEXT,
    scope TEXT NOT NULL DEFAULT 'project',
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    file_patterns TEXT,
    topic_tags TEXT,
    keywords TEXT,
    confidence REAL DEFAULT 1.0,
    times_applied INTEGER DEFAULT 0,
    times_validated INTEGER DEFAULT 0,
    status TEXT DEFAULT 'active',
    superseded_by TEXT REFERENCES corrections(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_corrections_project ON corrections(project_id);
CREATE INDEX IF NOT EXISTS idx_corrections_type ON corrections(correction_type);
CREATE INDEX IF NOT EXISTS idx_corrections_scope ON corrections(scope);
CREATE INDEX IF NOT EXISTS idx_corrections_status ON corrections(status);

CREATE TABLE IF NOT EXISTS correction_applications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    correction_id TEXT NOT NULL REFERENCES corrections(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL,
    file_path TEXT,
    task_context TEXT,
    applied_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_correction_apps_correction ON correction_applications(correction_id);
CREATE INDEX IF NOT EXISTS idx_correction_apps_outcome ON correction_applications(outcome);

-- =============================================================================
-- TASKS & GOALS
-- =============================================================================

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    parent_id TEXT,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT DEFAULT 'medium',
    project_path TEXT,
    tags TEXT,
    due_date INTEGER,
    completed_at INTEGER,
    completion_notes TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (parent_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_path);
CREATE INDEX IF NOT EXISTS idx_tasks_priority ON tasks(priority);

CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    success_criteria TEXT,
    status TEXT NOT NULL DEFAULT 'planning',
    priority TEXT DEFAULT 'medium',
    progress_percent INTEGER DEFAULT 0,
    progress_mode TEXT DEFAULT 'auto',
    blockers TEXT,
    notes TEXT,
    tags TEXT,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    started_at INTEGER,
    target_date INTEGER,
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_goals_project ON goals(project_id);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_priority ON goals(priority);

CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    weight INTEGER DEFAULT 1,
    order_index INTEGER DEFAULT 0,
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_milestones_goal ON milestones(goal_id);
CREATE INDEX IF NOT EXISTS idx_milestones_status ON milestones(status);

CREATE TABLE IF NOT EXISTS rejected_approaches (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    problem_context TEXT NOT NULL,
    approach TEXT NOT NULL,
    rejection_reason TEXT NOT NULL,
    related_files TEXT,
    related_topics TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rejected_project ON rejected_approaches(project_id);

-- =============================================================================
-- CODE INTELLIGENCE
-- =============================================================================

CREATE TABLE IF NOT EXISTS code_symbols (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT,
    symbol_type TEXT NOT NULL,
    language TEXT,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    signature TEXT,
    visibility TEXT,
    documentation TEXT,
    content_hash TEXT,
    parent_id INTEGER,
    is_test BOOLEAN DEFAULT FALSE,
    is_async BOOLEAN DEFAULT FALSE,
    complexity_score REAL,
    analyzed_at INTEGER NOT NULL,
    FOREIGN KEY (parent_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(file_path, name, start_line)
);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON code_symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_type ON code_symbols(symbol_type);
CREATE INDEX IF NOT EXISTS idx_symbols_parent ON code_symbols(parent_id);
CREATE INDEX IF NOT EXISTS idx_symbols_hash ON code_symbols(content_hash);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON code_symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_lang_type ON code_symbols(language, symbol_type);

CREATE TABLE IF NOT EXISTS call_graph (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    caller_id INTEGER NOT NULL,
    callee_id INTEGER NOT NULL,
    call_type TEXT DEFAULT 'direct',
    call_line INTEGER,
    callee_name TEXT,
    FOREIGN KEY (caller_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    FOREIGN KEY (callee_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(caller_id, callee_id, call_line)
);
CREATE INDEX IF NOT EXISTS idx_callgraph_caller ON call_graph(caller_id);
CREATE INDEX IF NOT EXISTS idx_callgraph_callee ON call_graph(callee_id);
CREATE INDEX IF NOT EXISTS idx_callgraph_callee_name ON call_graph(callee_name);

CREATE TABLE IF NOT EXISTS unresolved_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    caller_id INTEGER NOT NULL,
    callee_name TEXT NOT NULL,
    call_type TEXT DEFAULT 'direct',
    call_line INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (caller_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(caller_id, callee_name, call_line)
);
CREATE INDEX IF NOT EXISTS idx_unresolved_callee_name ON unresolved_calls(callee_name);
CREATE INDEX IF NOT EXISTS idx_unresolved_caller ON unresolved_calls(caller_id);

CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    import_path TEXT NOT NULL,
    imported_symbols TEXT,
    is_external BOOLEAN DEFAULT FALSE,
    analyzed_at INTEGER NOT NULL,
    UNIQUE(file_path, import_path)
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_path);
CREATE INDEX IF NOT EXISTS idx_imports_path ON imports(import_path);
CREATE INDEX IF NOT EXISTS idx_imports_external ON imports(is_external);

CREATE TABLE IF NOT EXISTS cochange_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    cochange_count INTEGER NOT NULL DEFAULT 1,
    confidence REAL NOT NULL DEFAULT 0.5,
    last_seen INTEGER NOT NULL,
    UNIQUE(file_a, file_b)
);
CREATE INDEX IF NOT EXISTS idx_cochange_file_a ON cochange_patterns(file_a);
CREATE INDEX IF NOT EXISTS idx_cochange_file_b ON cochange_patterns(file_b);
CREATE INDEX IF NOT EXISTS idx_cochange_confidence ON cochange_patterns(confidence);

CREATE TABLE IF NOT EXISTS code_style_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    pattern_value TEXT NOT NULL,
    sample_count INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.0,
    computed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_style_patterns_project ON code_style_patterns(project_path);
CREATE INDEX IF NOT EXISTS idx_style_patterns_type ON code_style_patterns(pattern_type);

-- =============================================================================
-- GIT INTELLIGENCE
-- =============================================================================

CREATE TABLE IF NOT EXISTS git_commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    commit_hash TEXT NOT NULL UNIQUE,
    author_name TEXT,
    author_email TEXT,
    message TEXT NOT NULL,
    files_changed TEXT,
    insertions INTEGER DEFAULT 0,
    deletions INTEGER DEFAULT 0,
    committed_at INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_commits_hash ON git_commits(commit_hash);
CREATE INDEX IF NOT EXISTS idx_commits_date ON git_commits(committed_at);
CREATE INDEX IF NOT EXISTS idx_commits_author ON git_commits(author_email);
CREATE INDEX IF NOT EXISTS idx_commits_message ON git_commits(message);

-- =============================================================================
-- BUILD INTELLIGENCE
-- =============================================================================

CREATE TABLE IF NOT EXISTS build_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT,
    command TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    duration_ms INTEGER,
    error_count INTEGER DEFAULT 0,
    warning_count INTEGER DEFAULT 0,
    started_at INTEGER NOT NULL,
    completed_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_builds_project ON build_runs(project_path);
CREATE INDEX IF NOT EXISTS idx_builds_success ON build_runs(success);
CREATE INDEX IF NOT EXISTS idx_builds_started ON build_runs(started_at);

CREATE TABLE IF NOT EXISTS build_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_run_id INTEGER,
    error_hash TEXT NOT NULL,
    category TEXT,
    severity TEXT DEFAULT 'error',
    message TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    column_number INTEGER,
    code TEXT,
    suggestion TEXT,
    resolved BOOLEAN DEFAULT FALSE,
    resolved_at INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (build_run_id) REFERENCES build_runs(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_builderrors_run ON build_errors(build_run_id);
CREATE INDEX IF NOT EXISTS idx_builderrors_hash ON build_errors(error_hash);
CREATE INDEX IF NOT EXISTS idx_builderrors_category ON build_errors(category);
CREATE INDEX IF NOT EXISTS idx_builderrors_file ON build_errors(file_path);
CREATE INDEX IF NOT EXISTS idx_builderrors_resolved ON build_errors(resolved);

CREATE TABLE IF NOT EXISTS error_fixes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    error_pattern TEXT NOT NULL,
    error_category TEXT,
    language TEXT,
    file_pattern TEXT,
    fix_description TEXT,
    fix_diff TEXT,
    fix_commit TEXT,
    times_seen INTEGER DEFAULT 1,
    times_fixed INTEGER DEFAULT 1,
    last_seen INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    project_id INTEGER REFERENCES projects(id)
);
CREATE INDEX IF NOT EXISTS idx_errorfixes_pattern ON error_fixes(error_pattern);
CREATE INDEX IF NOT EXISTS idx_errorfixes_category ON error_fixes(error_category);
CREATE INDEX IF NOT EXISTS idx_errorfixes_language ON error_fixes(language);
CREATE INDEX IF NOT EXISTS idx_error_fixes_project ON error_fixes(project_id);

-- =============================================================================
-- DOCUMENTS
-- =============================================================================

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    file_path TEXT,
    doc_type TEXT NOT NULL,
    content TEXT,
    summary TEXT,
    chunk_count INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    metadata TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_docs_name ON documents(name);
CREATE INDEX IF NOT EXISTS idx_docs_type ON documents(doc_type);
CREATE INDEX IF NOT EXISTS idx_docs_path ON documents(file_path);

CREATE TABLE IF NOT EXISTS document_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    embedding_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chunks_doc ON document_chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_chunks_embedding ON document_chunks(embedding_id);

-- =============================================================================
-- ACTIVITY & CONTEXT
-- =============================================================================

CREATE TABLE IF NOT EXISTS file_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    activity_type TEXT NOT NULL,
    context TEXT,
    session_id TEXT,
    created_at INTEGER NOT NULL,
    project_id INTEGER REFERENCES projects(id)
);
CREATE INDEX IF NOT EXISTS idx_activity_file ON file_activity(file_path);
CREATE INDEX IF NOT EXISTS idx_activity_type ON file_activity(activity_type);
CREATE INDEX IF NOT EXISTS idx_activity_session ON file_activity(session_id);
CREATE INDEX IF NOT EXISTS idx_activity_created ON file_activity(created_at);
CREATE INDEX IF NOT EXISTS idx_file_activity_project ON file_activity(project_id);

CREATE TABLE IF NOT EXISTS work_context (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_type TEXT NOT NULL,
    context_key TEXT NOT NULL,
    context_value TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    project_id INTEGER REFERENCES projects(id),
    session_id TEXT REFERENCES mcp_sessions(id) ON DELETE SET NULL,
    UNIQUE(context_type, context_key)
);
CREATE INDEX IF NOT EXISTS idx_context_type ON work_context(context_type);
CREATE INDEX IF NOT EXISTS idx_context_expires ON work_context(expires_at);
CREATE INDEX IF NOT EXISTS idx_work_context_project ON work_context(project_id);

CREATE TABLE IF NOT EXISTS carousel_state (
    id INTEGER PRIMARY KEY DEFAULT 1,
    state_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- =============================================================================
-- MCP SESSIONS & HISTORY
-- =============================================================================

CREATE TABLE IF NOT EXISTS mcp_sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    phase TEXT NOT NULL DEFAULT 'early',
    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_activity INTEGER NOT NULL DEFAULT (unixepoch()),
    tool_call_count INTEGER NOT NULL DEFAULT 0,
    read_count INTEGER NOT NULL DEFAULT 0,
    write_count INTEGER NOT NULL DEFAULT 0,
    build_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    commit_count INTEGER NOT NULL DEFAULT 0,
    estimated_progress REAL DEFAULT 0.0,
    active_goal_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    end_reason TEXT,
    touched_files TEXT,
    topics TEXT
);
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_project ON mcp_sessions(project_id, status, last_activity DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_active ON mcp_sessions(status, last_activity DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_lookup ON mcp_sessions(id, started_at);

CREATE TABLE IF NOT EXISTS mcp_history (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    session_id TEXT,
    project_id INTEGER REFERENCES projects(id),
    tool_name TEXT NOT NULL,
    arguments TEXT,
    result_summary TEXT,
    success INTEGER DEFAULT 1,
    duration_ms INTEGER,
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_mcp_history_session ON mcp_history(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_history_project ON mcp_history(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_history_tool ON mcp_history(tool_name, created_at DESC);

CREATE TABLE IF NOT EXISTS mcp_history_embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    history_id TEXT NOT NULL REFERENCES mcp_history(id) ON DELETE CASCADE,
    qdrant_point_id TEXT,
    content_hash TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE(history_id)
);
CREATE INDEX IF NOT EXISTS idx_mcp_embeddings_point ON mcp_history_embeddings(qdrant_point_id);

-- =============================================================================
-- PERMISSIONS
-- =============================================================================

CREATE TABLE IF NOT EXISTS permission_rules (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL DEFAULT 'project',
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    input_field TEXT,
    input_pattern TEXT,
    match_type TEXT DEFAULT 'prefix',
    description TEXT,
    times_used INTEGER DEFAULT 0,
    last_used_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(scope, project_id, tool_name, input_field, input_pattern)
);
CREATE INDEX IF NOT EXISTS idx_perm_rules_scope ON permission_rules(scope);
CREATE INDEX IF NOT EXISTS idx_perm_rules_project ON permission_rules(project_id);
CREATE INDEX IF NOT EXISTS idx_perm_rules_tool ON permission_rules(tool_name);

-- =============================================================================
-- CHAT (Studio)
-- =============================================================================

CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    blocks TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    archived_at INTEGER,
    summary_id TEXT REFERENCES chat_summaries(id)
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_created ON chat_messages(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_chat_messages_active ON chat_messages(created_at DESC) WHERE archived_at IS NULL;

CREATE TABLE IF NOT EXISTS chat_summaries (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    summary TEXT NOT NULL,
    message_ids TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    token_estimate INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    level INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_chat_summaries_project ON chat_summaries(project_path, created_at DESC);

CREATE TABLE IF NOT EXISTS chat_context (
    project_path TEXT PRIMARY KEY,
    last_response_id TEXT,
    last_compaction_id TEXT,
    window_start_id TEXT,
    total_messages INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    needs_handoff INTEGER NOT NULL DEFAULT 0,
    handoff_blob TEXT,
    consecutive_low_cache_turns INTEGER NOT NULL DEFAULT 0,
    turns_since_reset INTEGER NOT NULL DEFAULT 0,
    last_failure_command TEXT,
    last_failure_error TEXT,
    last_failure_at INTEGER,
    recent_artifact_ids TEXT
);

CREATE TABLE IF NOT EXISTS chat_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_usage (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES chat_messages(id),
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    reasoning_effort TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    response_id TEXT,
    previous_response_id TEXT,
    tool_count INTEGER DEFAULT 0,
    tool_names TEXT
);
CREATE INDEX IF NOT EXISTS idx_chat_usage_message ON chat_usage(message_id);

CREATE TABLE IF NOT EXISTS chat_tool_calls (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    success INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    artifact_id TEXT,
    inline_bytes INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON chat_tool_calls(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_tool_name ON chat_tool_calls(tool_name);
CREATE INDEX IF NOT EXISTS idx_tool_calls_artifact ON chat_tool_calls(artifact_id) WHERE artifact_id IS NOT NULL;

-- =============================================================================
-- ARTIFACTS
-- =============================================================================

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    project_path TEXT NOT NULL,
    kind TEXT NOT NULL,
    tool_name TEXT,
    tool_call_id TEXT,
    message_id TEXT,
    content_type TEXT NOT NULL DEFAULT 'text/plain; charset=utf-8',
    encoding TEXT NOT NULL DEFAULT 'utf-8',
    compression TEXT NOT NULL DEFAULT 'none',
    uncompressed_bytes INTEGER NOT NULL,
    compressed_bytes INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    contains_secrets INTEGER NOT NULL DEFAULT 0,
    secret_reason TEXT,
    preview_text TEXT,
    data BLOB NOT NULL,
    searchable_text TEXT
);
CREATE INDEX IF NOT EXISTS idx_artifacts_project_created ON artifacts(project_path, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_expires ON artifacts(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_artifacts_message ON artifacts(message_id) WHERE message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_artifacts_tool_call ON artifacts(tool_call_id) WHERE tool_call_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_artifacts_sha ON artifacts(sha256);

-- =============================================================================
-- PROPOSALS
-- =============================================================================

CREATE TABLE IF NOT EXISTS proposals (
    id TEXT PRIMARY KEY,
    proposal_type TEXT NOT NULL,
    content TEXT NOT NULL,
    title TEXT,
    confidence REAL NOT NULL DEFAULT 0.5,
    evidence TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    content_hash TEXT,
    embedding_id TEXT,
    similar_to TEXT,
    source_tool TEXT,
    source_context TEXT,
    project_path TEXT,
    created_at INTEGER NOT NULL,
    processed_at INTEGER,
    promoted_to TEXT,
    batch_id TEXT,
    review_priority INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_proposals_type ON proposals(proposal_type);
CREATE INDEX IF NOT EXISTS idx_proposals_status ON proposals(status);
CREATE INDEX IF NOT EXISTS idx_proposals_confidence ON proposals(confidence);
CREATE INDEX IF NOT EXISTS idx_proposals_project ON proposals(project_path);
CREATE INDEX IF NOT EXISTS idx_proposals_batch ON proposals(batch_id);
CREATE INDEX IF NOT EXISTS idx_proposals_hash ON proposals(content_hash);
CREATE INDEX IF NOT EXISTS idx_proposals_created ON proposals(created_at);

CREATE TABLE IF NOT EXISTS extraction_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_type TEXT NOT NULL,
    pattern TEXT NOT NULL,
    confidence_boost REAL DEFAULT 0.0,
    description TEXT,
    enabled BOOLEAN DEFAULT TRUE,
    times_matched INTEGER DEFAULT 0,
    times_confirmed INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_patterns_type ON extraction_patterns(pattern_type);
CREATE INDEX IF NOT EXISTS idx_patterns_enabled ON extraction_patterns(enabled);

CREATE TABLE IF NOT EXISTS extracted_decisions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    content TEXT NOT NULL,
    confidence REAL NOT NULL,
    decision_type TEXT NOT NULL,
    context TEXT,
    extracted_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_decisions_session ON extracted_decisions(session_id);
CREATE INDEX IF NOT EXISTS idx_decisions_type ON extracted_decisions(decision_type);
CREATE INDEX IF NOT EXISTS idx_decisions_project ON extracted_decisions(project_id);
CREATE INDEX IF NOT EXISTS idx_decisions_extracted ON extracted_decisions(extracted_at);

-- =============================================================================
-- INSTRUCTION QUEUE
-- =============================================================================

CREATE TABLE IF NOT EXISTS instruction_queue (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    instruction TEXT NOT NULL,
    context TEXT,
    priority TEXT DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'in_progress', 'completed', 'failed', 'cancelled')),
    created_at TEXT DEFAULT (datetime('now')),
    delivered_at TEXT,
    started_at TEXT,
    completed_at TEXT,
    result TEXT,
    error TEXT
);
CREATE INDEX IF NOT EXISTS idx_instruction_queue_pending ON instruction_queue(project_id, status, priority, created_at) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_instruction_queue_project ON instruction_queue(project_id, created_at DESC);

-- =============================================================================
-- BATCH PROCESSING
-- =============================================================================

CREATE TABLE IF NOT EXISTS batch_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    gemini_batch_name TEXT,
    display_name TEXT,
    input_data TEXT,
    request_count INTEGER DEFAULT 0,
    output_data TEXT,
    created_at INTEGER NOT NULL,
    submitted_at INTEGER,
    completed_at INTEGER,
    error_message TEXT,
    metadata TEXT
);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_status ON batch_jobs(status);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_project ON batch_jobs(project_id);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_type ON batch_jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_gemini ON batch_jobs(gemini_batch_name);

CREATE TABLE IF NOT EXISTS batch_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id INTEGER NOT NULL REFERENCES batch_jobs(id) ON DELETE CASCADE,
    request_key TEXT NOT NULL,
    request_data TEXT NOT NULL,
    response_data TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE(job_id, request_key)
);
CREATE INDEX IF NOT EXISTS idx_batch_requests_job ON batch_requests(job_id);
CREATE INDEX IF NOT EXISTS idx_batch_requests_status ON batch_requests(status);

-- =============================================================================
-- FILE SEARCH (Gemini RAG)
-- =============================================================================

CREATE TABLE IF NOT EXISTS file_search_stores (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    store_name TEXT NOT NULL,
    display_name TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    active_documents INTEGER DEFAULT 0,
    pending_documents INTEGER DEFAULT 0,
    failed_documents INTEGER DEFAULT 0,
    size_bytes INTEGER DEFAULT 0,
    UNIQUE(project_id)
);
CREATE INDEX IF NOT EXISTS idx_file_search_stores_project ON file_search_stores(project_id);

CREATE TABLE IF NOT EXISTS file_search_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id INTEGER NOT NULL REFERENCES file_search_stores(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    display_name TEXT,
    file_path TEXT NOT NULL,
    mime_type TEXT,
    size_bytes INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    indexed_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    metadata TEXT,
    UNIQUE(store_id, file_path)
);
CREATE INDEX IF NOT EXISTS idx_file_search_documents_store ON file_search_documents(store_id);
CREATE INDEX IF NOT EXISTS idx_file_search_documents_path ON file_search_documents(file_path);
CREATE INDEX IF NOT EXISTS idx_file_search_documents_status ON file_search_documents(status);

-- =============================================================================
-- ROUTING & CACHING
-- =============================================================================

CREATE TABLE IF NOT EXISTS routing_cache (
    query_hash TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    confidence REAL NOT NULL,
    reasoning TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    hits INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_routing_cache_created ON routing_cache(created_at);
CREATE INDEX IF NOT EXISTS idx_routing_cache_category ON routing_cache(category);

CREATE TABLE IF NOT EXISTS category_summaries (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    generated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(category, project_id)
);
CREATE INDEX IF NOT EXISTS idx_category_summaries_cat ON category_summaries(category);

CREATE TABLE IF NOT EXISTS debounce_state (
    key TEXT PRIMARY KEY,
    last_triggered INTEGER NOT NULL,
    trigger_count INTEGER DEFAULT 1,
    context TEXT
);
CREATE INDEX IF NOT EXISTS idx_debounce_last ON debounce_state(last_triggered);

-- =============================================================================
-- LEGACY TABLES (kept for compatibility, may be unused)
-- =============================================================================

CREATE TABLE IF NOT EXISTS memory_entries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    embedding_id TEXT,
    created_at INTEGER NOT NULL,
    project_id INTEGER REFERENCES projects(id)
);
CREATE INDEX IF NOT EXISTS idx_entries_project ON memory_entries(project_id);

CREATE TABLE IF NOT EXISTS rolling_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS repository_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    content_hash TEXT,
    last_indexed INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS code_compaction (
    id TEXT PRIMARY KEY,
    project_path TEXT NOT NULL,
    encrypted_content TEXT NOT NULL,
    token_count INTEGER,
    files_included TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_code_compaction_project ON code_compaction(project_path, created_at DESC);
