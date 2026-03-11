//! Mira API bindings for Rhai scripts.

pub mod helpers;

use crate::mcp::MiraServer;
use rhai::Engine;

/// Register all Mira API bindings on a Rhai engine.
pub fn register_all(engine: &mut Engine, server: MiraServer) {
    helpers::register(engine, server.clone());
}
