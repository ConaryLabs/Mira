// src/api/http/git/mod.rs
// Main module file - just coordinates the submodules

pub mod common;
pub mod repos;
pub mod files;
pub mod branches;
pub mod commits;

// Re-export all handlers for use in the main router
pub use repos::{
    attach_repo_handler,
    list_attached_repos_handler,
    sync_repo_handler,
};

pub use files::{
    get_file_tree_handler,
    get_file_content_handler,
    update_file_content_handler,
};

pub use branches::{
    list_branches,
    switch_branch,
};

pub use commits::{
    get_commit_history,
    get_commit_diff,
    get_file_at_commit,
};
