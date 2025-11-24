-- backend/migrations/20251125_006_tool_synthesis.sql
-- Tool Synthesis: Pattern Detection, Tool Generation, Execution Tracking

-- ============================================================================
-- TOOL PATTERNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    pattern_name TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    description TEXT NOT NULL,
    detected_occurrences INTEGER NOT NULL DEFAULT 1,
    example_locations TEXT NOT NULL,
    confidence_score REAL NOT NULL DEFAULT 0.0,
    should_synthesize BOOLEAN DEFAULT FALSE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_patterns_project ON tool_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_name ON tool_patterns(pattern_name);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_type ON tool_patterns(pattern_type);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_confidence ON tool_patterns(confidence_score);
CREATE INDEX IF NOT EXISTS idx_tool_patterns_should_synthesize ON tool_patterns(should_synthesize);

-- ============================================================================
-- SYNTHESIZED TOOLS
-- ============================================================================

CREATE TABLE IF NOT EXISTS synthesized_tools (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    tool_pattern_id INTEGER,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    source_code TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'rust',
    compilation_status TEXT NOT NULL DEFAULT 'pending',
    compilation_error TEXT,
    binary_path TEXT,
    enabled BOOLEAN DEFAULT TRUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (tool_pattern_id) REFERENCES tool_patterns(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_synthesized_tools_project ON synthesized_tools(project_id);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_pattern ON synthesized_tools(tool_pattern_id);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_name ON synthesized_tools(name);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_status ON synthesized_tools(compilation_status);
CREATE INDEX IF NOT EXISTS idx_synthesized_tools_enabled ON synthesized_tools(enabled);

-- ============================================================================
-- TOOL EXECUTIONS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    operation_id TEXT,
    session_id TEXT NOT NULL,
    user_id TEXT,
    arguments TEXT,
    success BOOLEAN NOT NULL,
    output TEXT,
    error_message TEXT,
    duration_ms INTEGER NOT NULL,
    executed_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_executions_tool ON tool_executions(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_operation ON tool_executions(operation_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_session ON tool_executions(session_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_user ON tool_executions(user_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_success ON tool_executions(success);
CREATE INDEX IF NOT EXISTS idx_tool_executions_executed ON tool_executions(executed_at);

-- ============================================================================
-- TOOL EFFECTIVENESS
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_effectiveness (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL UNIQUE,
    total_executions INTEGER NOT NULL DEFAULT 0,
    successful_executions INTEGER NOT NULL DEFAULT 0,
    failed_executions INTEGER NOT NULL DEFAULT 0,
    average_duration_ms REAL,
    total_time_saved_ms INTEGER DEFAULT 0,
    last_executed INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_effectiveness_tool ON tool_effectiveness(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_effectiveness_success_rate ON tool_effectiveness(successful_executions);

-- ============================================================================
-- TOOL FEEDBACK
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    execution_id INTEGER,
    user_id TEXT NOT NULL,
    rating INTEGER,
    comment TEXT,
    issue_type TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_id) REFERENCES tool_executions(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_feedback_tool ON tool_feedback(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_execution ON tool_feedback(execution_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_user ON tool_feedback(user_id);
CREATE INDEX IF NOT EXISTS idx_tool_feedback_rating ON tool_feedback(rating);

-- ============================================================================
-- TOOL EVOLUTION
-- ============================================================================

CREATE TABLE IF NOT EXISTS tool_evolution_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id TEXT NOT NULL,
    old_version INTEGER NOT NULL,
    new_version INTEGER NOT NULL,
    change_description TEXT NOT NULL,
    motivation TEXT,
    source_code_diff TEXT,
    evolved_at INTEGER NOT NULL,
    FOREIGN KEY (tool_id) REFERENCES synthesized_tools(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_evolution_tool ON tool_evolution_history(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_old_version ON tool_evolution_history(old_version);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_new_version ON tool_evolution_history(new_version);
CREATE INDEX IF NOT EXISTS idx_tool_evolution_evolved ON tool_evolution_history(evolved_at);
