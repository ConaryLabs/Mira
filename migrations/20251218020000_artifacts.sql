-- Artifact storage for tool outputs
-- Enables: token reduction, targeted retrieval, search, TTL cleanup

CREATE TABLE IF NOT EXISTS artifacts (
  id                TEXT PRIMARY KEY,              -- UUID
  created_at        INTEGER NOT NULL,              -- unix seconds
  expires_at        INTEGER,                       -- unix seconds, nullable (no TTL = keep forever)
  project_path      TEXT NOT NULL,                 -- partition/gc per project

  kind              TEXT NOT NULL,                 -- "tool_output", "tool_stderr", "diff", "file_snapshot"
  tool_name         TEXT,                          -- e.g. "git_diff", "grep", "read_file"
  tool_call_id      TEXT,                          -- model tool call id, for linking to blocks
  message_id        TEXT,                          -- assistant message id (nullable until known)

  content_type      TEXT NOT NULL DEFAULT 'text/plain; charset=utf-8',
  encoding          TEXT NOT NULL DEFAULT 'utf-8',
  compression       TEXT NOT NULL DEFAULT 'none',  -- "zstd" | "gzip" | "none"

  uncompressed_bytes INTEGER NOT NULL,
  compressed_bytes   INTEGER NOT NULL,

  sha256            TEXT NOT NULL,                 -- hash of uncompressed (dedupe + integrity)
  contains_secrets  INTEGER NOT NULL DEFAULT 0,    -- 0/1
  secret_reason     TEXT,                          -- "private_key", "api_token", etc.

  preview_text      TEXT,                          -- short excerpt safe to inline
  data              BLOB NOT NULL,                 -- actual payload (possibly compressed)
  searchable_text   TEXT                           -- normalized excerpt for LIKE search
);

CREATE INDEX IF NOT EXISTS idx_artifacts_project_created
  ON artifacts(project_path, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_artifacts_expires
  ON artifacts(expires_at) WHERE expires_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_artifacts_message
  ON artifacts(message_id) WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_artifacts_tool_call
  ON artifacts(tool_call_id) WHERE tool_call_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_artifacts_sha
  ON artifacts(sha256);

-- Optional: tool_calls table for analytics (can query which tools create bloat)
CREATE TABLE IF NOT EXISTS chat_tool_calls (
  id                TEXT PRIMARY KEY,             -- UUID (internal)
  message_id        TEXT NOT NULL,                -- assistant message
  call_id           TEXT NOT NULL,                -- model tool call id
  tool_name         TEXT NOT NULL,
  arguments_json    TEXT NOT NULL,
  success           INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  artifact_id       TEXT,                         -- nullable (inline = no artifact)
  inline_bytes      INTEGER NOT NULL DEFAULT 0,

  FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON chat_tool_calls(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_tool_name ON chat_tool_calls(tool_name);
CREATE INDEX IF NOT EXISTS idx_tool_calls_artifact ON chat_tool_calls(artifact_id) WHERE artifact_id IS NOT NULL;
