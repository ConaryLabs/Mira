//! Core operations - pure implementations shared by MCP and Chat
//!
//! Each submodule contains the actual business logic for a domain.
//! Wrappers in src/mcp and src/chat call these operations.
//!
//! # Conventions
//!
//! - Operations take `&OpContext` as first parameter
//! - Operations return `CoreResult<T>`
//! - Input/output types are defined in each module or shared in `types`
//! - No MCP or Chat-specific types allowed here

// Phase 1: Memory operations
pub mod memory;

// Phase 2: Mira tools (task, goal, correction, decision, rejected_approach)
pub mod tasks;
pub mod goals;
pub mod corrections;
pub mod decisions;
pub mod rejected;

// Re-export from mira module for backwards compatibility
pub mod mira;

// Phase 3: File, shell, git, code intelligence
// Note: web removed - replaced by Gemini's built-in google_search, code_execution, url_context
pub mod file;
pub mod shell;
pub mod git;
pub mod code_intel;

// Phase 5: Documents, build, work state, session
pub mod documents;
pub mod build;
pub mod work_state;
pub mod session;
pub mod mcp_session;
pub mod chat_summary;
pub mod chat_chain;

// Phase 6: Observability
pub mod audit;

// Phase 7: Proactive Organization
pub mod proposals;

// Future phases:
// pub mod index;
// pub mod artifacts;
// pub mod analytics;
// pub mod test;
