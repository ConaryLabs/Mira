-- backend/migrations/20251125_003_git_intelligence.sql
-- Git Intelligence: Commits, Co-Change Patterns, Author Expertise, Historical Fixes

-- ============================================================================
-- COMMIT TRACKING
-- ============================================================================

CREATE TABLE IF NOT EXISTS git_commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    author_name TEXT NOT NULL,
    author_email TEXT NOT NULL,
    commit_message TEXT NOT NULL,
    message_summary TEXT NOT NULL,
    authored_at INTEGER NOT NULL,
    committed_at INTEGER NOT NULL,
    parent_hashes TEXT,
    file_changes TEXT NOT NULL,
    insertions INTEGER DEFAULT 0,
    deletions INTEGER DEFAULT 0,
    embedding_point_id TEXT,
    indexed_at INTEGER NOT NULL,
    UNIQUE(project_id, commit_hash),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_git_commits_project ON git_commits(project_id);
CREATE INDEX IF NOT EXISTS idx_git_commits_hash ON git_commits(commit_hash);
CREATE INDEX IF NOT EXISTS idx_git_commits_author_email ON git_commits(author_email);
CREATE INDEX IF NOT EXISTS idx_git_commits_authored_at ON git_commits(authored_at);

-- ============================================================================
-- CO-CHANGE PATTERNS
-- ============================================================================

CREATE TABLE IF NOT EXISTS file_cochange_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path_a TEXT NOT NULL,
    file_path_b TEXT NOT NULL,
    cochange_count INTEGER NOT NULL,
    total_changes_a INTEGER NOT NULL,
    total_changes_b INTEGER NOT NULL,
    confidence_score REAL NOT NULL,
    last_cochange INTEGER NOT NULL,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, file_path_a, file_path_b),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cochange_project ON file_cochange_patterns(project_id);
CREATE INDEX IF NOT EXISTS idx_cochange_file_a ON file_cochange_patterns(file_path_a);
CREATE INDEX IF NOT EXISTS idx_cochange_file_b ON file_cochange_patterns(file_path_b);
CREATE INDEX IF NOT EXISTS idx_cochange_confidence ON file_cochange_patterns(confidence_score);

-- ============================================================================
-- GIT BLAME
-- ============================================================================

CREATE TABLE IF NOT EXISTS blame_annotations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line_number INTEGER NOT NULL,
    commit_hash TEXT NOT NULL,
    author_name TEXT NOT NULL,
    author_email TEXT NOT NULL,
    authored_at INTEGER NOT NULL,
    line_content TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(project_id, file_path, line_number, file_hash),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_blame_project ON blame_annotations(project_id);
CREATE INDEX IF NOT EXISTS idx_blame_file ON blame_annotations(file_path);
CREATE INDEX IF NOT EXISTS idx_blame_commit ON blame_annotations(commit_hash);
CREATE INDEX IF NOT EXISTS idx_blame_author ON blame_annotations(author_email);

-- ============================================================================
-- AUTHOR EXPERTISE
-- ============================================================================

CREATE TABLE IF NOT EXISTS author_expertise (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    author_email TEXT NOT NULL,
    author_name TEXT NOT NULL,
    file_pattern TEXT NOT NULL,
    domain TEXT,
    commit_count INTEGER NOT NULL,
    line_count INTEGER NOT NULL,
    last_contribution INTEGER NOT NULL,
    first_contribution INTEGER NOT NULL,
    expertise_score REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, author_email, file_pattern),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_author_expertise_project ON author_expertise(project_id);
CREATE INDEX IF NOT EXISTS idx_author_expertise_email ON author_expertise(author_email);
CREATE INDEX IF NOT EXISTS idx_author_expertise_pattern ON author_expertise(file_pattern);
CREATE INDEX IF NOT EXISTS idx_author_expertise_domain ON author_expertise(domain);
CREATE INDEX IF NOT EXISTS idx_author_expertise_score ON author_expertise(expertise_score);

-- ============================================================================
-- HISTORICAL FIXES
-- ============================================================================

CREATE TABLE IF NOT EXISTS historical_fixes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    error_pattern TEXT NOT NULL,
    error_category TEXT NOT NULL,
    fix_commit_hash TEXT NOT NULL,
    files_modified TEXT NOT NULL,
    fix_description TEXT,
    fixed_at INTEGER NOT NULL,
    similarity_hash TEXT NOT NULL,
    embedding_point_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_historical_fixes_project ON historical_fixes(project_id);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_error_pattern ON historical_fixes(error_pattern);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_category ON historical_fixes(error_category);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_commit ON historical_fixes(fix_commit_hash);
CREATE INDEX IF NOT EXISTS idx_historical_fixes_similarity ON historical_fixes(similarity_hash);
