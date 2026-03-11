//! Teams bindings for Rhai scripts.
//!
//! Exposes `launch` to Rhai scripts, bridging it to the existing tool
//! implementation in `tools/core/launch.rs`.

use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // launch(team) -> Map
    let srv = server.clone();
    engine.register_fn(
        "launch",
        move |team: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let team = team.to_string();
            call_async_json(async move {
                core::handle_launch(&srv, team, None, None, None).await
            })
        },
    );

    // launch(team, scope) -> Map
    let srv = server.clone();
    engine.register_fn(
        "launch",
        move |team: &str, scope: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let team = team.to_string();
            let scope = scope.to_string();
            call_async_json(async move {
                core::handle_launch(&srv, team, Some(scope), None, None).await
            })
        },
    );
}
