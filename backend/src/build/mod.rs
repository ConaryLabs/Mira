// src/build/mod.rs
// Build System Integration: Error tracking and fix learning

pub mod types;
pub mod runner;
pub mod parser;
pub mod tracker;
pub mod resolver;

pub use types::*;
pub use runner::BuildRunner;
pub use parser::{ErrorParser, ParsedError};
pub use tracker::BuildTracker;
pub use resolver::ErrorResolver;
