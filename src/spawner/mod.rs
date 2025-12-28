//! Claude Code Spawner
//!
//! Manages spawning and lifecycle of Claude Code sessions from Mira.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    MIRA ORCHESTRATOR                         │
//! │  • Strategic planning with 2M context                        │
//! │  • Builds ContextSnapshot for handoff                        │
//! │  • Reviews session output                                    │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ClaudeCodeSpawner                         │
//! │  • spawn() - Start Claude Code with config                   │
//! │  • inject_message() - Send message to running session        │
//! │  • answer_question() - Relay user answers                    │
//! │  • terminate() - Gracefully end session                      │
//! │  • subscribe() - Get SSE events for Studio                   │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    CLAUDE CODE (spawned)                     │
//! │  • --dangerously-skip-permissions (trusted sandbox)          │
//! │  • --input-format stream-json (bidirectional)                │
//! │  • --output-format stream-json (parseable events)            │
//! │  • --mcp-config (Mira MCP for memory/context)                │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! let spawner = ClaudeCodeSpawner::new(db.clone(), SpawnerConfig::from_env());
//!
//! // Subscribe to events for SSE
//! let mut events = spawner.subscribe();
//!
//! // Spawn a session
//! let config = SpawnConfig::new("/path/to/project", "Implement feature X")
//!     .with_context(context_snapshot)
//!     .with_budget(5.0);
//!
//! let session_id = spawner.spawn(config).await?;
//!
//! // Inject additional instructions
//! spawner.inject_message(&session_id, "Also add tests").await?;
//!
//! // Answer a question from Claude Code
//! spawner.answer_question("q_123", "Use option A").await?;
//!
//! // Gracefully terminate
//! let exit_code = spawner.terminate(&session_id).await?;
//! ```

mod context;
mod process;
mod stream;
pub mod types;

pub use context::build_context_snapshot;

pub use process::ClaudeCodeSpawner;
#[allow(unused_imports)]
pub use stream::{detect_question, DetectedQuestion, StreamParser};
#[allow(unused_imports)]
pub use types::{
    ContextSnapshot, CorrectionSummary, GoalSummary, PendingQuestion, QuestionOption,
    QuestionStatus, ReviewStatus, SessionEvent, SessionReview, SessionStatus, SpawnConfig,
    SpawnerConfig, StreamEvent,
};
