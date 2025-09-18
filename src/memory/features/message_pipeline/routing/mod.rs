// src/memory/features/message_pipeline/routing/mod.rs

//! Routing modules for embedding and memory decisions
//! 
//! This module handles:
//! - `memory_routing` - Determines which embedding heads to use
//! - Future: `code_routing` - Specialized routing for code intelligence

pub mod memory_routing;
// pub mod code_routing; // Future: Code intelligence routing

// Re-export main types
pub use memory_routing::{MemoryRouter, RoutingConfig, RoutingStrategy};
