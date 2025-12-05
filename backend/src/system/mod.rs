// backend/src/system/mod.rs
// System environment detection for platform-aware LLM responses

pub mod detector;
pub mod types;

pub use detector::SystemDetector;
pub use types::{AvailableTool, OsInfo, PackageManager, ShellInfo, SystemContext};
