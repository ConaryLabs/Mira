-- Audit log for security and observability events
-- Tracks: auth attempts, tool calls, session lifecycle, errors

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,              -- Unix epoch seconds
    event_type TEXT NOT NULL,                -- auth_success, auth_failure, tool_call, session_start, etc.
    source TEXT NOT NULL,                    -- mcp, studio, sync
    project_path TEXT,                       -- Optional project context
    request_id TEXT,                         -- For correlating with requests
    user_agent TEXT,                         -- Client identifier if available
    remote_addr TEXT,                        -- IP address (may be localhost)
    details TEXT,                            -- JSON blob with event-specific data
    severity TEXT DEFAULT 'info'             -- debug, info, warn, error
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_source ON audit_log(source);
CREATE INDEX IF NOT EXISTS idx_audit_severity ON audit_log(severity, timestamp);
