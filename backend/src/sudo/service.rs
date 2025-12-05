// src/sudo/service.rs
// Sudo Permission Service - Manages system administration command authorization
//
// This service implements a hybrid permission system:
// 1. Whitelist: Pre-approved commands execute immediately
// 2. Approval: Commands requiring user permission wait for approval
// 3. Audit: All sudo commands are logged to the database

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// A blocked command pattern from the blocklist
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SudoBlocklistEntry {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub pattern_exact: Option<String>,
    pub pattern_regex: Option<String>,
    pub pattern_prefix: Option<String>,
    pub severity: String,
    pub is_default: bool,
    pub enabled: bool,
    pub created_at: i64,
    pub notes: Option<String>,
}

/// Authorization decision for a sudo command
#[derive(Debug, Clone)]
pub enum AuthorizationDecision {
    /// Command is allowed immediately (whitelist match)
    Allowed { permission_id: i64 },

    /// Command requires user approval
    RequiresApproval { approval_request_id: String },

    /// Command is denied (no matching permission)
    Denied { reason: String },

    /// Command is blocked by blocklist (never allowed)
    BlockedByBlocklist { entry: SudoBlocklistEntry },
}

/// Sudo permission rule from database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SudoPermission {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub command_exact: Option<String>,
    pub command_pattern: Option<String>,
    pub command_prefix: Option<String>,
    pub requires_approval: bool,
    pub enabled: bool,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub last_used_at: Option<i64>,
    pub use_count: i64,
    pub notes: Option<String>,
}

/// Approval request for sudo command
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SudoApprovalRequest {
    pub id: String,
    pub command: String,
    pub working_dir: Option<String>,
    pub operation_id: Option<String>,
    pub session_id: Option<String>,
    pub requested_by: String,
    pub reason: Option<String>,
    pub status: String, // pending, approved, denied, expired
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub responded_at: Option<i64>,
    pub approved_by: Option<String>,
    pub denial_reason: Option<String>,
    pub executed_at: Option<i64>,
    pub exit_code: Option<i64>,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// Audit log entry for sudo command execution
#[derive(Debug, Clone)]
pub struct SudoAuditEntry {
    pub command: String,
    pub working_dir: Option<String>,
    pub permission_id: Option<i64>,
    pub approval_request_id: Option<String>,
    pub authorization_type: String, // 'whitelist' or 'approval'
    pub operation_id: Option<String>,
    pub session_id: Option<String>,
    pub executed_by: String,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
}

/// Service for managing sudo permissions and approvals
pub struct SudoPermissionService {
    db: Arc<SqlitePool>,
}

impl SudoPermissionService {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Check if a command is authorized and how
    ///
    /// Returns:
    /// - Allowed: Command matches whitelist and doesn't require approval
    /// - RequiresApproval: Command matches whitelist but needs user confirmation
    /// - Denied: Command doesn't match any permission
    /// - BlockedByBlocklist: Command matches blocklist and is never allowed
    pub async fn check_authorization(
        &self,
        command: &str,
        operation_id: Option<&str>,
        session_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<AuthorizationDecision> {
        // Check blocklist FIRST - blocked commands are never allowed
        if let Some(blocked) = self.check_blocklist(command).await? {
            warn!(
                "[SUDO] Command blocked by blocklist entry '{}': {}",
                blocked.name, command
            );
            return Ok(AuthorizationDecision::BlockedByBlocklist { entry: blocked });
        }

        // Fetch all enabled permissions
        let permissions: Vec<SudoPermission> = sqlx::query_as(
            "SELECT * FROM sudo_permissions WHERE enabled = 1"
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch sudo permissions")?;

        // Check each permission for a match
        for permission in permissions {
            if self.command_matches(&permission, command)? {
                info!(
                    "[SUDO] Command matches permission '{}' (id: {})",
                    permission.name, permission.id
                );

                // Update last_used_at and use_count
                sqlx::query(
                    "UPDATE sudo_permissions
                     SET last_used_at = ?, use_count = use_count + 1
                     WHERE id = ?"
                )
                .bind(chrono::Utc::now().timestamp())
                .bind(permission.id)
                .execute(&*self.db)
                .await?;

                if permission.requires_approval {
                    // Create approval request
                    let approval_id = self
                        .create_approval_request(
                            command,
                            operation_id,
                            session_id,
                            reason,
                            permission.id,
                        )
                        .await?;

                    return Ok(AuthorizationDecision::RequiresApproval {
                        approval_request_id: approval_id,
                    });
                } else {
                    // Auto-approve
                    return Ok(AuthorizationDecision::Allowed {
                        permission_id: permission.id,
                    });
                }
            }
        }

        // No match found
        warn!("[SUDO] No permission found for command: {}", command);
        Ok(AuthorizationDecision::Denied {
            reason: format!("No permission configured for command: {}", command),
        })
    }

    /// Check if a command matches a permission rule
    fn command_matches(&self, permission: &SudoPermission, command: &str) -> Result<bool> {
        // Exact match
        if let Some(ref exact) = permission.command_exact {
            if command == exact {
                return Ok(true);
            }
        }

        // Pattern match (regex)
        if let Some(ref pattern) = permission.command_pattern {
            let regex = Regex::new(pattern)
                .context(format!("Invalid regex pattern: {}", pattern))?;
            if regex.is_match(command) {
                return Ok(true);
            }
        }

        // Prefix match
        if let Some(ref prefix) = permission.command_prefix {
            if command.starts_with(prefix) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Create an approval request
    async fn create_approval_request(
        &self,
        command: &str,
        operation_id: Option<&str>,
        session_id: Option<&str>,
        reason: Option<&str>,
        _permission_id: i64,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + 300; // 5 minute expiry

        sqlx::query(
            "INSERT INTO sudo_approval_requests
             (id, command, operation_id, session_id, requested_by, reason,
              status, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)"
        )
        .bind(&id)
        .bind(command)
        .bind(operation_id)
        .bind(session_id)
        .bind("llm")
        .bind(reason)
        .bind(now)
        .bind(expires_at)
        .execute(&*self.db)
        .await
        .context("Failed to create approval request")?;

        info!(
            "[SUDO] Created approval request {} for command: {}",
            id, command
        );

        Ok(id)
    }

    /// Get pending approval requests for a session
    pub async fn get_pending_approvals(
        &self,
        session_id: &str,
    ) -> Result<Vec<SudoApprovalRequest>> {
        let requests: Vec<SudoApprovalRequest> = sqlx::query_as(
            "SELECT * FROM sudo_approval_requests
             WHERE session_id = ? AND status = 'pending'
             ORDER BY created_at DESC"
        )
        .bind(session_id)
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch pending approvals")?;

        Ok(requests)
    }

    /// Approve an approval request
    pub async fn approve_request(
        &self,
        request_id: &str,
        approved_by: &str,
    ) -> Result<SudoApprovalRequest> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "UPDATE sudo_approval_requests
             SET status = 'approved', responded_at = ?, approved_by = ?
             WHERE id = ? AND status = 'pending'"
        )
        .bind(now)
        .bind(approved_by)
        .bind(request_id)
        .execute(&*self.db)
        .await
        .context("Failed to approve request")?;

        info!("[SUDO] Approved request {} by {}", request_id, approved_by);

        self.get_approval_request(request_id).await
    }

    /// Deny an approval request
    pub async fn deny_request(
        &self,
        request_id: &str,
        approved_by: &str,
        reason: Option<&str>,
    ) -> Result<SudoApprovalRequest> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "UPDATE sudo_approval_requests
             SET status = 'denied', responded_at = ?, approved_by = ?, denial_reason = ?
             WHERE id = ? AND status = 'pending'"
        )
        .bind(now)
        .bind(approved_by)
        .bind(reason)
        .bind(request_id)
        .execute(&*self.db)
        .await
        .context("Failed to deny request")?;

        info!("[SUDO] Denied request {} by {}", request_id, approved_by);

        self.get_approval_request(request_id).await
    }

    /// Get an approval request by ID
    pub async fn get_approval_request(&self, request_id: &str) -> Result<SudoApprovalRequest> {
        let request: SudoApprovalRequest = sqlx::query_as(
            "SELECT * FROM sudo_approval_requests WHERE id = ?"
        )
        .bind(request_id)
        .fetch_one(&*self.db)
        .await
        .context("Approval request not found")?;

        Ok(request)
    }

    /// Log a sudo command execution to audit log
    pub async fn log_execution(&self, entry: SudoAuditEntry) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO sudo_audit_log
             (command, working_dir, permission_id, approval_request_id, authorization_type,
              operation_id, session_id, executed_by, started_at, completed_at, exit_code,
              stdout, stderr, success, error_message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&entry.command)
        .bind(&entry.working_dir)
        .bind(entry.permission_id)
        .bind(&entry.approval_request_id)
        .bind(&entry.authorization_type)
        .bind(&entry.operation_id)
        .bind(&entry.session_id)
        .bind(&entry.executed_by)
        .bind(now)
        .bind(now) // completed_at same as started (instant execution)
        .bind(entry.exit_code)
        .bind(&entry.stdout)
        .bind(&entry.stderr)
        .bind(entry.success)
        .bind(&entry.error_message)
        .execute(&*self.db)
        .await
        .context("Failed to log sudo execution")?;

        info!(
            "[SUDO] Logged execution: {} (success: {})",
            entry.command, entry.success
        );

        Ok(())
    }

    /// Get audit log entries for a session
    pub async fn get_audit_log(&self, session_id: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
        let entries = sqlx::query(
            "SELECT * FROM sudo_audit_log
             WHERE session_id = ?
             ORDER BY started_at DESC
             LIMIT ?"
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch audit log")?;

        let result: Vec<serde_json::Value> = entries
            .iter()
            .map(|row| {
                serde_json::json!({
                    "command": row.get::<String, _>("command"),
                    "authorization_type": row.get::<String, _>("authorization_type"),
                    "started_at": row.get::<i64, _>("started_at"),
                    "success": row.get::<bool, _>("success"),
                    "exit_code": row.get::<Option<i32>, _>("exit_code"),
                })
            })
            .collect();

        Ok(result)
    }

    /// Expire old pending approval requests
    pub async fn expire_old_requests(&self) -> Result<u64> {
        let now = chrono::Utc::now().timestamp();

        let result = sqlx::query(
            "UPDATE sudo_approval_requests
             SET status = 'expired'
             WHERE status = 'pending' AND expires_at < ?"
        )
        .bind(now)
        .execute(&*self.db)
        .await
        .context("Failed to expire old requests")?;

        if result.rows_affected() > 0 {
            info!("[SUDO] Expired {} old approval requests", result.rows_affected());
        }

        Ok(result.rows_affected())
    }

    /// Check if a command matches the blocklist
    async fn check_blocklist(&self, command: &str) -> Result<Option<SudoBlocklistEntry>> {
        let entries: Vec<SudoBlocklistEntry> = sqlx::query_as(
            "SELECT * FROM sudo_blocklist WHERE enabled = 1"
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch blocklist")?;

        for entry in entries {
            if self.blocklist_matches(&entry, command)? {
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }

    /// Check if a command matches a blocklist entry
    fn blocklist_matches(&self, entry: &SudoBlocklistEntry, command: &str) -> Result<bool> {
        // Exact match
        if let Some(ref exact) = entry.pattern_exact {
            if command == exact {
                return Ok(true);
            }
        }

        // Pattern match (regex)
        if let Some(ref pattern) = entry.pattern_regex {
            let regex = Regex::new(pattern)
                .context(format!("Invalid blocklist regex pattern: {}", pattern))?;
            if regex.is_match(command) {
                return Ok(true);
            }
        }

        // Prefix match
        if let Some(ref prefix) = entry.pattern_prefix {
            if command.starts_with(prefix) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get all blocklist entries
    pub async fn get_blocklist(&self) -> Result<Vec<SudoBlocklistEntry>> {
        let entries: Vec<SudoBlocklistEntry> = sqlx::query_as(
            "SELECT * FROM sudo_blocklist ORDER BY severity DESC, name ASC"
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch blocklist")?;

        Ok(entries)
    }

    /// Add a new blocklist entry
    pub async fn add_blocklist_entry(
        &self,
        name: &str,
        description: Option<&str>,
        pattern_exact: Option<&str>,
        pattern_regex: Option<&str>,
        pattern_prefix: Option<&str>,
        severity: &str,
        notes: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO sudo_blocklist
             (name, description, pattern_exact, pattern_regex, pattern_prefix, severity, is_default, enabled, notes)
             VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?)"
        )
        .bind(name)
        .bind(description)
        .bind(pattern_exact)
        .bind(pattern_regex)
        .bind(pattern_prefix)
        .bind(severity)
        .bind(notes)
        .execute(&*self.db)
        .await
        .context("Failed to add blocklist entry")?;

        info!("[SUDO] Added blocklist entry: {}", name);
        Ok(result.last_insert_rowid())
    }

    /// Remove a blocklist entry by ID
    pub async fn remove_blocklist_entry(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM sudo_blocklist WHERE id = ?")
            .bind(id)
            .execute(&*self.db)
            .await
            .context("Failed to remove blocklist entry")?;

        if result.rows_affected() > 0 {
            info!("[SUDO] Removed blocklist entry: {}", id);
        }
        Ok(result.rows_affected() > 0)
    }

    /// Toggle blocklist entry enabled state
    pub async fn toggle_blocklist_entry(&self, id: i64, enabled: bool) -> Result<bool> {
        let result = sqlx::query("UPDATE sudo_blocklist SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&*self.db)
            .await
            .context("Failed to toggle blocklist entry")?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all permission rules
    pub async fn get_permissions(&self) -> Result<Vec<SudoPermission>> {
        let permissions: Vec<SudoPermission> = sqlx::query_as(
            "SELECT * FROM sudo_permissions ORDER BY name ASC"
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch permissions")?;

        Ok(permissions)
    }

    /// Add a new permission rule
    pub async fn add_permission(
        &self,
        name: &str,
        description: Option<&str>,
        command_exact: Option<&str>,
        command_pattern: Option<&str>,
        command_prefix: Option<&str>,
        requires_approval: bool,
        notes: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO sudo_permissions
             (name, description, command_exact, command_pattern, command_prefix, requires_approval, enabled, notes)
             VALUES (?, ?, ?, ?, ?, ?, 1, ?)"
        )
        .bind(name)
        .bind(description)
        .bind(command_exact)
        .bind(command_pattern)
        .bind(command_prefix)
        .bind(requires_approval)
        .bind(notes)
        .execute(&*self.db)
        .await
        .context("Failed to add permission")?;

        info!("[SUDO] Added permission: {}", name);
        Ok(result.last_insert_rowid())
    }

    /// Remove a permission rule by ID
    pub async fn remove_permission(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM sudo_permissions WHERE id = ?")
            .bind(id)
            .execute(&*self.db)
            .await
            .context("Failed to remove permission")?;

        if result.rows_affected() > 0 {
            info!("[SUDO] Removed permission: {}", id);
        }
        Ok(result.rows_affected() > 0)
    }

    /// Toggle permission enabled state
    pub async fn toggle_permission(&self, id: i64, enabled: bool) -> Result<bool> {
        let result = sqlx::query("UPDATE sudo_permissions SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&*self.db)
            .await
            .context("Failed to toggle permission")?;

        Ok(result.rows_affected() > 0)
    }

    /// Update permission approval requirement
    pub async fn set_permission_requires_approval(&self, id: i64, requires_approval: bool) -> Result<bool> {
        let result = sqlx::query("UPDATE sudo_permissions SET requires_approval = ? WHERE id = ?")
            .bind(requires_approval)
            .bind(id)
            .execute(&*self.db)
            .await
            .context("Failed to update permission")?;

        Ok(result.rows_affected() > 0)
    }

    /// Update approval request with execution results
    pub async fn update_approval_with_results(
        &self,
        request_id: &str,
        exit_code: i32,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "UPDATE sudo_approval_requests
             SET executed_at = ?, exit_code = ?, output = ?, error = ?
             WHERE id = ?"
        )
        .bind(now)
        .bind(exit_code)
        .bind(output)
        .bind(error)
        .bind(request_id)
        .execute(&*self.db)
        .await
        .context("Failed to update approval with results")?;

        Ok(())
    }
}
