// crates/mira-server/src/db/schema/mod.rs
// Database schema and migrations

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

pub mod code;
mod entities;
mod fts;
mod intelligence;
mod memory;
mod reviews;
mod session;
mod session_tasks;
mod system;
pub mod team;
pub(crate) mod vectors;

// Re-export FTS functions that are used elsewhere
pub use fts::{rebuild_code_fts, rebuild_code_fts_for_project};
// Re-export code schema migrations for the code database pool
pub use code::run_code_migrations;

/// A versioned migration entry: (version, name, function).
/// Version numbers are sequential and must never be reordered or reused.
type MigrationFn = fn(&Connection) -> Result<()>;

struct Migration {
    version: u32,
    name: &'static str,
    func: MigrationFn,
}

/// Registry of all versioned migrations. New migrations go at the end with the
/// next sequential version number.
fn migration_registry() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "vec_tables",
            func: |c| vectors::migrate_vec_tables(c),
        },
        Migration {
            version: 2,
            name: "tool_history_full_result",
            func: session::migrate_tool_history_full_result,
        },
        Migration {
            version: 3,
            name: "chat_summaries_project_id",
            func: session::migrate_chat_summaries_project_id,
        },
        Migration {
            version: 4,
            name: "chat_messages_summary_id",
            func: session::migrate_chat_messages_summary_id,
        },
        Migration {
            version: 5,
            name: "memory_facts_has_embedding",
            func: memory::migrate_memory_facts_has_embedding,
        },
        Migration {
            version: 6,
            name: "memory_facts_evidence_tracking",
            func: memory::migrate_memory_facts_evidence_tracking,
        },
        Migration {
            version: 7,
            name: "system_prompts_provider",
            func: system::migrate_system_prompts_provider,
        },
        Migration {
            version: 8,
            name: "system_prompts_strip_suffix",
            func: system::migrate_system_prompts_strip_tool_suffix,
        },
        Migration {
            version: 9,
            name: "documentation_tables",
            func: memory::migrate_documentation_tables,
        },
        Migration {
            version: 10,
            name: "documentation_impact_analysis",
            func: memory::migrate_documentation_impact_analysis,
        },
        Migration {
            version: 11,
            name: "users_table",
            func: memory::migrate_users_table,
        },
        Migration {
            version: 12,
            name: "memory_user_scope",
            func: memory::migrate_memory_user_scope,
        },
        Migration {
            version: 13,
            name: "drop_teams_tables",
            func: memory::migrate_drop_teams_tables,
        },
        Migration {
            version: 14,
            name: "corrections_learning_columns",
            func: reviews::migrate_corrections_learning_columns,
        },
        Migration {
            version: 15,
            name: "embeddings_usage_table",
            func: reviews::migrate_embeddings_usage_table,
        },
        Migration {
            version: 16,
            name: "diff_analyses_table",
            func: reviews::migrate_diff_analyses_table,
        },
        Migration {
            version: 17,
            name: "diff_analyses_files_json",
            func: reviews::migrate_diff_analyses_files_json,
        },
        Migration {
            version: 18,
            name: "diff_outcomes_table",
            func: reviews::migrate_diff_outcomes_table,
        },
        Migration {
            version: 19,
            name: "llm_usage_table",
            func: reviews::migrate_llm_usage_table,
        },
        Migration {
            version: 20,
            name: "proactive_intelligence_tables",
            func: intelligence::migrate_proactive_intelligence_tables,
        },
        Migration {
            version: 21,
            name: "cross_project_intelligence",
            func: intelligence::migrate_cross_project_intelligence_tables,
        },
        Migration {
            version: 22,
            name: "memory_facts_branch",
            func: memory::migrate_memory_facts_branch,
        },
        Migration {
            version: 23,
            name: "sessions_branch",
            func: session::migrate_sessions_branch,
        },
        Migration {
            version: 24,
            name: "remove_capability_data",
            func: memory::migrate_remove_capability_data,
        },
        Migration {
            version: 25,
            name: "tech_debt_scores",
            func: migrate_tech_debt_scores,
        },
        Migration {
            version: 26,
            name: "module_conventions",
            func: migrate_module_conventions,
        },
        Migration {
            version: 27,
            name: "entity_tables",
            func: entities::migrate_entity_tables,
        },
        Migration {
            version: 28,
            name: "session_tasks_tables",
            func: session_tasks::migrate_session_tasks_tables,
        },
        Migration {
            version: 29,
            name: "sessions_resume",
            func: session::migrate_sessions_resume,
        },
        Migration {
            version: 30,
            name: "team_tables",
            func: team::migrate_team_tables,
        },
        Migration {
            version: 31,
            name: "session_snapshots",
            func: session::migrate_session_snapshots_table,
        },
    ]
}

/// Ensure the schema_versions tracking table exists.
fn ensure_schema_versions_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_versions (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT DEFAULT CURRENT_TIMESTAMP
        )",
    )?;
    Ok(())
}

/// Query which migration versions have already been applied.
fn applied_versions(conn: &Connection) -> Result<HashSet<u32>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_versions")?;
    let versions = stmt
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(versions)
}

/// Record a migration as applied.
fn record_migration(conn: &Connection, version: u32, name: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_versions (version, name) VALUES (?1, ?2)",
        rusqlite::params![version, name],
    )?;
    Ok(())
}

/// Run all schema setup and migrations.
///
/// Called during database initialization. Uses a schema_versions table to track
/// which migrations have been applied, skipping already-applied ones. On first
/// run (or upgrade from unversioned), all migrations execute and then get recorded.
pub fn run_all_migrations(conn: &Connection) -> Result<()> {
    // Create base tables (always idempotent via IF NOT EXISTS)
    conn.execute_batch(SCHEMA)?;

    // Ensure version tracking table exists
    ensure_schema_versions_table(conn)?;

    // Get already-applied versions
    let applied = applied_versions(conn)?;

    // Run each migration in order, skipping already-applied ones
    let registry = migration_registry();
    let mut ran = 0u32;
    for migration in &registry {
        if applied.contains(&migration.version) {
            continue;
        }
        tracing::info!(
            "Running migration v{}: {}",
            migration.version,
            migration.name
        );
        (migration.func)(conn)?;
        record_migration(conn, migration.version, migration.name)?;
        ran += 1;
    }

    if ran > 0 {
        tracing::info!("Applied {} new migration(s)", ran);
    }

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
CREATE INDEX IF NOT EXISTS idx_sessions_status_activity ON sessions(status, last_activity DESC);

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
CREATE INDEX IF NOT EXISTS idx_goals_project_status_created ON goals(project_id, status, created_at DESC, id DESC);

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
CREATE INDEX IF NOT EXISTS idx_tasks_project_status_created ON tasks(project_id, status, created_at DESC, id DESC);

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
    provider TEXT DEFAULT 'deepseek',  -- LLM provider: 'deepseek'
    model TEXT,                        -- custom model name (optional)
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

"#;
