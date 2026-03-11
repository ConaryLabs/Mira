//! Session bindings for Rhai scripts.
//!
//! Exposes `recap` and `current_session` to Rhai scripts,
//! bridging them to the existing tool implementations in `tools/core/session/`.

use crate::mcp::MiraServer;
use crate::mcp::requests::{SessionAction, SessionRequest};
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

fn make_session_request(action: SessionAction) -> SessionRequest {
    SessionRequest {
        action,
        session_id: None,
        limit: None,
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    }
}

pub fn register(engine: &mut Engine, server: MiraServer) {
    // recap() -> Map
    let srv = server.clone();
    engine.register_fn(
        "recap",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::handle_session(&srv, make_session_request(SessionAction::Recap)).await
            })
        },
    );

    // current_session() -> Map
    let srv = server.clone();
    engine.register_fn(
        "current_session",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            call_async_json(async move {
                core::handle_session(
                    &srv,
                    make_session_request(SessionAction::CurrentSession),
                )
                .await
            })
        },
    );
}
