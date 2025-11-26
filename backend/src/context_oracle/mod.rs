// backend/src/context_oracle/mod.rs
// Context Oracle: Unified context gathering from all intelligence systems

pub mod gatherer;
pub mod types;

pub use gatherer::ContextOracle;
pub use types::*;
