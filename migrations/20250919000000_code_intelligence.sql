-- migrations/20250919000000_code_intelligence.sql
-- Code intelligence foundation - builds on existing repository_files table

-- Language configuration and parsing rules
CREATE TABLE language_configs (
    language TEXT PRIMARY KEY,
    file_extensions TEXT NOT NULL, -- JSON: [".rs", ".ts", ".tsx", ".js", ".jsx"]
    parser_type TEXT NOT NULL,     -- 'rust_syn', 'typescript_swc', 'javascript_babel'
    complexity_rules TEXT,         -- JSON: language-specific complexity rules
    dependency_patterns TEXT,      -- JSON: import/use statement patterns
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Insert initial language configs
INSERT INTO language_configs (language, file_extensions, parser_type, complexity_rules, dependency_patterns) VALUES
('rust', '["rs"]', 'rust_syn', 
 '{"max_cyclomatic": 10, "max_nesting": 4, "max_function_length": 50}',
 '["use\\s+([^;]+);", "mod\\s+([a-zA-Z_][a-zA-Z0-9_]*);"]'),

('typescript', '["ts", "tsx"]', 'typescript_swc',
 '{"max_cyclomatic": 15, "max_nesting": 5, "max_component_props": 8}',
 '["import\\s+[^from]*from\\s+[\"'']([^\"'']+)[\"'']", "import\\s+[\"'']([^\"'']+)[\"'']"]'),

('javascript', '["js", "jsx"]', 'javascript_babel',
 '{"max_cyclomatic": 15, "max_nesting": 5, "max_component_props": 8}',
 '["import\\s+[^from]*from\\s+[\"'']([^\"'']+)[\"'']", "require\\([\\"'']([^\\"'']+)[\\"'']\\)"]');

-- Add fields to existing repository_files table  
ALTER TABLE repository_files ADD COLUMN ast_analyzed BOOLEAN DEFAULT FALSE;
ALTER TABLE repository_files ADD COLUMN ast_hash TEXT;
ALTER TABLE repository_files ADD COLUMN element_count INTEGER DEFAULT 0;
ALTER TABLE repository_files ADD COLUMN complexity_score INTEGER DEFAULT 0;
ALTER TABLE repository_files ADD COLUMN last_analyzed DATETIME;

-- Create indexes for new fields
CREATE INDEX idx_repo_files_analyzed ON repository_files(ast_analyzed);
CREATE INDEX idx_repo_files_language ON repository_files(language);

-- Code elements (functions, structs, components, etc.)
CREATE TABLE code_elements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL REFERENCES repository_files(id),
    language TEXT NOT NULL REFERENCES language_configs(language),
    element_type TEXT NOT NULL, -- 'function', 'struct', 'enum', 'component', 'interface'
    name TEXT NOT NULL,
    full_path TEXT NOT NULL,    -- 'module::path::element_name'
    visibility TEXT NOT NULL,   -- 'public', 'private', 'protected'
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content TEXT NOT NULL,      -- Full source code of element
    signature_hash TEXT,        -- For change detection
    
    -- Analysis fields
    complexity_score INTEGER DEFAULT 0,
    is_test BOOLEAN DEFAULT FALSE,
    is_async BOOLEAN DEFAULT FALSE,
    documentation TEXT,
    
    -- Language-specific metadata (JSON)
    metadata TEXT,
    
    -- Timestamps
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    analyzed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    
    UNIQUE(file_id, name, start_line)
);

-- Indexes for code_elements
CREATE INDEX idx_code_elements_file ON code_elements(file_id);
CREATE INDEX idx_code_elements_language ON code_elements(language);
CREATE INDEX idx_code_elements_type ON code_elements(element_type);
CREATE INDEX idx_code_elements_name ON code_elements(name);
CREATE INDEX idx_code_elements_complexity ON code_elements(complexity_score);

-- External dependencies (imports, use statements)
CREATE TABLE external_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER NOT NULL REFERENCES code_elements(id),
    import_path TEXT NOT NULL,            -- 'std::collections::HashMap', 'react'
    imported_symbols TEXT,               -- JSON array: ["HashMap", "BTreeMap"]
    dependency_type TEXT NOT NULL,       -- 'crate', 'npm_package', 'local_import'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_external_deps_element ON external_dependencies(element_id);
CREATE INDEX idx_external_deps_path ON external_dependencies(import_path);

-- Quality issues detected during analysis
CREATE TABLE code_quality_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    element_id INTEGER NOT NULL REFERENCES code_elements(id),
    issue_type TEXT NOT NULL,    -- 'complexity', 'duplication', 'security', 'performance'
    severity TEXT NOT NULL,      -- 'info', 'low', 'medium', 'high', 'critical'
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    suggested_fix TEXT,
    fix_confidence REAL DEFAULT 0.0,
    is_auto_fixable BOOLEAN DEFAULT FALSE,
    detected_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_quality_issues_element ON code_quality_issues(element_id);
CREATE INDEX idx_quality_issues_severity ON code_quality_issues(severity);
CREATE INDEX idx_quality_issues_type ON code_quality_issues(issue_type);

-- Enable foreign key constraints
PRAGMA foreign_keys = ON;
