-- backend/migrations/20251125_002_code_intelligence.sql
-- Code Intelligence: AST, Symbols, Semantic Graph, Pattern Detection

-- ============================================================================
-- AST & SYMBOLS
-- ============================================================================

CREATE TABLE IF NOT EXISTS code_elements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    element_type TEXT NOT NULL,
    visibility TEXT,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    content TEXT,
    signature TEXT,
    content_hash TEXT,
    signature_hash TEXT,
    parent_id INTEGER,
    complexity_score REAL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_code_elements_project ON code_elements(project_id);
CREATE INDEX IF NOT EXISTS idx_code_elements_file ON code_elements(file_path);
CREATE INDEX IF NOT EXISTS idx_code_elements_name ON code_elements(name);
CREATE INDEX IF NOT EXISTS idx_code_elements_type ON code_elements(element_type);
CREATE INDEX IF NOT EXISTS idx_code_elements_content_hash ON code_elements(content_hash);
CREATE INDEX IF NOT EXISTS idx_code_elements_signature_hash ON code_elements(signature_hash);
CREATE INDEX IF NOT EXISTS idx_code_elements_parent ON code_elements(parent_id);

CREATE TABLE IF NOT EXISTS call_graph (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    caller_id INTEGER NOT NULL,
    callee_id INTEGER NOT NULL,
    call_line INTEGER NOT NULL,
    FOREIGN KEY (caller_id) REFERENCES code_elements(id) ON DELETE CASCADE,
    FOREIGN KEY (callee_id) REFERENCES code_elements(id) ON DELETE CASCADE,
    UNIQUE(caller_id, callee_id, call_line)
);

CREATE INDEX IF NOT EXISTS idx_call_graph_caller ON call_graph(caller_id);
CREATE INDEX IF NOT EXISTS idx_call_graph_callee ON call_graph(callee_id);

CREATE TABLE IF NOT EXISTS external_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    element_id INTEGER,
    dependency_name TEXT NOT NULL,
    dependency_type TEXT,
    is_glob_import BOOLEAN DEFAULT FALSE,
    imported_items TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (element_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_external_dependencies_project ON external_dependencies(project_id);
CREATE INDEX IF NOT EXISTS idx_external_dependencies_file ON external_dependencies(file_path);
CREATE INDEX IF NOT EXISTS idx_external_dependencies_element ON external_dependencies(element_id);
CREATE INDEX IF NOT EXISTS idx_external_dependencies_name ON external_dependencies(dependency_name);

-- ============================================================================
-- SEMANTIC GRAPH
-- ============================================================================

CREATE TABLE IF NOT EXISTS semantic_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id INTEGER NOT NULL UNIQUE,
    purpose TEXT NOT NULL,
    description TEXT,
    concepts TEXT NOT NULL,
    domain_labels TEXT NOT NULL,
    confidence_score REAL NOT NULL DEFAULT 0.0,
    embedding_point_id TEXT,
    last_analyzed INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (symbol_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_semantic_nodes_symbol ON semantic_nodes(symbol_id);
CREATE INDEX IF NOT EXISTS idx_semantic_nodes_confidence ON semantic_nodes(confidence_score);
CREATE INDEX IF NOT EXISTS idx_semantic_nodes_point ON semantic_nodes(embedding_point_id);

CREATE TABLE IF NOT EXISTS semantic_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_node_id INTEGER NOT NULL,
    target_node_id INTEGER NOT NULL,
    relationship_type TEXT NOT NULL,
    strength REAL NOT NULL DEFAULT 0.0,
    metadata TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (source_node_id) REFERENCES semantic_nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (target_node_id) REFERENCES semantic_nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_semantic_edges_source ON semantic_edges(source_node_id);
CREATE INDEX IF NOT EXISTS idx_semantic_edges_target ON semantic_edges(target_node_id);
CREATE INDEX IF NOT EXISTS idx_semantic_edges_type ON semantic_edges(relationship_type);
CREATE INDEX IF NOT EXISTS idx_semantic_edges_strength ON semantic_edges(strength);

CREATE TABLE IF NOT EXISTS concept_index (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    concept TEXT NOT NULL,
    symbol_ids TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_concept_index_concept ON concept_index(concept);
CREATE INDEX IF NOT EXISTS idx_concept_index_confidence ON concept_index(confidence);

CREATE TABLE IF NOT EXISTS semantic_analysis_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id INTEGER NOT NULL UNIQUE,
    code_hash TEXT NOT NULL,
    analysis_result TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    last_used INTEGER NOT NULL,
    hit_count INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (symbol_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_semantic_cache_symbol ON semantic_analysis_cache(symbol_id);
CREATE INDEX IF NOT EXISTS idx_semantic_cache_hash ON semantic_analysis_cache(code_hash);
CREATE INDEX IF NOT EXISTS idx_semantic_cache_last_used ON semantic_analysis_cache(last_used);

-- ============================================================================
-- DESIGN PATTERNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS design_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    pattern_name TEXT NOT NULL,
    pattern_type TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    involved_symbols TEXT NOT NULL,
    description TEXT,
    embedding_point_id TEXT,
    detected_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_design_patterns_project ON design_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_design_patterns_name ON design_patterns(pattern_name);
CREATE INDEX IF NOT EXISTS idx_design_patterns_type ON design_patterns(pattern_type);
CREATE INDEX IF NOT EXISTS idx_design_patterns_confidence ON design_patterns(confidence);

CREATE TABLE IF NOT EXISTS pattern_validation_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_type TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    validation_result TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_at INTEGER NOT NULL,
    last_used INTEGER NOT NULL,
    hit_count INTEGER DEFAULT 0,
    UNIQUE(pattern_type, code_hash)
);

CREATE INDEX IF NOT EXISTS idx_pattern_validation_type ON pattern_validation_cache(pattern_type);
CREATE INDEX IF NOT EXISTS idx_pattern_validation_hash ON pattern_validation_cache(code_hash);
CREATE INDEX IF NOT EXISTS idx_pattern_validation_last_used ON pattern_validation_cache(last_used);

-- ============================================================================
-- DOMAIN CLUSTERING
-- ============================================================================

CREATE TABLE IF NOT EXISTS domain_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    domain_name TEXT NOT NULL,
    symbol_ids TEXT NOT NULL,
    file_paths TEXT NOT NULL,
    cohesion_score REAL NOT NULL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, domain_name),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_domain_clusters_project ON domain_clusters(project_id);
CREATE INDEX IF NOT EXISTS idx_domain_clusters_domain ON domain_clusters(domain_name);
CREATE INDEX IF NOT EXISTS idx_domain_clusters_cohesion ON domain_clusters(cohesion_score);

-- ============================================================================
-- CODE QUALITY
-- ============================================================================

CREATE TABLE IF NOT EXISTS code_quality_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    element_id INTEGER,
    issue_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    suggested_fix TEXT,
    auto_fixable BOOLEAN DEFAULT FALSE,
    resolved BOOLEAN DEFAULT FALSE,
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (element_id) REFERENCES code_elements(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_code_quality_project ON code_quality_issues(project_id);
CREATE INDEX IF NOT EXISTS idx_code_quality_file ON code_quality_issues(file_path);
CREATE INDEX IF NOT EXISTS idx_code_quality_element ON code_quality_issues(element_id);
CREATE INDEX IF NOT EXISTS idx_code_quality_severity ON code_quality_issues(severity);
CREATE INDEX IF NOT EXISTS idx_code_quality_resolved ON code_quality_issues(resolved);

CREATE TABLE IF NOT EXISTS language_configs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    language TEXT NOT NULL UNIQUE,
    parser_config TEXT NOT NULL,
    file_extensions TEXT NOT NULL,
    enabled BOOLEAN DEFAULT TRUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_language_configs_language ON language_configs(language);
CREATE INDEX IF NOT EXISTS idx_language_configs_enabled ON language_configs(enabled);
