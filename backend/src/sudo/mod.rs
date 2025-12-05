// src/sudo/mod.rs
// Sudo Permission System - System administration command authorization
//
// Hybrid permission system combining whitelists and user approvals

pub mod service;

pub use service::{
    AuthorizationDecision, SudoApprovalRequest, SudoAuditEntry, SudoBlocklistEntry,
    SudoPermission, SudoPermissionService,
};
