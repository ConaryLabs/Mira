-- Sudo Permission System Tables
-- Enables privileged command execution with user authorization

-- SUDO PERMISSIONS (Whitelist Rules)
-- Defines which commands are allowed and under what conditions
CREATE TABLE IF NOT EXISTS sudo_permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    -- Match criteria (exactly one should be set)
    command_exact TEXT,           -- Exact command match
    command_pattern TEXT,         -- Regex pattern match
    command_prefix TEXT,          -- Prefix match
    -- Behavior
    requires_approval INTEGER NOT NULL DEFAULT 1,  -- 1 = needs user confirmation, 0 = auto-approve
    enabled INTEGER NOT NULL DEFAULT 1,
    -- Metadata
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    created_by TEXT,
    last_used_at INTEGER,
    use_count INTEGER NOT NULL DEFAULT 0,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_sudo_permissions_enabled ON sudo_permissions(enabled);
CREATE INDEX IF NOT EXISTS idx_sudo_permissions_name ON sudo_permissions(name);

-- SUDO BLOCKLIST (Never Allow)
-- Commands matching these patterns are NEVER executed, even with approval
CREATE TABLE IF NOT EXISTS sudo_blocklist (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    -- Match criteria (exactly one should be set)
    pattern_exact TEXT,           -- Exact command block
    pattern_regex TEXT,           -- Regex pattern to block
    pattern_prefix TEXT,          -- Prefix to block
    -- Severity and origin
    severity TEXT NOT NULL DEFAULT 'high',  -- 'critical', 'high', 'medium'
    is_default INTEGER NOT NULL DEFAULT 0,  -- 1 = came from default blocklist
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_sudo_blocklist_enabled ON sudo_blocklist(enabled);
CREATE INDEX IF NOT EXISTS idx_sudo_blocklist_severity ON sudo_blocklist(severity);

-- SUDO APPROVAL REQUESTS
-- Tracks pending, approved, denied, and expired approval requests
CREATE TABLE IF NOT EXISTS sudo_approval_requests (
    id TEXT PRIMARY KEY,
    command TEXT NOT NULL,
    working_dir TEXT,
    operation_id TEXT,
    session_id TEXT,
    permission_id INTEGER,
    requested_by TEXT NOT NULL DEFAULT 'llm',
    reason TEXT,
    status TEXT NOT NULL DEFAULT 'pending',  -- 'pending', 'approved', 'denied', 'expired'
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    expires_at INTEGER,
    responded_at INTEGER,
    approved_by TEXT,
    denial_reason TEXT,
    -- Execution results (filled after command runs)
    executed_at INTEGER,
    exit_code INTEGER,
    output TEXT,
    error TEXT,
    FOREIGN KEY (permission_id) REFERENCES sudo_permissions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_sudo_approval_session ON sudo_approval_requests(session_id);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_status ON sudo_approval_requests(status);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_operation ON sudo_approval_requests(operation_id);
CREATE INDEX IF NOT EXISTS idx_sudo_approval_expires ON sudo_approval_requests(expires_at);

-- SUDO AUDIT LOG
-- Complete audit trail of all sudo command executions
CREATE TABLE IF NOT EXISTS sudo_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command TEXT NOT NULL,
    working_dir TEXT,
    permission_id INTEGER,
    approval_request_id TEXT,
    authorization_type TEXT NOT NULL,  -- 'whitelist', 'approval', 'denied', 'blocked'
    operation_id TEXT,
    session_id TEXT,
    executed_by TEXT NOT NULL,
    started_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    completed_at INTEGER,
    exit_code INTEGER,
    stdout TEXT,
    stderr TEXT,
    success INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    FOREIGN KEY (permission_id) REFERENCES sudo_permissions(id) ON DELETE SET NULL,
    FOREIGN KEY (approval_request_id) REFERENCES sudo_approval_requests(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_sudo_audit_session ON sudo_audit_log(session_id);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_operation ON sudo_audit_log(operation_id);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_started ON sudo_audit_log(started_at);
CREATE INDEX IF NOT EXISTS idx_sudo_audit_success ON sudo_audit_log(success);

-- DEFAULT BLOCKLIST (critical commands that should NEVER be allowed)
INSERT INTO sudo_blocklist (name, pattern_regex, severity, is_default, description) VALUES
    ('Recursive root deletion', 'rm\s+(-rf|-fr|-r\s+-f|-f\s+-r)\s+/', 'critical', 1, 'Catastrophic file deletion from root'),
    ('DD to disk device', 'dd\s+.*of=/dev/[a-z]+$', 'critical', 1, 'Overwrite disk device'),
    ('DD to partition', 'dd\s+.*of=/dev/[a-z]+[0-9]', 'high', 1, 'Write to raw partition'),
    ('Format filesystem', 'mkfs', 'critical', 1, 'Format filesystem'),
    ('Destructive fdisk', 'fdisk.*-w', 'critical', 1, 'Destructive partition changes'),
    ('Curl pipe to shell', 'curl\s+.*\|\s*(ba)?sh', 'high', 1, 'Remote code execution via curl'),
    ('Wget pipe to shell', 'wget\s+.*\|\s*(ba)?sh', 'high', 1, 'Remote code execution via wget'),
    ('Fork bomb', ':\(\)\{:\|:&\};:', 'critical', 1, 'Fork bomb denial of service'),
    ('Overwrite MBR', 'dd.*of=/dev/[sh]da$', 'critical', 1, 'Overwrite boot sector'),
    ('Chmod 777 root', 'chmod\s+(777|a\+rwx)\s+/', 'high', 1, 'Insecure root permissions'),
    ('Delete boot', 'rm\s+.*(/boot|/etc/passwd|/etc/shadow)', 'critical', 1, 'Delete critical system files');

-- DEFAULT PERMISSIONS (common safe operations)
INSERT INTO sudo_permissions (name, command_prefix, requires_approval, description) VALUES
    ('Mira service restart', 'systemctl restart mira-', 0, 'Restart Mira services without approval'),
    ('Mira service status', 'systemctl status mira-', 0, 'Check Mira service status without approval'),
    ('Mira service stop', 'systemctl stop mira-', 0, 'Stop Mira services without approval'),
    ('Mira service start', 'systemctl start mira-', 0, 'Start Mira services without approval'),
    ('Apt update', 'apt update', 0, 'Update package lists without approval'),
    ('Apt upgrade', 'apt upgrade', 1, 'Upgrade packages (requires approval)'),
    ('Apt install', 'apt install ', 1, 'Install packages (requires approval)'),
    ('Systemctl restart', 'systemctl restart ', 1, 'Restart any service (requires approval)'),
    ('Systemctl enable', 'systemctl enable ', 1, 'Enable services (requires approval)'),
    ('Systemctl disable', 'systemctl disable ', 1, 'Disable services (requires approval)');
