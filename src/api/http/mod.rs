// src/api/http/mod.rs
// REFACTORED VERSION - Reduced from ~420-450 lines to ~100 lines
// 
// EXTRACTED MODULES:
// - handlers.rs: HTTP handlers for health, chat, project details
// - router.rs: Router composition and route definitions  
// - chat.rs: REST chat handling and related types
// - git.rs: Git repository handlers (from git/ submodule)
//
// PRESERVED CRITICAL INTEGRATIONS:
// - http_router() and project_git_router() exports for main.rs compatibility
// - All HTTP endpoints and handlers functionality
// - Error handling now uses centralized src/api/error.rs
// - Configuration uses centralized CONFIG from src/config/mod.rs

// Import extracted modules
pub mod handlers;
pub mod router;
pub mod chat;

// Import existing git submodule (already exists as git/mod.rs)
pub mod git;

// Re-export handlers for external compatibility
pub use handlers::{
    health_handler,
    project_details_handler,
};

pub use chat::{
    get_chat_history,
    rest_chat_handler,
    RestChatRequest,
    RestChatResponse,
    ChatHistoryMessage,
    ChatHistoryResponse,
    HistoryQuery,
};

// Re-export git handlers (from existing git/ submodule)
pub use git::{
    attach_repo_handler,
    list_attached_repos_handler,
    sync_repo_handler,
    get_file_tree_handler,
    get_file_content_handler,
    update_file_content_handler,
    list_branches,
    switch_branch,
    get_commit_history,
    get_commit_diff,
    get_file_at_commit,
};

// Re-export router functions (CRITICAL: Preserve for main.rs compatibility)
pub use router::{
    http_router,
    project_git_router,
};
