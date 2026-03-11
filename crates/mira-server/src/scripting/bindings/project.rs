//! Project bindings for Rhai scripts.
//!
//! Exposes `project_init`, `project_init(path)`, and `project_info` to Rhai scripts,
//! bridging them to the existing tool implementations in `tools/core/project/`.

use crate::mcp::MiraServer;
use crate::mcp::requests::ProjectAction;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // project_init() -> Map
    let srv = server.clone();
    engine.register_fn(
        "project_init",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::project(&srv, ProjectAction::Start, None, None, None).await
            })
        },
    );

    // project_init(path) -> Map
    let srv = server.clone();
    engine.register_fn(
        "project_init",
        move |path: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let path = path.to_string();
            call_async_json(async move {
                core::project(&srv, ProjectAction::Start, Some(path), None, None).await
            })
        },
    );

    // project_info() -> Map
    let srv = server.clone();
    engine.register_fn(
        "project_info",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::project(&srv, ProjectAction::Get, None, None, None).await
            })
        },
    );
}
