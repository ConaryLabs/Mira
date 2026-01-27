// crates/mira-server/src/db/schema/mod.rs
// Database schema and migrations

use anyhow::Result;
use rusqlite::Connection;

mod fts;
mod intelligence;
mod memory;
mod reviews;
mod session;
mod system;
mod vectors;

// Re-export FTS functions that are used elsewhere
pub use fts::{rebuild_code_fts, rebuild_code_fts_for_project};

/// Run all schema setup and migrations.
///
/// Called during database initialization. This function is idempotent -
/// it checks for existing tables/columns before making changes.
pub fn run_all_migrations(conn: &Connection) -> Result<()> {
    // Create base tables
    conn.execute_batch(SCHEMA)?;

    // Run migrations in order
    vectors::migrate_vec_tables(conn)?;
    vectors::migrate_pending_embeddings_line_numbers(conn)?;
    vectors::migrate_vec_code_line_numbers(conn)?;
    session::migrate_tool_history_full_result(conn)?;
    session::migrate_chat_summaries_project_id(conn)?;
    session::migrate_chat_messages_summary_id(conn)?;
    memory::migrate_memory_facts_has_embedding(conn)?;
    memory::migrate_memory_facts_evidence_tracking(conn)?;
    system::migrate_system_prompts_provider(conn)?;
    system::migrate_system_prompts_strip_tool_suffix(conn)?;
    fts::migrate_code_fts(conn)?;
    memory::migrate_imports_unique(conn)?;
    memory::migrate_documentation_tables(conn)?;
    memory::migrate_users_table(conn)?;
    memory::migrate_memory_user_scope(conn)?;
    memory::migrate_teams_tables(conn)?;

    // Add review findings table for code review learning loop
    reviews::migrate_review_findings_table(conn)?;

    // Add learning columns to corrections table
    reviews::migrate_corrections_learning_columns(conn)?;

    // Add embeddings usage tracking table
    reviews::migrate_embeddings_usage_table(conn)?;

    // Add diff analyses table for semantic diff analysis
    reviews::migrate_diff_analyses_table(conn)?;

    // Add LLM usage tracking table for cost analytics
    reviews::migrate_llm_usage_table(conn)?;

    // Add proactive intelligence tables for behavior tracking
    intelligence::migrate_proactive_intelligence_tables(conn)?;

    // Add evolutionary expert system tables
    intelligence::migrate_evolutionary_expert_tables(conn)?;

    // Add cross-project intelligence tables for pattern sharing
    intelligence::migrate_cross_project_intelligence_tables(conn)?;

    // Add branch column for branch-aware context switching
    memory::migrate_memory_facts_branch(conn)?;
    session::migrate_sessions_branch(conn)?;

    Ok(())
}

/// Database schema SQL
pub const SCHEMA: &str = r#"
-- =======================================
-- CORE: Projects
-- =======================================
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- MEMORY: Semantic Facts
-- =======================================
CREATE TABLE IF NOT EXISTS memory_facts (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    key TEXT,
    content TEXT NOT NULL,
    fact_type TEXT DEFAULT 'general',
    category TEXT,
    confidence REAL DEFAULT 0.5,
    has_embedding INTEGER DEFAULT 0,  -- 1 if fact has embedding in vec_memory
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    -- Evidence-based memory tracking
    session_count INTEGER DEFAULT 1,       -- number of sessions where this was seen/used
    first_session_id TEXT,                 -- session when first created
    last_session_id TEXT,                  -- most recent session that referenced this
    status TEXT DEFAULT 'candidate'        -- 'candidate' or 'confirmed'
);
CREATE INDEX IF NOT EXISTS idx_memory_project ON memory_facts(project_id);
CREATE INDEX IF NOT EXISTS idx_memory_key ON memory_facts(key);
CREATE INDEX IF NOT EXISTS idx_memory_no_embedding ON memory_facts(has_embedding) WHERE has_embedding = 0;
-- Note: idx_memory_status is created in migrate_memory_facts_evidence_tracking() for compatibility with existing databases

CREATE TABLE IF NOT EXISTS corrections (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    what_was_wrong TEXT NOT NULL,
    what_is_right TEXT NOT NULL,
    correction_type TEXT DEFAULT 'pattern',
    scope TEXT DEFAULT 'project',
    confidence REAL DEFAULT 1.0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- CODE INTELLIGENCE
-- =======================================
CREATE TABLE IF NOT EXISTS code_symbols (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    start_line INTEGER,
    end_line INTEGER,
    signature TEXT,
    indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON code_symbols(project_id, file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);

CREATE TABLE IF NOT EXISTS call_graph (
    id INTEGER PRIMARY KEY,
    caller_id INTEGER REFERENCES code_symbols(id),
    callee_name TEXT NOT NULL,
    callee_id INTEGER REFERENCES code_symbols(id),
    call_count INTEGER DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_calls_caller ON call_graph(caller_id);
CREATE INDEX IF NOT EXISTS idx_calls_callee ON call_graph(callee_id);

CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    file_path TEXT NOT NULL,
    import_path TEXT NOT NULL,
    is_external INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(project_id, file_path);

CREATE TABLE IF NOT EXISTS codebase_modules (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    module_id TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    purpose TEXT,
    exports TEXT,
    depends_on TEXT,
    symbol_count INTEGER DEFAULT 0,
    line_count INTEGER DEFAULT 0,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, module_id)
);
CREATE INDEX IF NOT EXISTS idx_modules_project ON codebase_modules(project_id);

-- =======================================
-- SESSIONS & HISTORY
-- =======================================
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    status TEXT DEFAULT 'active',
    summary TEXT,
    started_at TEXT DEFAULT CURRENT_TIMESTAMP,
    last_activity TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id, last_activity DESC);

CREATE TABLE IF NOT EXISTS tool_history (
    id INTEGER PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id),
    tool_name TEXT NOT NULL,
    arguments TEXT,
    result_summary TEXT,
    full_result TEXT,
    success INTEGER DEFAULT 1,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_history_session ON tool_history(session_id);
CREATE INDEX IF NOT EXISTS idx_history_tool ON tool_history(tool_name);

-- =======================================
-- TASKS & GOALS
-- =======================================
CREATE TABLE IF NOT EXISTS goals (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'planning',
    priority TEXT DEFAULT 'medium',
    progress_percent INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS milestones (
    id INTEGER PRIMARY KEY,
    goal_id INTEGER REFERENCES goals(id),
    title TEXT NOT NULL,
    completed INTEGER DEFAULT 0,
    weight INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    goal_id INTEGER REFERENCES goals(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'pending',
    priority TEXT DEFAULT 'medium',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(project_id, status);

-- =======================================
-- PERMISSIONS
-- =======================================
CREATE TABLE IF NOT EXISTS permission_rules (
    id INTEGER PRIMARY KEY,
    tool_name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    match_type TEXT DEFAULT 'prefix',
    scope TEXT DEFAULT 'global',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- BACKGROUND PROCESSING
-- =======================================
CREATE TABLE IF NOT EXISTS pending_embeddings (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    file_path TEXT NOT NULL,
    chunk_content TEXT NOT NULL,
    start_line INTEGER NOT NULL DEFAULT 1,
    status TEXT DEFAULT 'pending',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_pending_embeddings_status ON pending_embeddings(status);

-- =======================================
-- PROJECT BRIEFINGS (What's New)
-- =======================================
CREATE TABLE IF NOT EXISTS project_briefings (
    id INTEGER PRIMARY KEY,
    project_id INTEGER UNIQUE REFERENCES projects(id),
    last_known_commit TEXT,           -- git HEAD hash when briefing was generated
    last_session_at TEXT,             -- timestamp of last session
    briefing_text TEXT,               -- DeepSeek-generated summary of changes
    generated_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_briefings_project ON project_briefings(project_id);

CREATE TABLE IF NOT EXISTS background_batches (
    id INTEGER PRIMARY KEY,
    batch_id TEXT NOT NULL,
    item_ids TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- SERVER STATE (for restart recovery)
-- =======================================
CREATE TABLE IF NOT EXISTS server_state (
    key TEXT PRIMARY KEY,
    value TEXT,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- CHAT MESSAGES (conversation history)
-- =======================================
CREATE TABLE IF NOT EXISTS chat_messages (
    id INTEGER PRIMARY KEY,
    role TEXT NOT NULL,  -- 'user', 'assistant'
    content TEXT NOT NULL,
    reasoning_content TEXT,  -- for deepseek reasoner responses
    summarized INTEGER DEFAULT 0,  -- 1 if included in a summary
    summary_id INTEGER REFERENCES chat_summaries(id) ON DELETE SET NULL,  -- links to the summary for reversibility
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_created ON chat_messages(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_chat_messages_summary ON chat_messages(summary_id);

CREATE TABLE IF NOT EXISTS chat_summaries (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,  -- NULL for global/legacy summaries
    summary TEXT NOT NULL,
    message_range_start INTEGER,  -- first message id covered
    message_range_end INTEGER,    -- last message id covered
    summary_level INTEGER DEFAULT 1,  -- 1=session, 2=daily, 3=weekly
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chat_summaries_level ON chat_summaries(summary_level, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_chat_summaries_project ON chat_summaries(project_id, summary_level, created_at DESC);

-- =======================================
-- VECTOR TABLES (sqlite-vec)
-- =======================================
CREATE VIRTUAL TABLE IF NOT EXISTS vec_memory USING vec0(
    embedding float[1536],
    +fact_id INTEGER,
    +content TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
    embedding float[1536],
    +file_path TEXT,
    +chunk_content TEXT,
    +project_id INTEGER,
    +start_line INTEGER
);

-- =======================================
-- CONFIGURATION
-- =======================================
CREATE TABLE IF NOT EXISTS system_prompts (
    role TEXT PRIMARY KEY,             -- 'architect', 'plan_reviewer', etc.
    prompt TEXT NOT NULL,              -- custom system prompt
    provider TEXT DEFAULT 'deepseek',  -- LLM provider: 'deepseek', 'gemini'
    model TEXT,                        -- custom model name (optional)
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =======================================
-- FULL-TEXT SEARCH (FTS5)
-- =======================================
-- High-performance keyword search for code
-- Rebuilt from vec_code after indexing
CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
    file_path,
    chunk_content,
    project_id UNINDEXED,  -- not searchable, just for filtering
    start_line UNINDEXED,
    content='',            -- contentless (we rebuild from vec_code)
    tokenize='porter unicode61 remove_diacritics 1'
);
"#;
