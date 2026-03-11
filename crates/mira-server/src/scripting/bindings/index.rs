//! Index bindings for Rhai scripts.
//!
//! Exposes `index_project` and `index_status` to Rhai scripts, bridging them
//! to the existing tool implementation in `tools/core/code/index.rs`.

use crate::mcp::MiraServer;
use crate::mcp::requests::IndexAction;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // index_project() -> Map
    let srv = server.clone();
    engine.register_fn(
        "index_project",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::index(&srv, IndexAction::Project, None, false).await
            })
        },
    );

    // index_project(skip_embed) -> Map
    let srv = server.clone();
    engine.register_fn(
        "index_project",
        move |skip_embed: bool| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::index(&srv, IndexAction::Project, None, skip_embed).await
            })
        },
    );

    // index_status() -> Map
    let srv = server.clone();
    engine.register_fn(
        "index_status",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::index(&srv, IndexAction::Status, None, false).await
            })
        },
    );
}
