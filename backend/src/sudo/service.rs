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

/// Authorization decision for a sudo command
#[derive(Debug, Clone)]
pub enum AuthorizationDecision {
    /// Command is allowed immediately (whitelist match)
    Allowed { permission_id: i64 },

    /// Command requires user approval
    RequiresApproval { approval_request_id: String },

    /// Command is denied (no matching permission)
    Denied { reason: String },
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
    pub async fn check_authorization(
        &self,
        command: &str,
        operation_id: Option<&str>,
        session_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<AuthorizationDecision> {
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
}
