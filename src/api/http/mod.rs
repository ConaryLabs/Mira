// src/api/http/mod.rs
// HTTP API module with handlers, routing, Git operations, and memory endpoints

// Submodules
pub mod handlers;
pub mod router;
pub mod chat;
pub mod git;
pub mod memory; // Phase 4: pin/unpin/import endpoints

// Re-export core HTTP handlers
pub use handlers::{health_handler, project_details_handler};

// Chat API
pub use chat::{
    get_chat_history,
    rest_chat_handler,
    RestChatRequest,
    RestChatResponse,
    ChatHistoryMessage,
    ChatHistoryResponse,
    HistoryQuery,
};

// Git API
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

// Memory API (Phase 4)
pub use memory::{pin_memory, unpin_memory, import_memories};

// Routers for main.rs
pub use router::{http_router, project_git_router};
