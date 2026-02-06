// crates/mira-server/src/db/schema/mod.rs
// Database schema and migrations

use anyhow::Result;
use rusqlite::Connection;

pub mod code;
mod entities;
mod fts;
mod intelligence;
mod memory;
mod reviews;
mod session;
mod session_tasks;
mod system;
pub(crate) mod vectors;

// Re-export FTS functions that are used elsewhere
pub use fts::{rebuild_code_fts, rebuild_code_fts_for_project};
// Re-export code schema migrations for the code database pool
pub use code::run_code_migrations;

/// Run all schema setup and migrations.
///
/// Called during database initialization. This function is idempotent -
/// it checks for existing tables/columns before making changes.
pub fn run_all_migrations(conn: &Connection) -> Result<()> {
    // Create base tables
    conn.execute_batch(SCHEMA)?;

    // Run migrations in order
    // Note: code-DB-specific migrations (vec_code, pending_embeddings, code_fts, imports)
    // are handled by run_code_migrations() on the separate code database.
    vectors::migrate_vec_tables(conn)?;
    session::migrate_tool_history_full_result(conn)?;
    session::migrate_chat_summaries_project_id(conn)?;
    session::migrate_chat_messages_summary_id(conn)?;
    memory::migrate_memory_facts_has_embedding(conn)?;
    memory::migrate_memory_facts_evidence_tracking(conn)?;
    system::migrate_system_prompts_provider(conn)?;
    system::migrate_system_prompts_strip_tool_suffix(conn)?;
    memory::migrate_documentation_tables(conn)?;
    memory::migrate_documentation_impact_analysis(conn)?;
    memory::migrate_users_table(conn)?;
    memory::migrate_memory_user_scope(conn)?;
    memory::migrate_drop_teams_tables(conn)?;

    // Add review findings table for code review learning loop
    reviews::migrate_review_findings_table(conn)?;

    // Add learning columns to corrections table
    reviews::migrate_corrections_learning_columns(conn)?;

    // Add embeddings usage tracking table
    reviews::migrate_embeddings_usage_table(conn)?;

    // Add diff analyses table for semantic diff analysis
    reviews::migrate_diff_analyses_table(conn)?;

    // Add files_json column to diff_analyses for outcome tracking
    reviews::migrate_diff_analyses_files_json(conn)?;

    // Add diff_outcomes table for tracking change outcomes
    reviews::migrate_diff_outcomes_table(conn)?;

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

    // Remove orphaned capability data (check_capability tool removed)
    memory::migrate_remove_capability_data(conn)?;

    // Add tech debt scores table for per-module debt tracking
    migrate_tech_debt_scores(conn)?;

    // Add module conventions table for convention-aware context injection
    migrate_module_conventions(conn)?;

    // Add entity tables for memory entity linking (recall boost)
    entities::migrate_entity_tables(conn)?;

    // Add session_tasks tables for Claude Code task persistence bridge
    session_tasks::migrate_session_tasks_tables(conn)?;

    // Add source and resumed_from columns for session resume tracking
    session::migrate_sessions_resume(conn)?;

    Ok(())
}

/// Add tech_debt_scores table for per-module composite debt scoring
fn migrate_tech_debt_scores(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::create_table_if_missing;
    create_table_if_missing(
        conn,
        "tech_debt_scores",
        r#"
        CREATE TABLE IF NOT EXISTS tech_debt_scores (
            id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL,
            module_id TEXT NOT NULL,
            module_path TEXT NOT NULL,
            overall_score REAL NOT NULL,
            tier TEXT NOT NULL,
            factor_scores TEXT NOT NULL,
            line_count INTEGER,
            finding_count INTEGER,
            computed_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, module_id)
        );
        CREATE INDEX IF NOT EXISTS idx_tech_debt_project ON tech_debt_scores(project_id);
        CREATE INDEX IF NOT EXISTS idx_tech_debt_tier ON tech_debt_scores(project_id, tier);
    "#,
    )
}

/// Add module_conventions table for convention-aware context injection
fn migrate_module_conventions(conn: &Connection) -> Result<()> {
    use crate::db::migration_helpers::create_table_if_missing;
    create_table_if_missing(
        conn,
        "module_conventions",
        r#"
        CREATE TABLE IF NOT EXISTS module_conventions (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            module_id TEXT NOT NULL,
            module_path TEXT NOT NULL,
            error_handling TEXT,
            test_pattern TEXT,
            key_imports TEXT,
            naming TEXT,
            detected_patterns TEXT,
            confidence REAL DEFAULT 0.7,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, module_path)
        );
        CREATE INDEX IF NOT EXISTS idx_module_conventions_project ON module_conventions(project_id);
    "#,
    )
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
    project_id INTEGER,  -- NULL for global summaries
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

"#;
