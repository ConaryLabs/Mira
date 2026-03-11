//! Mira API bindings for Rhai scripts.

pub mod code;
pub mod goals;
pub mod helpers;
pub mod insights;
pub mod project;
pub mod session;

use crate::mcp::MiraServer;
use rhai::Engine;

/// Register all Mira API bindings on a Rhai engine.
pub fn register_all(engine: &mut Engine, server: MiraServer) {
    helpers::register(engine, server.clone());
    code::register(engine, server.clone());
    goals::register(engine, server.clone());
    project::register(engine, server.clone());
    session::register(engine, server.clone());
    insights::register(engine, server.clone());
}
