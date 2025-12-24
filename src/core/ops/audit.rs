//! Audit logging for security and observability
//!
//! Provides persistent audit log for:
//! - Authentication events (success/failure)
//! - Tool call tracking
//! - Session lifecycle
//! - Security-relevant errors

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// Event types for audit log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Successful authentication
    AuthSuccess,
    /// Failed authentication attempt
    AuthFailure,
    /// Tool was called
    ToolCall,
    /// Tool call failed
    ToolError,
    /// Session started
    SessionStart,
    /// Session ended
    SessionEnd,
    /// Rate limit triggered
    RateLimited,
    /// Request rejected (size, format, etc.)
    RequestRejected,
    /// Security-relevant error
    SecurityError,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AuthSuccess => "auth_success",
            Self::AuthFailure => "auth_failure",
            Self::ToolCall => "tool_call",
            Self::ToolError => "tool_error",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::RateLimited => "rate_limited",
            Self::RequestRejected => "request_rejected",
            Self::SecurityError => "security_error",
        };
        write!(f, "{}", s)
    }
}

/// Severity levels for audit events
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuditSeverity {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        };
        write!(f, "{}", s)
    }
}

/// Source of the audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSource {
    Mcp,
    Studio,
    Sync,
    Hook,
}

impl std::fmt::Display for AuditSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Mcp => "mcp",
            Self::Studio => "studio",
            Self::Sync => "sync",
            Self::Hook => "hook",
        };
        write!(f, "{}", s)
    }
}

/// An audit event to be logged
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub event_type: AuditEventType,
    pub source: AuditSource,
    pub project_path: Option<String>,
    pub request_id: Option<String>,
    pub user_agent: Option<String>,
    pub remote_addr: Option<String>,
    pub details: serde_json::Value,
    pub severity: AuditSeverity,
}

impl AuditEvent {
    /// Create a new audit event
    pub fn new(event_type: AuditEventType, source: AuditSource) -> Self {
        Self {
            event_type,
            source,
            project_path: None,
            request_id: None,
            user_agent: None,
            remote_addr: None,
            details: serde_json::Value::Null,
            severity: AuditSeverity::Info,
        }
    }

    /// Set project path
    pub fn project(mut self, path: impl Into<String>) -> Self {
        self.project_path = Some(path.into());
        self
    }

    /// Set request ID
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Set remote address
    pub fn remote_addr(mut self, addr: impl Into<String>) -> Self {
        self.remote_addr = Some(addr.into());
        self
    }

    /// Set details as JSON
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    /// Set severity
    pub fn severity(mut self, severity: AuditSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Convenience: warn severity
    pub fn warn(self) -> Self {
        self.severity(AuditSeverity::Warn)
    }

    /// Convenience: error severity
    pub fn error(self) -> Self {
        self.severity(AuditSeverity::Error)
    }
}

/// Write an audit event to the database
pub async fn log_audit(db: &SqlitePool, event: AuditEvent) -> anyhow::Result<()> {
    let timestamp = chrono::Utc::now().timestamp();
    let details_json = serde_json::to_string(&event.details)?;

    sqlx::query(
        r#"
        INSERT INTO audit_log (timestamp, event_type, source, project_path, request_id, user_agent, remote_addr, details, severity)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(timestamp)
    .bind(event.event_type.to_string())
    .bind(event.source.to_string())
    .bind(&event.project_path)
    .bind(&event.request_id)
    .bind(&event.user_agent)
    .bind(&event.remote_addr)
    .bind(details_json)
    .bind(event.severity.to_string())
    .execute(db)
    .await?;

    // Also emit as tracing event for live observability
    match event.severity {
        AuditSeverity::Debug => tracing::debug!(
            event_type = %event.event_type,
            source = %event.source,
            project = ?event.project_path,
            request_id = ?event.request_id,
            "audit"
        ),
        AuditSeverity::Info => tracing::info!(
            event_type = %event.event_type,
            source = %event.source,
            project = ?event.project_path,
            request_id = ?event.request_id,
            "audit"
        ),
        AuditSeverity::Warn => tracing::warn!(
            event_type = %event.event_type,
            source = %event.source,
            project = ?event.project_path,
            request_id = ?event.request_id,
            "audit"
        ),
        AuditSeverity::Error => tracing::error!(
            event_type = %event.event_type,
            source = %event.source,
            project = ?event.project_path,
            request_id = ?event.request_id,
            "audit"
        ),
    }

    Ok(())
}

/// Query recent audit events
pub async fn get_recent_audit_events(
    db: &SqlitePool,
    limit: i64,
    event_type: Option<&str>,
    min_severity: Option<&str>,
) -> anyhow::Result<Vec<AuditLogEntry>> {
    let events: Vec<AuditLogEntry> = if let Some(etype) = event_type {
        sqlx::query_as(
            r#"
            SELECT id, timestamp, event_type, source, project_path, request_id, user_agent, remote_addr, details, severity
            FROM audit_log
            WHERE event_type = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#,
        )
        .bind(etype)
        .bind(limit)
        .fetch_all(db)
        .await?
    } else if let Some(_sev) = min_severity {
        // Filter by severity (warn and error only)
        sqlx::query_as(
            r#"
            SELECT id, timestamp, event_type, source, project_path, request_id, user_agent, remote_addr, details, severity
            FROM audit_log
            WHERE severity IN ('warn', 'error')
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT id, timestamp, event_type, source, project_path, request_id, user_agent, remote_addr, details, severity
            FROM audit_log
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(db)
        .await?
    };

    Ok(events)
}

/// A row from the audit_log table
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub timestamp: i64,
    pub event_type: String,
    pub source: String,
    pub project_path: Option<String>,
    pub request_id: Option<String>,
    pub user_agent: Option<String>,
    pub remote_addr: Option<String>,
    pub details: String,
    pub severity: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_builder() {
        let event = AuditEvent::new(AuditEventType::AuthFailure, AuditSource::Sync)
            .request_id("req-123")
            .remote_addr("192.168.1.1")
            .details(serde_json::json!({"reason": "invalid token"}))
            .warn();

        assert_eq!(event.event_type.to_string(), "auth_failure");
        assert_eq!(event.source.to_string(), "sync");
        assert_eq!(event.request_id, Some("req-123".to_string()));
        assert!(matches!(event.severity, AuditSeverity::Warn));
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(AuditEventType::AuthSuccess.to_string(), "auth_success");
        assert_eq!(AuditEventType::ToolCall.to_string(), "tool_call");
        assert_eq!(AuditEventType::RateLimited.to_string(), "rate_limited");
    }
}
