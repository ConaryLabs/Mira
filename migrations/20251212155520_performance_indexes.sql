-- Performance indexes for frequently queried columns

-- code_symbols: qualified_name is frequently searched with LIKE
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON code_symbols(qualified_name);

-- code_symbols: compound index for language + symbol_type (common filter combo)
CREATE INDEX IF NOT EXISTS idx_symbols_lang_type ON code_symbols(language, symbol_type);

-- memory_facts: compound index for project + fact_type (common query pattern)
CREATE INDEX IF NOT EXISTS idx_facts_project_type ON memory_facts(project_id, fact_type);

-- git_commits: message for text search
CREATE INDEX IF NOT EXISTS idx_commits_message ON git_commits(message);

-- Analyze tables for query optimizer
ANALYZE code_symbols;
ANALYZE memory_facts;
ANALYZE call_graph;
ANALYZE git_commits;
