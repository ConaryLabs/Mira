// backend/src/session/mod.rs
// Dual-session architecture: Voice (eternal) + Codex (discrete) sessions
//
// Voice sessions:
// - Eternal/rolling with personality continuity
// - Uses GPT-5.1 Voice tier
// - Rolling summaries, semantic search
// - Tracks relationship and preferences
//
// Codex sessions:
// - Discrete task-scoped for code work
// - Uses GPT-5.1-Codex-Max with native compaction
// - Spawned from Voice sessions
// - Summarizes back to Voice on completion

pub mod types;
pub mod manager;
pub mod injection;
pub mod codex_spawner;
pub mod completion;
pub mod summary_generator;

pub use types::*;
pub use manager::SessionManager;
pub use injection::InjectionService;
pub use codex_spawner::{CodexSpawner, CodexEvent};
pub use completion::{CompletionDetector, CompletionMonitor, CompletionReason, CompletionSignal};
pub use summary_generator::{SessionArtifacts, SummaryGenerator};
