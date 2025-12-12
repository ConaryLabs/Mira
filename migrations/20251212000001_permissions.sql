-- Persistent Permission Rules for Claude Code
-- Allows auto-approval of previously-approved tool operations across sessions

CREATE TABLE IF NOT EXISTS permission_rules (
    id TEXT PRIMARY KEY,
    -- Scope: 'global' or 'project'
    scope TEXT NOT NULL DEFAULT 'project',
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,

    -- Tool matching
    tool_name TEXT NOT NULL,                -- e.g., 'Bash', 'Edit', 'Write', 'Read'

    -- Input matching (for fine-grained control)
    input_field TEXT,                       -- Which field to match (e.g., 'command', 'file_path')
    input_pattern TEXT,                     -- Pattern to match (glob, prefix, or exact)
    match_type TEXT DEFAULT 'prefix',       -- 'exact', 'prefix', 'glob'

    -- Metadata
    description TEXT,                       -- Human-readable description
    times_used INTEGER DEFAULT 0,           -- How often this rule matched
    last_used_at INTEGER,                   -- Last time this rule was applied

    -- Audit
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,

    -- Unique constraint for deduplication
    UNIQUE(scope, project_id, tool_name, input_field, input_pattern)
);

CREATE INDEX IF NOT EXISTS idx_perm_rules_scope ON permission_rules(scope);
CREATE INDEX IF NOT EXISTS idx_perm_rules_project ON permission_rules(project_id);
CREATE INDEX IF NOT EXISTS idx_perm_rules_tool ON permission_rules(tool_name);
