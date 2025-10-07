-- migrations/20251008_local_directories.sql
-- Phase 3: Add local directory support to projects

-- Add attachment type to existing git_repo_attachments table
ALTER TABLE git_repo_attachments ADD COLUMN attachment_type TEXT DEFAULT 'git_repository';
-- Values: 'git_repository' | 'local_directory'

-- Add local path override column for local directories
ALTER TABLE git_repo_attachments ADD COLUMN local_path_override TEXT;

-- File modification history for undo functionality
CREATE TABLE IF NOT EXISTS file_modifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    original_content TEXT NOT NULL,
    modified_content TEXT NOT NULL,
    modification_time INTEGER DEFAULT (strftime('%s', 'now')),
    reverted BOOLEAN DEFAULT FALSE,
    UNIQUE(project_id, file_path, modification_time)
);

CREATE INDEX idx_file_mods_project ON file_modifications(project_id, file_path);
CREATE INDEX idx_file_mods_time ON file_modifications(modification_time DESC);
CREATE INDEX idx_file_mods_reverted ON file_modifications(reverted);

-- Add modification counter to projects for tracking
ALTER TABLE projects ADD COLUMN modification_count INTEGER DEFAULT 0;
