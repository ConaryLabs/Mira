//! Async bridge for calling Mira's async functions from synchronous Rhai callbacks.
//!
//! Bindings run inside `spawn_blocking` (via `execute_script`), so
//! `Handle::block_on` is safe here — we're not inside an async task.

use rhai::{Dynamic, EvalAltResult, Position};
use std::future::Future;

/// Call an async function from a synchronous Rhai callback.
///
/// The `convert` closure transforms the async result into a `Dynamic`.
pub fn call_async<F, T, C>(future_fn: F, convert: C) -> Result<Dynamic, Box<EvalAltResult>>
where
    F: Future<Output = Result<T, crate::error::MiraError>> + Send + 'static,
    T: Send + 'static,
    C: FnOnce(T) -> Result<Dynamic, String> + Send + 'static,
{
    let handle = tokio::runtime::Handle::current();
    handle
        .block_on(future_fn)
        .map_err(|e| {
            Box::new(EvalAltResult::ErrorRuntime(
                Dynamic::from(e.to_string()),
                Position::NONE,
            ))
        })
        .and_then(|val| {
            convert(val).map_err(|e| {
                Box::new(EvalAltResult::ErrorRuntime(
                    Dynamic::from(e),
                    Position::NONE,
                ))
            })
        })
}

/// Simplified call_async that auto-converts via serde_json.
/// Unwraps the Json<T> newtype wrapper and serializes T to Dynamic.
pub fn call_async_json<F, T>(future_fn: F) -> Result<Dynamic, Box<EvalAltResult>>
where
    F: Future<
            Output = Result<
                crate::mcp::responses::Json<T>,
                crate::error::MiraError,
            >,
        > + Send
        + 'static,
    T: serde::Serialize + Send + 'static,
{
    call_async(future_fn, |json_wrapper| {
        // Unwrap Json<T> newtype to get inner T, then serialize
        super::convert::to_dynamic(&json_wrapper.0)
    })
}
