// src/synthesis/mod.rs
// Tool Synthesis: Auto-generate custom tools from codebase patterns

pub mod types;
pub mod storage;
pub mod detector;
pub mod generator;
pub mod loader;
pub mod evolver;

pub use types::*;
pub use storage::SynthesisStorage;
pub use detector::{DetectorConfig, PatternDetector};
pub use generator::{GeneratorConfig, ToolGenerator};
pub use loader::{DynamicToolLoader, Tool};
pub use evolver::{EvolverConfig, ToolEvolver};
