//! Core operations - shared implementation for MCP and Chat tools
//!
//! This module contains the pure business logic that both interfaces share.
//! MCP and Chat wrappers are thin layers that call into these operations.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    src/core/                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
//! │  │  OpContext  │  │  CoreError  │  │  Shared Types   │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────┘  │
//! │                                                         │
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │                    ops/                             ││
//! │  │  file, shell, git, web, memory, mira, council, ...  ││
//! │  └─────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────┘
//!           │                           │
//!     ┌─────┴─────┐               ┌─────┴─────┐
//!     │  src/mcp  │               │ src/chat  │
//!     │  (tools)  │               │  (tools)  │
//!     └───────────┘               └───────────┘
//! ```

mod context;
mod error;
pub mod ops;

pub use context::OpContext;
pub use error::{CoreError, CoreResult};
