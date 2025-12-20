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

// Phase 1: Memory operations (pilot)
pub mod memory;

// Phase 2: Mira tools (task, goal, correction, decision, rejected_approach)
pub mod mira;

// Future phases:
// pub mod file;
// pub mod shell;
// pub mod git;
// pub mod web;
// pub mod code_intel;
// pub mod git_intel;
// pub mod session;
// pub mod council;
// pub mod build;
// pub mod index;
// pub mod artifacts;
// pub mod analytics;
// pub mod test;
