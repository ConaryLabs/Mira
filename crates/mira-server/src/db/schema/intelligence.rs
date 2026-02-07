// crates/mira-server/src/db/schema/intelligence.rs
// Proactive intelligence, expert system, and cross-project migrations

use crate::db::migration_helpers::{column_exists, create_table_if_missing};
use anyhow::Result;
use rusqlite::Connection;

/// Migrate to add proactive intelligence tables for behavior tracking and predictions
pub fn migrate_proactive_intelligence_tables(conn: &Connection) -> Result<()> {
    // Behavior patterns table - tracks file sequences, tool chains, session flows
    create_table_if_missing(
        conn,
        "behavior_patterns",
        r#"
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
    "#,
    )?;

    // Proactive interventions table - tracks what we suggested and user response
    create_table_if_missing(
        conn,
        "proactive_interventions",
        r#"
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
    "#,
    )?;

    // Session behavior log - raw events for pattern mining
    create_table_if_missing(
        conn,
        "session_behavior_log",
        r#"
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
    "#,
    )?;

    // User preferences for proactive features
    create_table_if_missing(
        conn,
        "proactive_preferences",
        r#"
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
    "#,
    )?;

    // Pre-generated proactive suggestions table - for fast O(1) lookup
    create_table_if_missing(
        conn,
        "proactive_suggestions",
        r#"
        CREATE TABLE IF NOT EXISTS proactive_suggestions (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            pattern_id INTEGER REFERENCES behavior_patterns(id),
            trigger_key TEXT NOT NULL,          -- Fast lookup key (file path or tool name)
            suggestion_text TEXT NOT NULL,      -- LLM-generated contextual hint
            confidence REAL,
            shown_count INTEGER DEFAULT 0,
            accepted_count INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,                    -- Suggestions expire after 7 days
            UNIQUE(project_id, trigger_key)
        );
        CREATE INDEX IF NOT EXISTS idx_proactive_suggestions_lookup
            ON proactive_suggestions(project_id, trigger_key, confidence DESC);
        CREATE INDEX IF NOT EXISTS idx_proactive_suggestions_expire
            ON proactive_suggestions(expires_at);
    "#,
    )?;

    // Add shown_count and dismissed columns to behavior_patterns (check independently)
    if !column_exists(conn, "behavior_patterns", "shown_count") {
        tracing::info!("Adding shown_count column to behavior_patterns");
        conn.execute_batch(
            "ALTER TABLE behavior_patterns ADD COLUMN shown_count INTEGER DEFAULT 0;",
        )?;
    }
    if !column_exists(conn, "behavior_patterns", "dismissed") {
        tracing::info!("Adding dismissed column to behavior_patterns");
        conn.execute_batch(
            "ALTER TABLE behavior_patterns ADD COLUMN dismissed INTEGER DEFAULT 0;",
        )?;
    }

    // Migrate existing pondering patterns to use insight_ prefix
    // This separates pondering insights from prediction patterns
    migrate_pondering_pattern_types(conn)?;

    Ok(())
}

/// Migrate pondering patterns to use insight_ prefix for pattern_type
/// This separates pondering-generated insights from mining-generated patterns
fn migrate_pondering_pattern_types(conn: &Connection) -> Result<()> {
    // Check if migration is needed by looking for non-insight pondering patterns
    let needs_migration: bool = conn
        .query_row(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM behavior_patterns
                WHERE json_extract(pattern_data, '$.generated_by') = 'pondering'
                  AND pattern_type NOT LIKE 'insight_%'
                LIMIT 1
            )
            "#,
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !needs_migration {
        return Ok(());
    }

    tracing::info!("Migrating pondering patterns to use insight_ prefix");

    // Prefix pattern_type with 'insight_' for all pondering-generated patterns
    let updated = conn.execute(
        r#"
        UPDATE behavior_patterns
        SET pattern_type = 'insight_' || pattern_type,
            updated_at = datetime('now')
        WHERE json_extract(pattern_data, '$.generated_by') = 'pondering'
          AND pattern_type NOT LIKE 'insight_%'
        "#,
        [],
    )?;

    tracing::info!("Migrated {} pondering patterns to insight_ prefix", updated);

    Ok(())
}


/// Migrate to add cross-project intelligence tables for pattern sharing
pub fn migrate_cross_project_intelligence_tables(conn: &Connection) -> Result<()> {
    // Cross-project patterns - anonymized patterns that can be shared
    create_table_if_missing(
        conn,
        "cross_project_patterns",
        r#"
        CREATE TABLE IF NOT EXISTS cross_project_patterns (
            id INTEGER PRIMARY KEY,
            pattern_type TEXT NOT NULL,           -- 'file_sequence', 'tool_chain', 'problem_pattern', 'collaboration'
            pattern_hash TEXT UNIQUE NOT NULL,    -- Hash for deduplication and lookup
            anonymized_data TEXT NOT NULL,        -- JSON: pattern data with all identifiers removed
            category TEXT,                        -- High-level category (e.g., 'rust', 'web', 'database')
            confidence REAL DEFAULT 0.5,          -- Aggregated confidence across projects
            occurrence_count INTEGER DEFAULT 1,   -- How many projects show this pattern
            noise_added REAL DEFAULT 0.0,         -- Differential privacy noise level applied
            min_projects_required INTEGER DEFAULT 3, -- K-anonymity threshold
            source_project_count INTEGER DEFAULT 1,  -- Number of projects contributing
            last_updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_cross_patterns_type ON cross_project_patterns(pattern_type, confidence DESC);
        CREATE INDEX IF NOT EXISTS idx_cross_patterns_category ON cross_project_patterns(category);
        CREATE INDEX IF NOT EXISTS idx_cross_patterns_hash ON cross_project_patterns(pattern_hash);
    "#,
    )?;

    // Pattern sharing log - tracks what patterns were shared and when
    create_table_if_missing(
        conn,
        "pattern_sharing_log",
        r#"
        CREATE TABLE IF NOT EXISTS pattern_sharing_log (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            direction TEXT NOT NULL,              -- 'exported' or 'imported'
            pattern_type TEXT NOT NULL,
            pattern_hash TEXT NOT NULL,
            anonymization_level TEXT,             -- 'full', 'partial', 'none'
            differential_privacy_epsilon REAL,    -- Privacy budget used
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_sharing_log_project ON pattern_sharing_log(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_sharing_log_direction ON pattern_sharing_log(direction, pattern_type);
    "#,
    )?;

    // Cross-project sharing preferences per project
    create_table_if_missing(
        conn,
        "cross_project_preferences",
        r#"
        CREATE TABLE IF NOT EXISTS cross_project_preferences (
            id INTEGER PRIMARY KEY,
            project_id INTEGER UNIQUE REFERENCES projects(id),
            sharing_enabled INTEGER DEFAULT 0,    -- Master opt-in switch
            export_patterns INTEGER DEFAULT 0,    -- Allow exporting patterns from this project
            import_patterns INTEGER DEFAULT 1,    -- Allow importing patterns to this project
            min_anonymization_level TEXT DEFAULT 'full',  -- 'full', 'partial', 'none'
            allowed_pattern_types TEXT,           -- JSON array of allowed types, NULL = all
            privacy_epsilon_budget REAL DEFAULT 1.0,  -- Total differential privacy budget
            privacy_epsilon_used REAL DEFAULT 0.0,    -- Privacy budget consumed
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_cross_prefs_enabled ON cross_project_preferences(sharing_enabled);
    "#,
    )?;

    // Pattern provenance - tracks which projects contributed (without identifying them)
    create_table_if_missing(
        conn,
        "pattern_provenance",
        r#"
        CREATE TABLE IF NOT EXISTS pattern_provenance (
            id INTEGER PRIMARY KEY,
            pattern_id INTEGER REFERENCES cross_project_patterns(id),
            contribution_hash TEXT NOT NULL,      -- Hash of project contribution (not project id)
            contribution_weight REAL DEFAULT 1.0, -- How much this contribution affects pattern
            contributed_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(pattern_id, contribution_hash)
        );
        CREATE INDEX IF NOT EXISTS idx_provenance_pattern ON pattern_provenance(pattern_id);
    "#,
    )?;

    Ok(())
}
