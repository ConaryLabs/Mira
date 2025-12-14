-- Code style patterns table for caching computed style metrics
-- Stores detected patterns like average function length, abstraction level, etc.

CREATE TABLE IF NOT EXISTS code_style_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT NOT NULL,
    pattern_type TEXT NOT NULL,  -- 'avg_function_length', 'abstraction_level', 'test_ratio'
    pattern_value TEXT NOT NULL,  -- '18', 'low', '0.15'
    sample_count INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.0,
    computed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_style_patterns_project ON code_style_patterns(project_path);
CREATE INDEX IF NOT EXISTS idx_style_patterns_type ON code_style_patterns(pattern_type);
