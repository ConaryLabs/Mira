//! Code navigation bindings for Rhai scripts.
//!
//! Exposes `search`, `symbols`, `callers`, and `callees` to Rhai scripts,
//! bridging them to the existing tool implementations in `tools/core/code/`.

use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // search(query) -> Array
    let srv = server.clone();
    engine.register_fn(
        "search",
        move |query: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let query = query.to_string();
            call_async_json(async move { core::search_code(&srv, query, None).await })
        },
    );

    // search(query, limit) -> Array
    let srv = server.clone();
    engine.register_fn(
        "search",
        move |query: &str, limit: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let query = query.to_string();
            call_async_json(async move { core::search_code(&srv, query, Some(limit)).await })
        },
    );

    // symbols(file_path) -> Array
    // get_symbols is SYNC — no async bridge needed
    engine.register_fn(
        "symbols",
        |file_path: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            match core::get_symbols(file_path.to_string(), None) {
                Ok(json) => crate::scripting::convert::to_dynamic(&json.0).map_err(|e| {
                    Box::new(EvalAltResult::ErrorRuntime(
                        Dynamic::from(e),
                        rhai::Position::NONE,
                    ))
                }),
                Err(e) => Err(Box::new(EvalAltResult::ErrorRuntime(
                    Dynamic::from(e.to_string()),
                    rhai::Position::NONE,
                ))),
            }
        },
    );

    // callers(function_name) -> Array
    let srv = server.clone();
    engine.register_fn(
        "callers",
        move |function_name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let function_name = function_name.to_string();
            call_async_json(async move {
                core::find_function_callers(&srv, function_name, None).await
            })
        },
    );

    // callees(function_name) -> Array
    let srv = server.clone();
    engine.register_fn(
        "callees",
        move |function_name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let function_name = function_name.to_string();
            call_async_json(async move {
                core::find_function_callees(&srv, function_name, None).await
            })
        },
    );
}
