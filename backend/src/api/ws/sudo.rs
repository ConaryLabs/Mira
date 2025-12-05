// backend/src/api/ws/sudo.rs
// WebSocket handler for sudo permission management and command approval

use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
    sudo::SudoApprovalRequest,
};

// ============================================================================
// REQUEST TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
struct ApproveRequest {
    approval_request_id: String,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DenyRequest {
    approval_request_id: String,
    reason: Option<String>,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListPendingRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct AddPermissionRequest {
    name: String,
    description: Option<String>,
    command_exact: Option<String>,
    command_pattern: Option<String>,
    command_prefix: Option<String>,
    requires_approval: Option<bool>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemovePermissionRequest {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TogglePermissionRequest {
    id: i64,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SetApprovalRequiredRequest {
    id: i64,
    requires_approval: bool,
}

#[derive(Debug, Deserialize)]
struct AddBlocklistRequest {
    name: String,
    description: Option<String>,
    pattern_exact: Option<String>,
    pattern_regex: Option<String>,
    pattern_prefix: Option<String>,
    severity: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoveBlocklistRequest {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct ToggleBlocklistRequest {
    id: i64,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct AuditLogRequest {
    session_id: String,
    limit: Option<i64>,
}

// ============================================================================
// MAIN ROUTER
// ============================================================================

pub async fn handle_sudo_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing sudo command: {}", method);

    let result = match method {
        "sudo.approve" => handle_approve(params, app_state).await,
        "sudo.deny" => handle_deny(params, app_state).await,
        "sudo.list_pending" => handle_list_pending(params, app_state).await,
        "sudo.get_permissions" => handle_get_permissions(app_state).await,
        "sudo.add_permission" => handle_add_permission(params, app_state).await,
        "sudo.remove_permission" => handle_remove_permission(params, app_state).await,
        "sudo.toggle_permission" => handle_toggle_permission(params, app_state).await,
        "sudo.set_approval_required" => handle_set_approval_required(params, app_state).await,
        "sudo.get_blocklist" => handle_get_blocklist(app_state).await,
        "sudo.add_blocklist" => handle_add_blocklist(params, app_state).await,
        "sudo.remove_blocklist" => handle_remove_blocklist(params, app_state).await,
        "sudo.toggle_blocklist" => handle_toggle_blocklist(params, app_state).await,
        "sudo.audit_log" => handle_audit_log(params, app_state).await,
        _ => {
            error!("Unknown sudo method: {}", method);
            return Err(ApiError::bad_request(format!(
                "Unknown sudo method: {}",
                method
            )));
        }
    };

    match &result {
        Ok(_) => info!("Sudo command {} completed successfully", method),
        Err(e) => error!("Sudo command {} failed: {:?}", method, e),
    }

    result
}

// ============================================================================
// APPROVAL HANDLERS
// ============================================================================

async fn handle_approve(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let req: ApproveRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;
    let user_id = req.user_id.as_deref().unwrap_or("user");

    // Approve the request
    let approval = sudo_service
        .approve_request(&req.approval_request_id, user_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to approve request: {}", e)))?;

    info!(
        "Approved sudo request {} for command: {}",
        approval.id, approval.command
    );

    // Execute the command now that it's approved
    let (exit_code, output, error) = execute_approved_command(&approval).await;

    // Update the approval request with results
    sudo_service
        .update_approval_with_results(
            &approval.id,
            exit_code,
            output.as_deref(),
            error.as_deref(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to update approval results: {}", e)))?;

    // Log to audit
    sudo_service
        .log_execution(crate::sudo::SudoAuditEntry {
            command: approval.command.clone(),
            working_dir: approval.working_dir.clone(),
            permission_id: None,
            approval_request_id: Some(approval.id.clone()),
            authorization_type: "approval".to_string(),
            operation_id: approval.operation_id.clone(),
            session_id: approval.session_id.clone(),
            executed_by: "llm".to_string(),
            exit_code: Some(exit_code),
            stdout: output.clone(),
            stderr: error.clone(),
            success: exit_code == 0,
            error_message: if exit_code != 0 {
                error.clone()
            } else {
                None
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("Failed to log execution: {}", e)))?;

    Ok(WsServerMessage::SudoApprovalResponse {
        approval_request_id: approval.id,
        status: "approved".to_string(),
        command: approval.command,
        exit_code: Some(exit_code),
        output,
        error,
    })
}

async fn handle_deny(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let req: DenyRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;
    let user_id = req.user_id.as_deref().unwrap_or("user");

    let approval = sudo_service
        .deny_request(&req.approval_request_id, user_id, req.reason.as_deref())
        .await
        .map_err(|e| ApiError::internal(format!("Failed to deny request: {}", e)))?;

    info!(
        "Denied sudo request {} for command: {}",
        approval.id, approval.command
    );

    Ok(WsServerMessage::SudoApprovalResponse {
        approval_request_id: approval.id,
        status: "denied".to_string(),
        command: approval.command,
        exit_code: None,
        output: None,
        error: req.reason,
    })
}

async fn handle_list_pending(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: ListPendingRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    // First expire any old requests
    let _ = sudo_service.expire_old_requests().await;

    let pending = sudo_service
        .get_pending_approvals(&req.session_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get pending approvals: {}", e)))?;

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_pending_approvals",
            "approvals": pending
        }),
        request_id: None,
    })
}

// ============================================================================
// PERMISSION MANAGEMENT
// ============================================================================

async fn handle_get_permissions(app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let sudo_service = &app_state.sudo_service;

    let permissions = sudo_service
        .get_permissions()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get permissions: {}", e)))?;

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_permissions",
            "permissions": permissions
        }),
        request_id: None,
    })
}

async fn handle_add_permission(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: AddPermissionRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    // Validate that at least one match criteria is provided
    if req.command_exact.is_none()
        && req.command_pattern.is_none()
        && req.command_prefix.is_none()
    {
        return Err(ApiError::bad_request(
            "At least one of command_exact, command_pattern, or command_prefix must be provided",
        ));
    }

    let sudo_service = &app_state.sudo_service;

    let id = sudo_service
        .add_permission(
            &req.name,
            req.description.as_deref(),
            req.command_exact.as_deref(),
            req.command_pattern.as_deref(),
            req.command_prefix.as_deref(),
            req.requires_approval.unwrap_or(true),
            req.notes.as_deref(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to add permission: {}", e)))?;

    info!("Added sudo permission: {} (id: {})", req.name, id);

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_permission_added",
            "id": id,
            "name": req.name
        }),
        request_id: None,
    })
}

async fn handle_remove_permission(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: RemovePermissionRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let removed = sudo_service
        .remove_permission(req.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to remove permission: {}", e)))?;

    if !removed {
        return Err(ApiError::not_found("Permission not found"));
    }

    info!("Removed sudo permission: {}", req.id);

    Ok(WsServerMessage::Status {
        message: format!("Permission {} removed", req.id),
        detail: None,
    })
}

async fn handle_toggle_permission(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: TogglePermissionRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let updated = sudo_service
        .toggle_permission(req.id, req.enabled)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to toggle permission: {}", e)))?;

    if !updated {
        return Err(ApiError::not_found("Permission not found"));
    }

    info!(
        "Toggled sudo permission {} to enabled={}",
        req.id, req.enabled
    );

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_permission_toggled",
            "id": req.id,
            "enabled": req.enabled
        }),
        request_id: None,
    })
}

async fn handle_set_approval_required(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: SetApprovalRequiredRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let updated = sudo_service
        .set_permission_requires_approval(req.id, req.requires_approval)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to update permission: {}", e)))?;

    if !updated {
        return Err(ApiError::not_found("Permission not found"));
    }

    info!(
        "Set sudo permission {} requires_approval={}",
        req.id, req.requires_approval
    );

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_permission_updated",
            "id": req.id,
            "requires_approval": req.requires_approval
        }),
        request_id: None,
    })
}

// ============================================================================
// BLOCKLIST MANAGEMENT
// ============================================================================

async fn handle_get_blocklist(app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let sudo_service = &app_state.sudo_service;

    let blocklist = sudo_service
        .get_blocklist()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get blocklist: {}", e)))?;

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_blocklist",
            "blocklist": blocklist
        }),
        request_id: None,
    })
}

async fn handle_add_blocklist(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: AddBlocklistRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    // Validate that at least one match criteria is provided
    if req.pattern_exact.is_none()
        && req.pattern_regex.is_none()
        && req.pattern_prefix.is_none()
    {
        return Err(ApiError::bad_request(
            "At least one of pattern_exact, pattern_regex, or pattern_prefix must be provided",
        ));
    }

    let sudo_service = &app_state.sudo_service;

    let id = sudo_service
        .add_blocklist_entry(
            &req.name,
            req.description.as_deref(),
            req.pattern_exact.as_deref(),
            req.pattern_regex.as_deref(),
            req.pattern_prefix.as_deref(),
            req.severity.as_deref().unwrap_or("high"),
            req.notes.as_deref(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to add blocklist entry: {}", e)))?;

    info!("Added sudo blocklist entry: {} (id: {})", req.name, id);

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_blocklist_added",
            "id": id,
            "name": req.name
        }),
        request_id: None,
    })
}

async fn handle_remove_blocklist(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: RemoveBlocklistRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let removed = sudo_service
        .remove_blocklist_entry(req.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to remove blocklist entry: {}", e)))?;

    if !removed {
        return Err(ApiError::not_found("Blocklist entry not found"));
    }

    info!("Removed sudo blocklist entry: {}", req.id);

    Ok(WsServerMessage::Status {
        message: format!("Blocklist entry {} removed", req.id),
        detail: None,
    })
}

async fn handle_toggle_blocklist(
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let req: ToggleBlocklistRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let updated = sudo_service
        .toggle_blocklist_entry(req.id, req.enabled)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to toggle blocklist entry: {}", e)))?;

    if !updated {
        return Err(ApiError::not_found("Blocklist entry not found"));
    }

    info!(
        "Toggled sudo blocklist entry {} to enabled={}",
        req.id, req.enabled
    );

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_blocklist_toggled",
            "id": req.id,
            "enabled": req.enabled
        }),
        request_id: None,
    })
}

// ============================================================================
// AUDIT LOG
// ============================================================================

async fn handle_audit_log(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let req: AuditLogRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let sudo_service = &app_state.sudo_service;

    let entries = sudo_service
        .get_audit_log(&req.session_id, req.limit.unwrap_or(50))
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get audit log: {}", e)))?;

    Ok(WsServerMessage::Data {
        data: json!({
            "type": "sudo_audit_log",
            "entries": entries
        }),
        request_id: None,
    })
}

// ============================================================================
// COMMAND EXECUTION
// ============================================================================

async fn execute_approved_command(approval: &SudoApprovalRequest) -> (i32, Option<String>, Option<String>) {
    let working_dir = approval
        .working_dir
        .as_deref()
        .unwrap_or("/tmp");

    info!(
        "Executing approved sudo command: {} in {}",
        approval.command, working_dir
    );

    // Execute with sudo
    let result = Command::new("sudo")
        .arg("-n") // Non-interactive (fail if password needed)
        .arg("sh")
        .arg("-c")
        .arg(&approval.command)
        .current_dir(working_dir)
        .output()
        .await;

    match result {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            let stdout_opt = if stdout.is_empty() {
                None
            } else {
                Some(stdout)
            };
            let stderr_opt = if stderr.is_empty() {
                None
            } else {
                Some(stderr)
            };

            if exit_code == 0 {
                info!("Sudo command completed successfully");
            } else {
                warn!("Sudo command failed with exit code {}", exit_code);
            }

            (exit_code, stdout_opt, stderr_opt)
        }
        Err(e) => {
            error!("Failed to execute sudo command: {}", e);
            (-1, None, Some(format!("Failed to execute command: {}", e)))
        }
    }
}
