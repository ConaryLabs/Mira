// db/schema.rs
// Database schema and migrations

use anyhow::Result;
use rusqlite::Connection;

// Import migration helpers
use super::migration_helpers::{table_exists, column_exists, add_column_if_missing, create_table_if_missing};

/// Run all schema setup and migrations.
///
/// Called during database initialization. This function is idempotent -
/// it checks for existing tables/columns before making changes.
pub fn run_all_migrations(conn: &Connection) -> Result<()> {
    // Create base tables
    conn.execute_batch(SCHEMA)?;

    // Run migrations in order
    migrate_vec_tables(conn)?;
    migrate_pending_embeddings_line_numbers(conn)?;
    migrate_vec_code_line_numbers(conn)?;
    migrate_tool_history_full_result(conn)?;
    migrate_chat_summaries_project_id(conn)?;
    migrate_chat_messages_summary_id(conn)?;
    migrate_memory_facts_has_embedding(conn)?;
    migrate_memory_facts_evidence_tracking(conn)?;
    migrate_system_prompts_provider(conn)?;
    migrate_system_prompts_strip_tool_suffix(conn)?;
    migrate_code_fts(conn)?;
    migrate_imports_unique(conn)?;
    migrate_documentation_tables(conn)?;
    migrate_users_table(conn)?;
    migrate_memory_user_scope(conn)?;
    migrate_teams_tables(conn)?;

    // Add review findings table for code review learning loop
    migrate_review_findings_table(conn)?;

    // Add learning columns to corrections table
    migrate_corrections_learning_columns(conn)?;

    // Add proxy usage tracking table
    migrate_proxy_usage_table(conn)?;

    // Add diff analyses table for semantic diff analysis
    migrate_diff_analyses_table(conn)?;

    // Add proactive intelligence tables for behavior tracking
    migrate_proactive_intelligence_tables(conn)?;

    // Add evolutionary expert system tables
    migrate_evolutionary_expert_tables(conn)?;

    Ok(())
}

/// Migrate vector tables if dimensions changed
pub fn migrate_vec_tables(conn: &Connection) -> Result<()> {
    // Check if vec_memory exists and has wrong dimensions
    let needs_migration: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_memory_info'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if needs_migration {
        // Check current dimension by looking at chunk info
        let current_dim: Result<i64, _> = conn.query_row(
            "SELECT vector_column_size FROM vec_memory_info WHERE vector_column_name='embedding'",
            [],
            |row| row.get(0),
        );

        if let Ok(dim) = current_dim {
            if dim != 1536 {
                tracing::info!("Migrating vector tables from {} to 1536 dimensions", dim);
                // Drop old tables - CASCADE not supported, drop in order
                conn.execute_batch(
                    "DROP TABLE IF EXISTS vec_memory;
                     DROP TABLE IF EXISTS vec_code;"
                )?;
            }
        }
    }

    Ok(())
}

/// Migrate vec_code to add start_line column (v2.1 schema)
/// Also creates vec_code if it doesn't exist (for databases created before vec_code was added)
pub fn migrate_vec_code_line_numbers(conn: &Connection) -> Result<()> {
    // Check if vec_code exists
    let vec_code_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_code'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !vec_code_exists {
        // Create vec_code table (for databases created before this table was added to schema)
        tracing::info!("Creating vec_code table for code embeddings");
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
                embedding float[1536],
                +file_path TEXT,
                +chunk_content TEXT,
                +project_id INTEGER,
                +start_line INTEGER
            )",
            [],
        )?;
        return Ok(());
    }

    // Check if start_line column exists by checking vec_code_info
    let has_start_line: bool = conn
        .query_row(
            "SELECT 1 FROM vec_code_info WHERE auxiliary_column_name = 'start_line'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_start_line {
        tracing::info!("Migrating vec_code to add start_line column");
        // Virtual tables can't be altered - must drop and recreate
        // Embeddings will be regenerated on next indexing
        conn.execute("DROP TABLE IF EXISTS vec_code", [])?;
        // Recreate with start_line column
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_code USING vec0(
                embedding float[1536],
                +file_path TEXT,
                +chunk_content TEXT,
                +project_id INTEGER,
                +start_line INTEGER
            )",
            [],
        )?;
    }

    Ok(())
}

/// Migrate pending_embeddings to add start_line column
pub fn migrate_pending_embeddings_line_numbers(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "pending_embeddings") {
        return Ok(());
    }

    // Add column if missing
    add_column_if_missing(
        conn,
        "pending_embeddings",
        "start_line",
        "INTEGER NOT NULL DEFAULT 1"
    )
}

/// Migrate tool_history to add full_result column for complete tool output storage
pub fn migrate_tool_history_full_result(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "tool_history") {
        return Ok(());
    }

    // Add column if missing
    add_column_if_missing(
        conn,
        "tool_history",
        "full_result",
        "TEXT"
    )
}

/// Migrate memory_facts to add has_embedding column for tracking embedding status
pub fn migrate_memory_facts_has_embedding(conn: &Connection) -> Result<()> {
    // Early return if table doesn't exist
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    // Add column if missing (also handles backfill)
    if !column_exists(conn, "memory_facts", "has_embedding") {
        tracing::info!("Migrating memory_facts to add has_embedding column");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN has_embedding INTEGER DEFAULT 0",
            [],
        )?;
        // Backfill: mark existing facts that have embeddings
        conn.execute(
            "UPDATE memory_facts SET has_embedding = 1 WHERE id IN (SELECT fact_id FROM vec_memory)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate chat_messages to add summary_id for reversible summarization
pub fn migrate_chat_messages_summary_id(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "chat_messages") {
        return Ok(());
    }

    if !column_exists(conn, "chat_messages", "summary_id") {
        tracing::info!("Migrating chat_messages to add summary_id column");
        conn.execute(
            "ALTER TABLE chat_messages ADD COLUMN summary_id INTEGER REFERENCES chat_summaries(id) ON DELETE SET NULL",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_summary ON chat_messages(summary_id)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate chat_summaries to add project_id column for multi-project separation
pub fn migrate_chat_summaries_project_id(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "chat_summaries") {
        return Ok(());
    }

    if !column_exists(conn, "chat_summaries", "project_id") {
        tracing::info!("Migrating chat_summaries to add project_id column");
        conn.execute(
            "ALTER TABLE chat_summaries ADD COLUMN project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_summaries_project ON chat_summaries(project_id, summary_level, created_at DESC)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate to add FTS5 full-text search table
pub fn migrate_code_fts(conn: &Connection) -> Result<()> {
    // Check if code_fts exists
    let fts_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='code_fts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !fts_exists {
        tracing::info!("Creating FTS5 full-text search table for code");
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
                file_path,
                chunk_content,
                project_id UNINDEXED,
                start_line UNINDEXED,
                content='',
                tokenize='porter unicode61 remove_diacritics 1'
            );",
        )?;

        // Populate from existing vec_code data
        rebuild_code_fts(conn)?;
    }

    Ok(())
}

/// Rebuild the FTS5 index from vec_code
/// Call this after indexing or when FTS index needs refreshing
pub fn rebuild_code_fts(conn: &Connection) -> Result<()> {
    tracing::info!("Rebuilding FTS5 code search index");

    // Clear existing FTS data
    conn.execute("DELETE FROM code_fts", [])?;

    // Populate from vec_code
    let inserted = conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT rowid, file_path, chunk_content, project_id, start_line FROM vec_code",
        [],
    )?;

    tracing::info!("FTS5 index rebuilt with {} entries", inserted);
    Ok(())
}

/// Rebuild FTS5 index for a specific project
pub fn rebuild_code_fts_for_project(conn: &Connection, project_id: i64) -> Result<()> {
    tracing::debug!("Rebuilding FTS5 index for project {}", project_id);

    // Delete existing entries for this project
    conn.execute("DELETE FROM code_fts WHERE project_id = ?", [project_id])?;

    // Re-insert from vec_code
    conn.execute(
        "INSERT INTO code_fts(rowid, file_path, chunk_content, project_id, start_line)
         SELECT rowid, file_path, chunk_content, project_id, start_line
         FROM vec_code WHERE project_id = ?",
        [project_id],
    )?;

    Ok(())
}

/// Migrate system_prompts to add provider and model columns
pub fn migrate_system_prompts_provider(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "system_prompts") {
        return Ok(());
    }

    if !column_exists(conn, "system_prompts", "provider") {
        tracing::info!("Adding provider and model columns to system_prompts");
        conn.execute_batch(
            "ALTER TABLE system_prompts ADD COLUMN provider TEXT DEFAULT 'deepseek';
             ALTER TABLE system_prompts ADD COLUMN model TEXT;",
        )?;
    }

    Ok(())
}

/// Migrate system_prompts to strip old TOOL_USAGE_PROMPT suffix for KV cache optimization
pub fn migrate_system_prompts_strip_tool_suffix(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "system_prompts") {
        return Ok(());
    }

    // Get all prompts that might contain the old tool usage suffix
    let mut stmt = conn.prepare("SELECT role, prompt FROM system_prompts WHERE prompt LIKE '%Use tools to explore codebase before analysis.%'")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(Result::ok)
        .collect();

    if rows.is_empty() {
        return Ok(());
    }

    tracing::info!("Migrating {} system prompts to strip old tool usage suffix", rows.len());

    for (role, prompt) in rows {
        if let Some(pos) = prompt.find("Use tools to explore codebase before analysis.") {
            let stripped = prompt[..pos].trim_end().to_string();
            conn.execute(
                "UPDATE system_prompts SET prompt = ? WHERE role = ?",
                [&stripped, &role],
            )?;
            tracing::debug!("Stripped tool usage suffix from prompt for role: {}", role);
        }
    }

    Ok(())
}

/// Migrate memory_facts to add evidence-based tracking columns
pub fn migrate_memory_facts_evidence_tracking(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "session_count") {
        tracing::info!("Migrating memory_facts to add evidence-based tracking columns");
        conn.execute_batch(
            "ALTER TABLE memory_facts ADD COLUMN session_count INTEGER DEFAULT 1;
             ALTER TABLE memory_facts ADD COLUMN first_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN last_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN status TEXT DEFAULT 'candidate';",
        )?;

        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed' WHERE confidence >= 0.8",
            [],
        )?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_facts(status)",
        [],
    )?;

    Ok(())
}

/// Migrate imports table to add unique constraint and deduplicate
pub fn migrate_imports_unique(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "imports") {
        return Ok(());
    }

    let index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='index' AND name='uniq_imports'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if index_exists {
        return Ok(());
    }

    tracing::info!("Deduplicating imports and adding unique constraint");

    conn.execute_batch(
        "DELETE FROM imports
         WHERE id NOT IN (
             SELECT MIN(id)
             FROM imports
             GROUP BY project_id, file_path, import_path
         )"
    )?;

    conn.execute_batch("CREATE UNIQUE INDEX uniq_imports ON imports(project_id, file_path, import_path)")?;

    Ok(())
}

/// Migrate to add documentation tracking tables
pub fn migrate_documentation_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "documentation_tasks", r#"
        CREATE TABLE IF NOT EXISTS documentation_tasks (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            doc_type TEXT NOT NULL,
            doc_category TEXT NOT NULL,
            source_file_path TEXT,
            target_doc_path TEXT NOT NULL,
            priority TEXT DEFAULT 'medium',
            status TEXT DEFAULT 'pending',
            reason TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            git_commit TEXT,
            -- Safety rails for concurrent edits
            source_signature_hash TEXT,
            target_doc_checksum_at_generation TEXT,
            -- Draft content with preview for list views
            draft_content TEXT,
            draft_preview TEXT,
            draft_sha256 TEXT,
            draft_generated_at TEXT,
            reviewed_at TEXT,
            applied_at TEXT,
            -- Retry tracking
            retry_count INTEGER DEFAULT 0,
            last_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_status ON documentation_tasks(project_id, status);
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_type ON documentation_tasks(doc_type, doc_category);
        CREATE INDEX IF NOT EXISTS idx_doc_tasks_priority ON documentation_tasks(project_id, priority, status);
        -- Uniqueness constraint to prevent duplicate tasks for same target
        CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_tasks_unique
            ON documentation_tasks(project_id, target_doc_path, doc_type, doc_category)
            WHERE status = 'pending';

        CREATE TABLE IF NOT EXISTS documentation_inventory (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            doc_path TEXT NOT NULL,
            doc_type TEXT NOT NULL,
            doc_category TEXT,
            title TEXT,
            -- Normalized source signature hash (not raw checksum)
            source_signature_hash TEXT,
            source_symbols TEXT,
            last_seen_commit TEXT,
            is_stale INTEGER DEFAULT 0,
            staleness_reason TEXT,
            verified_at TEXT DEFAULT CURRENT_TIMESTAMP,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, doc_path)
        );
        CREATE INDEX IF NOT EXISTS idx_doc_inventory_stale ON documentation_inventory(project_id, is_stale);
    "#)
}

/// Migrate to add users table for multi-user support
pub fn migrate_users_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "users", r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            identity TEXT UNIQUE NOT NULL,
            display_name TEXT,
            email TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_users_identity ON users(identity);
    "#)
}

/// Migrate memory_facts to add user_id and scope columns for multi-user sharing
pub fn migrate_memory_user_scope(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "memory_facts") {
        return Ok(());
    }

    if !column_exists(conn, "memory_facts", "user_id") {
        tracing::info!("Adding user_id column to memory_facts for multi-user support");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN user_id TEXT",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_user ON memory_facts(user_id)",
            [],
        )?;
    }

    if !column_exists(conn, "memory_facts", "scope") {
        tracing::info!("Adding scope column to memory_facts for visibility control");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN scope TEXT DEFAULT 'project'",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_scope ON memory_facts(scope)",
            [],
        )?;
    }

    if !column_exists(conn, "memory_facts", "team_id") {
        tracing::info!("Adding team_id column to memory_facts for team sharing");
        conn.execute(
            "ALTER TABLE memory_facts ADD COLUMN team_id INTEGER",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_team ON memory_facts(team_id)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate to add teams tables for team-based memory sharing
pub fn migrate_teams_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "teams", r#"
        CREATE TABLE IF NOT EXISTS teams (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            created_by TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_teams_name ON teams(name);

        CREATE TABLE IF NOT EXISTS team_members (
            id INTEGER PRIMARY KEY,
            team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
            user_identity TEXT NOT NULL,
            role TEXT DEFAULT 'member',
            joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(team_id, user_identity)
        );
        CREATE INDEX IF NOT EXISTS idx_team_members_team ON team_members(team_id);
        CREATE INDEX IF NOT EXISTS idx_team_members_user ON team_members(user_identity);
    "#)
}

/// Migrate to add review_findings table for code review learning loop
pub fn migrate_review_findings_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "review_findings", r#"
        CREATE TABLE IF NOT EXISTS review_findings (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            expert_role TEXT NOT NULL,
            file_path TEXT,
            finding_type TEXT NOT NULL,
            severity TEXT DEFAULT 'medium',
            content TEXT NOT NULL,
            code_snippet TEXT,
            suggestion TEXT,
            status TEXT DEFAULT 'pending',
            feedback TEXT,
            confidence REAL DEFAULT 0.5,
            user_id TEXT,
            reviewed_by TEXT,
            session_id TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            reviewed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_review_findings_project ON review_findings(project_id, status);
        CREATE INDEX IF NOT EXISTS idx_review_findings_expert ON review_findings(expert_role);
        CREATE INDEX IF NOT EXISTS idx_review_findings_file ON review_findings(file_path);
        CREATE INDEX IF NOT EXISTS idx_review_findings_status ON review_findings(status);
    "#)
}

/// Migrate corrections table to add learning columns
pub fn migrate_corrections_learning_columns(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "corrections") {
        return Ok(());
    }

    if !column_exists(conn, "corrections", "occurrence_count") {
        tracing::info!("Adding learning columns to corrections table");
        conn.execute_batch(
            "ALTER TABLE corrections ADD COLUMN occurrence_count INTEGER DEFAULT 1;
             ALTER TABLE corrections ADD COLUMN acceptance_rate REAL DEFAULT 1.0;",
        )?;
    }

    Ok(())
}

/// Migrate to add proxy_usage table for token tracking and cost estimation
pub fn migrate_proxy_usage_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "proxy_usage", r#"
        CREATE TABLE IF NOT EXISTS proxy_usage (
            id INTEGER PRIMARY KEY,
            backend_name TEXT NOT NULL,
            model TEXT,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_creation_tokens INTEGER DEFAULT 0,
            cache_read_tokens INTEGER DEFAULT 0,
            cost_estimate REAL,
            request_id TEXT,
            session_id TEXT,
            project_id INTEGER REFERENCES projects(id),
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_backend ON proxy_usage(backend_name, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_session ON proxy_usage(session_id);
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_project ON proxy_usage(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_created ON proxy_usage(created_at DESC);
    "#)?;

    // Add embeddings_usage table
    create_table_if_missing(conn, "embeddings_usage", r#"
        CREATE TABLE IF NOT EXISTS embeddings_usage (
            id INTEGER PRIMARY KEY,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            tokens INTEGER NOT NULL,
            text_count INTEGER NOT NULL,
            cost_estimate REAL,
            project_id INTEGER REFERENCES projects(id),
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_provider ON embeddings_usage(provider, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_project ON embeddings_usage(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_embeddings_usage_created ON embeddings_usage(created_at DESC);
    "#)
}

/// Migrate to add diff_analyses table for semantic diff analysis
pub fn migrate_diff_analyses_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(conn, "diff_analyses", r#"
        CREATE TABLE IF NOT EXISTS diff_analyses (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            from_commit TEXT NOT NULL,
            to_commit TEXT NOT NULL,
            analysis_type TEXT DEFAULT 'commit',
            changes_json TEXT,
            impact_json TEXT,
            risk_json TEXT,
            summary TEXT,
            files_changed INTEGER,
            lines_added INTEGER,
            lines_removed INTEGER,
            status TEXT DEFAULT 'complete',
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_diff_commits ON diff_analyses(project_id, from_commit, to_commit);
        CREATE INDEX IF NOT EXISTS idx_diff_created ON diff_analyses(project_id, created_at DESC);
    "#)
}

/// Migrate to add proactive intelligence tables for behavior tracking and predictions
pub fn migrate_proactive_intelligence_tables(conn: &Connection) -> Result<()> {
    // Behavior patterns table - tracks file sequences, tool chains, session flows
    create_table_if_missing(conn, "behavior_patterns", r#"
        CREATE TABLE IF NOT EXISTS behavior_patterns (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            pattern_type TEXT NOT NULL,      -- 'file_sequence', 'tool_chain', 'session_flow', 'query_pattern'
            pattern_key TEXT NOT NULL,       -- unique identifier for the pattern (e.g., hash of sequence)
            pattern_data TEXT NOT NULL,      -- JSON: sequence details, items, transitions
            confidence REAL DEFAULT 0.5,     -- how reliable this pattern is (0.0-1.0)
            occurrence_count INTEGER DEFAULT 1,
            last_triggered_at TEXT,
            first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, pattern_type, pattern_key)
        );
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_project ON behavior_patterns(project_id, pattern_type);
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_confidence ON behavior_patterns(confidence DESC);
        CREATE INDEX IF NOT EXISTS idx_behavior_patterns_recent ON behavior_patterns(last_triggered_at DESC);
    "#)?;

    // Proactive interventions table - tracks what we suggested and user response
    create_table_if_missing(conn, "proactive_interventions", r#"
        CREATE TABLE IF NOT EXISTS proactive_interventions (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            session_id TEXT,
            intervention_type TEXT NOT NULL,  -- 'context_prediction', 'security_alert', 'bug_warning', 'resource_suggestion'
            trigger_pattern_id INTEGER REFERENCES behavior_patterns(id),
            trigger_context TEXT,             -- what triggered this intervention
            suggestion_content TEXT NOT NULL, -- what we suggested
            confidence REAL DEFAULT 0.5,      -- how confident we were
            user_response TEXT,               -- 'accepted', 'dismissed', 'acted_upon', 'ignored', NULL if pending
            response_time_ms INTEGER,         -- how long user took to respond (NULL if ignored)
            effectiveness_score REAL,         -- computed based on response and subsequent actions
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            responded_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_interventions_project ON proactive_interventions(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_interventions_type ON proactive_interventions(intervention_type, user_response);
        CREATE INDEX IF NOT EXISTS idx_interventions_pattern ON proactive_interventions(trigger_pattern_id);
        CREATE INDEX IF NOT EXISTS idx_interventions_pending ON proactive_interventions(user_response) WHERE user_response IS NULL;
    "#)?;

    // Session behavior log - raw events for pattern mining
    create_table_if_missing(conn, "session_behavior_log", r#"
        CREATE TABLE IF NOT EXISTS session_behavior_log (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,         -- 'file_access', 'tool_use', 'query', 'context_switch'
            event_data TEXT NOT NULL,         -- JSON: file_path, tool_name, query_text, etc.
            sequence_position INTEGER,        -- position in session sequence
            time_since_last_event_ms INTEGER, -- milliseconds since previous event
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_behavior_log_session ON session_behavior_log(session_id, sequence_position);
        CREATE INDEX IF NOT EXISTS idx_behavior_log_project ON session_behavior_log(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_behavior_log_type ON session_behavior_log(event_type, created_at DESC);
    "#)?;

    // User preferences for proactive features
    create_table_if_missing(conn, "proactive_preferences", r#"
        CREATE TABLE IF NOT EXISTS proactive_preferences (
            id INTEGER PRIMARY KEY,
            user_id TEXT,
            project_id INTEGER REFERENCES projects(id),
            preference_key TEXT NOT NULL,     -- 'proactivity_level', 'max_alerts_per_hour', 'min_confidence'
            preference_value TEXT NOT NULL,   -- JSON value
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(user_id, project_id, preference_key)
        );
        CREATE INDEX IF NOT EXISTS idx_proactive_prefs_user ON proactive_preferences(user_id, project_id);
    "#)?;

    Ok(())
}

/// Migrate to add evolutionary expert system tables
pub fn migrate_evolutionary_expert_tables(conn: &Connection) -> Result<()> {
    // Expert consultations - detailed history of each consultation
    create_table_if_missing(conn, "expert_consultations", r#"
        CREATE TABLE IF NOT EXISTS expert_consultations (
            id INTEGER PRIMARY KEY,
            expert_role TEXT NOT NULL,
            project_id INTEGER REFERENCES projects(id),
            session_id TEXT,
            context_hash TEXT,                -- Hash of context for pattern matching
            problem_category TEXT,            -- Categorized problem type
            context_summary TEXT,             -- Brief summary of the consultation context
            tools_used TEXT,                  -- JSON array of tools called
            tool_call_count INTEGER DEFAULT 0,
            consultation_duration_ms INTEGER,
            initial_confidence REAL,          -- Expert's stated confidence
            calibrated_confidence REAL,       -- Adjusted based on history
            prompt_version INTEGER DEFAULT 1, -- Which prompt version was used
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_expert_consultations_role ON expert_consultations(expert_role, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_expert_consultations_project ON expert_consultations(project_id, expert_role);
        CREATE INDEX IF NOT EXISTS idx_expert_consultations_category ON expert_consultations(problem_category);
    "#)?;

    // Problem patterns - recurring problem signatures per expert
    create_table_if_missing(conn, "problem_patterns", r#"
        CREATE TABLE IF NOT EXISTS problem_patterns (
            id INTEGER PRIMARY KEY,
            expert_role TEXT NOT NULL,
            pattern_signature TEXT NOT NULL,  -- Hash of problem characteristics
            pattern_description TEXT,         -- Human-readable pattern description
            common_context_elements TEXT,     -- JSON: what context elements appear together
            successful_approaches TEXT,       -- JSON: which analysis approaches work best
            recommended_tools TEXT,           -- JSON: which tools yield best results
            success_rate REAL DEFAULT 0.5,
            occurrence_count INTEGER DEFAULT 1,
            avg_confidence REAL DEFAULT 0.5,
            avg_acceptance_rate REAL DEFAULT 0.5,
            last_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(expert_role, pattern_signature)
        );
        CREATE INDEX IF NOT EXISTS idx_problem_patterns_role ON problem_patterns(expert_role, success_rate DESC);
    "#)?;

    // Expert outcomes - track whether advice led to good results
    create_table_if_missing(conn, "expert_outcomes", r#"
        CREATE TABLE IF NOT EXISTS expert_outcomes (
            id INTEGER PRIMARY KEY,
            consultation_id INTEGER REFERENCES expert_consultations(id),
            finding_id INTEGER REFERENCES review_findings(id),
            outcome_type TEXT NOT NULL,       -- 'code_change', 'design_adoption', 'bug_fix', 'security_fix'
            git_commit_hash TEXT,             -- If advice led to code change
            files_changed TEXT,               -- JSON array of changed files
            change_similarity_score REAL,     -- How closely change matches suggestion
            user_outcome_rating REAL,         -- User-provided rating (0-1)
            outcome_evidence TEXT,            -- JSON: links to tests, metrics, etc.
            time_to_outcome_seconds INTEGER,  -- How long until outcome realized
            learned_lesson TEXT,              -- What pattern we learned
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            verified_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_expert_outcomes_consultation ON expert_outcomes(consultation_id);
        CREATE INDEX IF NOT EXISTS idx_expert_outcomes_finding ON expert_outcomes(finding_id);
    "#)?;

    // Expert prompt evolution - track prompt versions and their performance
    create_table_if_missing(conn, "expert_prompt_versions", r#"
        CREATE TABLE IF NOT EXISTS expert_prompt_versions (
            id INTEGER PRIMARY KEY,
            expert_role TEXT NOT NULL,
            version INTEGER NOT NULL,
            prompt_additions TEXT,            -- Additional context added to base prompt
            performance_metrics TEXT,         -- JSON: acceptance_rate, outcome_success, etc.
            adaptation_reason TEXT,           -- Why this version was created
            consultation_count INTEGER DEFAULT 0,
            acceptance_rate REAL DEFAULT 0.5,
            is_active INTEGER DEFAULT 1,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(expert_role, version)
        );
        CREATE INDEX IF NOT EXISTS idx_expert_prompts_active ON expert_prompt_versions(expert_role, is_active);
    "#)?;

    // Expert collaboration patterns - when experts should work together
    create_table_if_missing(conn, "collaboration_patterns", r#"
        CREATE TABLE IF NOT EXISTS collaboration_patterns (
            id INTEGER PRIMARY KEY,
            problem_domains TEXT NOT NULL,    -- JSON: which expertise domains involved
            complexity_threshold REAL,        -- Min complexity score to trigger
            recommended_experts TEXT NOT NULL,-- JSON: which experts to involve
            collaboration_mode TEXT,          -- 'parallel', 'sequential', 'hierarchical'
            synthesis_method TEXT,            -- How to combine outputs
            success_rate REAL DEFAULT 0.5,
            time_saved_percent REAL,          -- Efficiency vs individual consultations
            occurrence_count INTEGER DEFAULT 1,
            last_used_at TEXT DEFAULT CURRENT_TIMESTAMP,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_collab_patterns_domains ON collaboration_patterns(problem_domains);
    "#)?;

    Ok(())
}

/// Database schema SQL
pub const SCHEMA: &str = r#"
-- ═══════════════════════════════════════
-- CORE: Projects
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- MEMORY: Semantic Facts
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- CODE INTELLIGENCE
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- SESSIONS & HISTORY
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- TASKS & GOALS
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- PERMISSIONS
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS permission_rules (
    id INTEGER PRIMARY KEY,
    tool_name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    match_type TEXT DEFAULT 'prefix',
    scope TEXT DEFAULT 'global',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- BACKGROUND PROCESSING
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- PROJECT BRIEFINGS (What's New)
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- SERVER STATE (for restart recovery)
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS server_state (
    key TEXT PRIMARY KEY,
    value TEXT,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- CHAT MESSAGES (conversation history)
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- VECTOR TABLES (sqlite-vec)
-- ═══════════════════════════════════════
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

-- ═══════════════════════════════════════
-- CONFIGURATION
-- ═══════════════════════════════════════
CREATE TABLE IF NOT EXISTS system_prompts (
    role TEXT PRIMARY KEY,             -- 'architect', 'plan_reviewer', etc.
    prompt TEXT NOT NULL,              -- custom system prompt
    provider TEXT DEFAULT 'deepseek',  -- LLM provider: 'deepseek', 'gemini'
    model TEXT,                        -- custom model name (optional)
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- ═══════════════════════════════════════
-- FULL-TEXT SEARCH (FTS5)
-- ═══════════════════════════════════════
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
