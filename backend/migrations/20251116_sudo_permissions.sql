-- Migration: Sudo Permissions System
-- Enables GPT-5 to execute system administration commands with proper authorization
-- Created: 2025-11-15

-- ============================================================================
-- Sudo Permission Rules (Whitelist)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Permission identification
    name TEXT NOT NULL UNIQUE,              -- e.g., "nginx_restart", "view_logs"
    description TEXT,                        -- Human-readable description

    -- Command matching (choose one approach per rule)
    command_exact TEXT,                      -- Exact command match: "systemctl restart nginx"
    command_pattern TEXT,                    -- Regex pattern: "^systemctl status .*$"
    command_prefix TEXT,                     -- Prefix match: "journalctl -u "

    -- Permission behavior
    requires_approval BOOLEAN DEFAULT 0,     -- 0 = auto-allow, 1 = ask user first
    enabled BOOLEAN DEFAULT 1,               -- Can be disabled without deleting

    -- Metadata
    created_at INTEGER NOT NULL,
    created_by TEXT,                         -- Who created this permission
    last_used_at INTEGER,                    -- Last time this permission was used
    use_count INTEGER DEFAULT 0,             -- How many times used

    -- Notes
    notes TEXT                               -- Admin notes about this permission
);

-- Index for fast command lookup
CREATE INDEX IF NOT EXISTS idx_sudo_permissions_enabled
    ON sudo_permissions(enabled);

-- ============================================================================
-- Sudo Approval Requests (Pending User Approval)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_approval_requests (
    id TEXT PRIMARY KEY,                     -- UUID

    -- Command details
    command TEXT NOT NULL,                   -- Full command to execute
    working_dir TEXT,                        -- Working directory for command

    -- Context
    operation_id TEXT,                       -- Associated operation ID
    session_id TEXT,                         -- User session
    requested_by TEXT DEFAULT 'gpt5',        -- Which system requested it
    reason TEXT,                             -- Why this command is needed

    -- Status
    status TEXT DEFAULT 'pending',           -- pending, approved, denied, expired

    -- Timing
    created_at INTEGER NOT NULL,
    expires_at INTEGER,                      -- Auto-deny if not approved by this time
    responded_at INTEGER,                    -- When user approved/denied

    -- Response
    approved_by TEXT,                        -- User who approved/denied
    denial_reason TEXT,                      -- Why user denied (optional)

    -- Result (if approved and executed)
    executed_at INTEGER,
    exit_code INTEGER,
    output TEXT,
    error TEXT,

    FOREIGN KEY (operation_id) REFERENCES operations(id)
);

-- Indexes for approval queue queries
CREATE INDEX IF NOT EXISTS idx_sudo_approval_status
    ON sudo_approval_requests(status, created_at);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_session
    ON sudo_approval_requests(session_id, status);

-- ============================================================================
-- Sudo Audit Log (Complete History)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sudo_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Command executed
    command TEXT NOT NULL,
    working_dir TEXT,

    -- Authorization
    permission_id INTEGER,                   -- Which permission allowed this (if whitelist)
    approval_request_id TEXT,                -- Which approval allowed this (if approval flow)
    authorization_type TEXT NOT NULL,        -- 'whitelist' or 'approval'

    -- Context
    operation_id TEXT,
    session_id TEXT,
    executed_by TEXT DEFAULT 'gpt5',         -- Which system executed

    -- Execution details
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    exit_code INTEGER,

    -- Output (may be large - consider separate table if needed)
    stdout TEXT,
    stderr TEXT,

    -- Success/failure
    success BOOLEAN,
    error_message TEXT,

    -- Metadata
    environment_vars TEXT,                   -- JSON of env vars (if any were set)
    timeout_ms INTEGER,                      -- Timeout that was set

    FOREIGN KEY (permission_id) REFERENCES sudo_permissions(id),
    FOREIGN KEY (approval_request_id) REFERENCES sudo_approval_requests(id),
    FOREIGN KEY (operation_id) REFERENCES operations(id)
);

-- Indexes for audit queries
CREATE INDEX IF NOT EXISTS idx_sudo_audit_time
    ON sudo_audit_log(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_session
    ON sudo_audit_log(session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_operation
    ON sudo_audit_log(operation_id);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_success
    ON sudo_audit_log(success, started_at DESC);

-- ============================================================================
-- Default Permissions (Safe Read-Only Commands)
-- ============================================================================

-- System status checks (no approval needed)
INSERT INTO sudo_permissions (name, description, command_exact, requires_approval, enabled, created_at, created_by, notes) VALUES
    ('nginx_status', 'Check nginx service status', 'systemctl status nginx', 0, 1, strftime('%s', 'now'), 'system', 'Safe read-only command'),
    ('nginx_test_config', 'Test nginx configuration syntax', 'nginx -t', 0, 1, strftime('%s', 'now'), 'system', 'Safe validation command'),
    ('mira_status', 'Check mira-backend service status', 'systemctl status mira-backend', 0, 1, strftime('%s', 'now'), 'system', 'Safe read-only command');

-- Log viewing (pattern-based, no approval needed)
INSERT INTO sudo_permissions (name, description, command_pattern, requires_approval, enabled, created_at, created_by, notes) VALUES
    ('view_nginx_logs', 'View recent nginx logs', '^journalctl -u nginx (-n \d+|--since.*)?$', 0, 1, strftime('%s', 'now'), 'system', 'Safe read-only with line limits'),
    ('view_mira_logs', 'View recent mira-backend logs', '^journalctl -u mira-backend (-n \d+|--since.*)?$', 0, 1, strftime('%s', 'now'), 'system', 'Safe read-only with line limits');

-- Service restarts (REQUIRES APPROVAL by default)
INSERT INTO sudo_permissions (name, description, command_exact, requires_approval, enabled, created_at, created_by, notes) VALUES
    ('nginx_restart', 'Restart nginx service', 'systemctl restart nginx', 1, 1, strftime('%s', 'now'), 'system', 'Requires approval - service disruption'),
    ('nginx_reload', 'Reload nginx configuration', 'systemctl reload nginx', 1, 1, strftime('%s', 'now'), 'system', 'Requires approval - safer than restart'),
    ('mira_restart', 'Restart mira-backend service', 'systemctl restart mira-backend', 1, 1, strftime('%s', 'now'), 'system', 'Requires approval - will disconnect session');

-- Note: System file editing (nginx configs, etc.) should use write_project_file with specific paths
-- and potentially require approval as well. Consider adding path-based permissions.
