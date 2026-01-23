// db/schema.rs
// Database schema and migrations

use anyhow::Result;
use rusqlite::Connection;

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
    // Check if pending_embeddings exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='pending_embeddings'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if start_line column exists
    let has_column: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('pending_embeddings') WHERE name = 'start_line'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_column {
        tracing::info!("Migrating pending_embeddings to add start_line column");
        conn.execute(
            "ALTER TABLE pending_embeddings ADD COLUMN start_line INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }

    Ok(())
}

/// Migrate tool_history to add full_result column for complete tool output storage
pub fn migrate_tool_history_full_result(conn: &Connection) -> Result<()> {
    // Check if tool_history exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='tool_history'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if full_result column exists
    let has_column: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('tool_history') WHERE name='full_result'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_column {
        tracing::info!("Migrating tool_history to add full_result column");
        conn.execute(
            "ALTER TABLE tool_history ADD COLUMN full_result TEXT",
            [],
        )?;
    }

    Ok(())
}

/// Migrate memory_facts to add has_embedding column for tracking embedding status
pub fn migrate_memory_facts_has_embedding(conn: &Connection) -> Result<()> {
    // Check if memory_facts exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_facts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if has_embedding column exists
    let has_column: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('memory_facts') WHERE name='has_embedding'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_column {
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
    // Check if chat_messages exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='chat_messages'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if summary_id column exists
    let has_column: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('chat_messages') WHERE name='summary_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_column {
        tracing::info!("Migrating chat_messages to add summary_id column for reversible summarization");
        conn.execute(
            "ALTER TABLE chat_messages ADD COLUMN summary_id INTEGER REFERENCES chat_summaries(id) ON DELETE SET NULL",
            [],
        )?;
        // Add index for efficient lookup by summary
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_summary ON chat_messages(summary_id)",
            [],
        )?;
    }

    Ok(())
}

/// Migrate chat_summaries to add project_id column for multi-project separation
pub fn migrate_chat_summaries_project_id(conn: &Connection) -> Result<()> {
    // Check if chat_summaries exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='chat_summaries'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if project_id column exists
    let has_column: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('chat_summaries') WHERE name='project_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_column {
        tracing::info!("Migrating chat_summaries to add project_id column");
        conn.execute(
            "ALTER TABLE chat_summaries ADD COLUMN project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE",
            [],
        )?;
        // Add index for efficient project-scoped queries
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
    // Check if system_prompts exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='system_prompts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if provider column exists
    let has_provider: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('system_prompts') WHERE name='provider'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_provider {
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
    // Check if system_prompts exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='system_prompts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
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
        // Find the position of the tool usage suffix
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
    // Check if memory_facts exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_facts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if session_count column exists (indicator of migration status)
    let has_session_count: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('memory_facts') WHERE name='session_count'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_session_count {
        tracing::info!("Migrating memory_facts to add evidence-based tracking columns");
        conn.execute_batch(
            "ALTER TABLE memory_facts ADD COLUMN session_count INTEGER DEFAULT 1;
             ALTER TABLE memory_facts ADD COLUMN first_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN last_session_id TEXT;
             ALTER TABLE memory_facts ADD COLUMN status TEXT DEFAULT 'candidate';",
        )?;

        // Backfill: existing memories with high confidence are already 'confirmed'
        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed' WHERE confidence >= 0.8",
            [],
        )?;
    }

    // Create index for status-based queries (runs for both new and migrated databases)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_facts(status)",
        [],
    )?;

    Ok(())
}

/// Migrate imports table to add unique constraint and deduplicate
pub fn migrate_imports_unique(conn: &Connection) -> Result<()> {
    // Check if imports table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='imports'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !table_exists {
        return Ok(());
    }

    // Check if unique index already exists
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

    // Delete duplicate rows, keeping the one with the smallest id
    conn.execute_batch(
        "DELETE FROM imports
         WHERE id NOT IN (
             SELECT MIN(id)
             FROM imports
             GROUP BY project_id, file_path, import_path
         )"
    )?;

    // Create unique index
    conn.execute_batch("CREATE UNIQUE INDEX uniq_imports ON imports(project_id, file_path, import_path)")?;

    Ok(())
}

/// Migrate to add documentation tracking tables
pub fn migrate_documentation_tables(conn: &Connection) -> Result<()> {
    let tables_exist: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='documentation_tasks'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if tables_exist {
        return Ok(());
    }

    tracing::info!("Creating documentation tracking tables");

    conn.execute_batch(
        r#"
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
            WHERE status IN ('pending', 'draft_ready');

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
        "#
    )?;

    Ok(())
}

/// Migrate to add users table for multi-user support
pub fn migrate_users_table(conn: &Connection) -> Result<()> {
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='users'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if table_exists {
        return Ok(());
    }

    tracing::info!("Creating users table for multi-user support");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            identity TEXT UNIQUE NOT NULL,
            display_name TEXT,
            email TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_users_identity ON users(identity);
        "#
    )?;

    Ok(())
}

/// Migrate memory_facts to add user_id and scope columns for multi-user sharing
pub fn migrate_memory_user_scope(conn: &Connection) -> Result<()> {
    // Check if memory_facts exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_facts'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if user_id column exists
    let has_user_id: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('memory_facts') WHERE name='user_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_user_id {
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

    // Check if scope column exists
    let has_scope: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('memory_facts') WHERE name='scope'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_scope {
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

    // Check if team_id column exists
    let has_team_id: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('memory_facts') WHERE name='team_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_team_id {
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
    let teams_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='teams'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if teams_exists {
        return Ok(());
    }

    tracing::info!("Creating teams tables for team-based memory sharing");

    conn.execute_batch(
        r#"
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
        "#
    )?;

    Ok(())
}

/// Migrate to add review_findings table for code review learning loop
pub fn migrate_review_findings_table(conn: &Connection) -> Result<()> {
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='review_findings'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if table_exists {
        return Ok(());
    }

    tracing::info!("Creating review_findings table for code review learning loop");

    conn.execute_batch(
        r#"
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
        "#
    )?;

    Ok(())
}

/// Migrate corrections table to add learning columns
pub fn migrate_corrections_learning_columns(conn: &Connection) -> Result<()> {
    // Check if corrections table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='corrections'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(());
    }

    // Check if occurrence_count column exists
    let has_occurrence_count: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('corrections') WHERE name='occurrence_count'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !has_occurrence_count {
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
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='proxy_usage'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !table_exists {
        tracing::info!("Creating proxy_usage table for token tracking and cost estimation");

        conn.execute_batch(
            r#"
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
            "#
        )?;
    }

    // Add embeddings_usage table
    let embeddings_table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='embeddings_usage'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if !embeddings_table_exists {
        tracing::info!("Creating embeddings_usage table for embedding cost tracking");

        conn.execute_batch(
            r#"
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
            "#
        )?;
    }

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
    provider TEXT DEFAULT 'deepseek',  -- LLM provider: 'deepseek', 'openai', 'gemini'
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
