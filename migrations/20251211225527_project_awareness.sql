-- Project Awareness: Make Mira aware of which project Claude is working in
-- This enables automatic scoping of memories, sessions, and context per-project

-- ============================================================================
-- PROJECTS: Registry of known projects
-- ============================================================================

CREATE TABLE projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,              -- Canonical absolute path
    name TEXT NOT NULL,                     -- Human-readable name (from directory)
    project_type TEXT,                      -- 'rust', 'node', 'python', 'go', etc.
    first_seen INTEGER NOT NULL,
    last_accessed INTEGER NOT NULL
);

CREATE INDEX idx_projects_path ON projects(path);
CREATE INDEX idx_projects_name ON projects(name);

-- ============================================================================
-- ADD project_id TO EXISTING TABLES
-- All columns are nullable for backward compatibility (existing data stays global)
-- ============================================================================

-- Memory facts (decisions, context are project-scoped; preferences stay global)
ALTER TABLE memory_facts ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_facts_project ON memory_facts(project_id);

-- Memory entries / sessions
ALTER TABLE memory_entries ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_entries_project ON memory_entries(project_id);

-- File activity tracking
ALTER TABLE file_activity ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_file_activity_project ON file_activity(project_id);

-- Work context (active task, recent error, etc.)
ALTER TABLE work_context ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_work_context_project ON work_context(project_id);

-- Error fixes (can be language-global or project-specific)
ALTER TABLE error_fixes ADD COLUMN project_id INTEGER REFERENCES projects(id);
CREATE INDEX idx_error_fixes_project ON error_fixes(project_id);
