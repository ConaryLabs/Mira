//! Insights bindings for Rhai scripts.
//!
//! Exposes `insights` and `dismiss_insight` to Rhai scripts,
//! bridging them to the existing tool implementations in `tools/core/insights.rs`.

use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // insights() -> Map
    let srv = server.clone();
    engine.register_fn(
        "insights",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::query_insights(&srv, None, None, None, None).await
            })
        },
    );

    // dismiss_insight(id, source) -> Map
    let srv = server.clone();
    engine.register_fn(
        "dismiss_insight",
        move |id: i64, source: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let source = source.to_string();
            call_async_json(async move {
                core::dismiss_insight(&srv, Some(id), Some(source)).await
            })
        },
    );
}
