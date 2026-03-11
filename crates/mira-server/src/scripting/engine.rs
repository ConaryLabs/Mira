//! Rhai Engine construction, sandboxing, and script execution.

use crate::error::MiraError;
use crate::mcp::MiraServer;
use rhai::{Engine, Dynamic, Scope};
use std::time::Instant;

use super::bindings;
use super::convert::dynamic_to_value;

/// Resource limits for script execution.
const MAX_OPERATIONS: u64 = 100_000;
const MAX_CALL_LEVELS: usize = 32;
const MAX_STRING_SIZE: usize = 1_048_576; // 1 MB
const MAX_ARRAY_SIZE: usize = 10_000;
const MAX_MAP_SIZE: usize = 5_000;
const WALL_CLOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Create a sandboxed Rhai engine with Mira bindings.
fn create_engine(server: MiraServer) -> Engine {
    let mut engine = Engine::new();

    // Sandbox: disable dangerous features
    engine.disable_symbol("eval");

    // Resource limits
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);

    // Register all Mira API bindings
    bindings::register_all(&mut engine, server);

    engine
}

/// Execute a Rhai script with access to Mira's API.
///
/// Returns the script's return value as a `serde_json::Value`.
/// Applies both Rhai operation limits and a wall-clock timeout.
pub async fn execute_script(
    server: &MiraServer,
    code: &str,
) -> Result<serde_json::Value, MiraError> {
    let server = server.clone();
    let code = code.to_string();
    let start = Instant::now();

    // The entire Rhai execution runs inside block_in_place so that:
    // 1. Rhai's synchronous execution doesn't block an async task
    // 2. Bindings can call Handle::block_on for async Mira operations
    //    (block_in_place tells Tokio this thread is going to block)
    // Run the script in a blocking thread and await it with a wall-clock timeout.
    // spawn_blocking returns Result<Result<Dynamic, Box<EvalAltResult>>, JoinError>.
    // The timeout wraps that in Result<_, Elapsed>.
    // We peel off each layer separately: timeout, then join, then Rhai eval.
    let join_result = tokio::time::timeout(WALL_CLOCK_TIMEOUT, async {
        tokio::task::spawn_blocking(move || {
            let engine = create_engine(server);
            let mut scope = Scope::new();
            engine.eval_with_scope::<Dynamic>(&mut scope, &code)
        })
        .await
    })
    .await
    .map_err(|_| {
        MiraError::Other(format!(
            "Script timed out after {}s",
            WALL_CLOCK_TIMEOUT.as_secs()
        ))
    })?;

    // Unwrap the JoinError (task panic)
    let result = join_result.map_err(|e| MiraError::Other(format!("Script task panicked: {e}")))?;

    let elapsed_ms = start.elapsed().as_millis();

    match result {
        Ok(dynamic) => {
            tracing::debug!("Script executed in {elapsed_ms}ms");
            Ok(dynamic_to_value(dynamic))
        }
        Err(err) => {
            let position = err.position();
            let line = position.line().unwrap_or(0);
            let col = position.position().unwrap_or(0);

            let error_json = serde_json::json!({
                "error": err.to_string(),
                "line": line,
                "column": col,
                "elapsed_ms": elapsed_ms,
            });

            Err(MiraError::Other(
                serde_json::to_string(&error_json).unwrap_or_else(|_| err.to_string()),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_server() -> crate::mcp::MiraServer {
        use crate::db::pool::{CodePool, DatabasePool, MainPool};
        use std::sync::Arc;
        let pool = MainPool::new(Arc::new(DatabasePool::open_in_memory().await.unwrap()));
        let code_pool = CodePool::new(Arc::new(
            DatabasePool::open_code_db_in_memory().await.unwrap(),
        ));
        crate::mcp::MiraServer::new(pool, code_pool, None)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_returns_literal() {
        let server = create_test_server().await;
        let result = execute_script(&server, "42").await.unwrap();
        assert_eq!(result, serde_json::json!(42));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_returns_map() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"#{ a: 1, b: "hello" }"#).await.unwrap();
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], "hello");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_help_returns_reference() {
        let server = create_test_server().await;
        let result = execute_script(&server, "help()").await.unwrap();
        let text = result.as_str().unwrap();
        assert!(text.contains("search"));
        assert!(text.contains("goal_create"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_eval_disabled() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"eval("42")"#).await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_operation_limit() {
        let server = create_test_server().await;
        let result = execute_script(&server, "loop { }").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_chains_helpers() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"
            let data = [
                #{ name: "a", score: 0.1 },
                #{ name: "b", score: 0.9 },
                #{ name: "c", score: 0.5 },
            ];
            let top = summarize(data, 2);
            pick(top, ["name"])
        "#).await.unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "b");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_syntax_error() {
        let server = create_test_server().await;
        let result = execute_script(&server, "let x = !!!").await;
        assert!(result.is_err());
    }
}
