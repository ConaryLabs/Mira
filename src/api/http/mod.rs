// src/api/http/mod.rs
// HTTP API module with handlers, routing, and Git operations

// Import modules
pub mod handlers;
pub mod router;
pub mod chat;
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

// Re-export Git handlers
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

// Re-export router functions for main.rs compatibility
pub use router::{
    http_router,
    project_git_router,
};
