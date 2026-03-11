//! Diff bindings for Rhai scripts.
//!
//! Exposes `diff` to Rhai scripts, bridging them to the existing tool
//! implementation in `tools/core/diff.rs`.

use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // diff() -> Map
    let srv = server.clone();
    engine.register_fn("diff", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::analyze_diff_tool(&srv, None, None, None).await
        })
    });

    // diff(from_ref, to_ref) -> Map
    let srv = server.clone();
    engine.register_fn(
        "diff",
        move |from_ref: &str, to_ref: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let from_ref = from_ref.to_string();
            let to_ref = to_ref.to_string();
            call_async_json(async move {
                core::analyze_diff_tool(&srv, Some(from_ref), Some(to_ref), Some(true)).await
            })
        },
    );
}
