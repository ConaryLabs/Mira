//! Rhai Engine construction, sandboxing, and script execution.

use crate::mcp::MiraServer;
use rhai::{Dynamic, Engine, Scope};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::bindings;
use super::convert::dynamic_to_value;

/// Resource limits for script execution.
const MAX_OPERATIONS: u64 = 100_000;
const MAX_CALL_LEVELS: usize = 32;
const MAX_STRING_SIZE: usize = 1_048_576; // 1 MB
const MAX_ARRAY_SIZE: usize = 10_000;
const MAX_MAP_SIZE: usize = 5_000;

/// Default timeout for scripts that don't contain long-running operations.
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Extended timeout for scripts containing long-running ops (index_project, diff).
const LONG_RUNNING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Function names that indicate long-running operations.
const LONG_RUNNING_FUNCTIONS: &[&str] = &["index_project", "diff"];

/// Result of script execution, carrying both the return value and any print output.
#[derive(Debug)]
pub struct ScriptResult {
    /// The script's return value.
    pub value: serde_json::Value,
    /// Lines captured from print()/debug() calls during execution.
    pub print_output: Vec<String>,
}

/// Error from script execution with structured position info.
#[derive(Debug)]
pub struct ScriptError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub elapsed_ms: u128,
    /// Lines captured from print()/debug() calls before the error.
    pub print_output: Vec<String>,
}

/// Create a sandboxed Rhai engine with Mira bindings.
fn create_engine(
    server: MiraServer,
    cancelled: Arc<AtomicBool>,
    print_buffer: Arc<Mutex<Vec<String>>>,
) -> Engine {
    let mut engine = Engine::new();

    // Sandbox: disable dangerous features
    engine.disable_symbol("eval");

    // Resource limits
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);

    // Cancellation: check the flag on every Rhai operation tick.
    // When the wall-clock timeout fires, it sets `cancelled` to true,
    // and the next operation tick terminates the engine from the inside.
    engine.on_progress(move |_| {
        if cancelled.load(Ordering::Relaxed) {
            Some(Dynamic::UNIT)
        } else {
            None
        }
    });

    // Capture print() output into the shared buffer instead of stdout.
    let buf = print_buffer.clone();
    engine.on_print(move |s| {
        buf.lock().unwrap().push(s.to_string());
    });

    // Capture debug() output too.
    let buf = print_buffer;
    engine.on_debug(move |s, _src, _pos| {
        buf.lock().unwrap().push(s.to_string());
    });

    // Register all Mira API bindings
    bindings::register_all(&mut engine, server);

    engine
}

/// Determine the appropriate timeout based on whether the script calls long-running functions.
fn timeout_for_script(code: &str) -> std::time::Duration {
    for name in LONG_RUNNING_FUNCTIONS {
        if code.contains(name) {
            return LONG_RUNNING_TIMEOUT;
        }
    }
    DEFAULT_TIMEOUT
}

/// Execute a Rhai script with access to Mira's API.
///
/// Returns a `ScriptResult` on success or a `ScriptError` on failure.
/// Applies both Rhai operation limits and a wall-clock timeout.
/// The timeout actually cancels the engine via `on_progress`, so no orphaned
/// work continues after a timeout.
pub async fn execute_script(
    server: &MiraServer,
    code: &str,
) -> Result<ScriptResult, ScriptError> {
    let server = server.clone();
    let code = code.to_string();
    let start = Instant::now();
    let timeout = timeout_for_script(&code);

    // Shared cancellation flag: set by timeout, checked by on_progress.
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_for_timeout = cancelled.clone();

    // Shared print buffer: written by on_print/on_debug, read after execution.
    let print_buffer: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let print_buffer_for_result = print_buffer.clone();

    // Run the script in a blocking thread and await it with a wall-clock timeout.
    let join_result = tokio::time::timeout(timeout, async {
        tokio::task::spawn_blocking(move || {
            let engine = create_engine(server, cancelled, print_buffer);
            let mut scope = Scope::new();
            engine.eval_with_scope::<Dynamic>(&mut scope, &code)
        })
        .await
    })
    .await;

    let print_output = print_buffer_for_result
        .lock()
        .unwrap()
        .drain(..)
        .collect::<Vec<_>>();

    let elapsed_ms = start.elapsed().as_millis();

    // Handle timeout: signal cancellation so the engine stops on next op tick.
    let join_result = match join_result {
        Ok(r) => r,
        Err(_) => {
            cancelled_for_timeout.store(true, Ordering::Relaxed);
            return Err(ScriptError {
                message: format!("Script timed out after {}s", timeout.as_secs()),
                line: 0,
                column: 0,
                elapsed_ms,
                print_output,
            });
        }
    };

    // Unwrap the JoinError (task panic)
    let result = match join_result {
        Ok(r) => r,
        Err(e) => {
            return Err(ScriptError {
                message: format!("Script task panicked: {e}"),
                line: 0,
                column: 0,
                elapsed_ms,
                print_output,
            });
        }
    };

    match result {
        Ok(dynamic) => {
            tracing::debug!("Script executed in {elapsed_ms}ms");
            Ok(ScriptResult {
                value: dynamic_to_value(dynamic),
                print_output,
            })
        }
        Err(err) => {
            let position = err.position();
            let line = position.line().unwrap_or(0);
            let col = position.position().unwrap_or(0);

            Err(ScriptError {
                message: err.to_string(),
                line,
                column: col,
                elapsed_ms,
                print_output,
            })
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
        assert_eq!(result.value, serde_json::json!(42));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_returns_map() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"#{ a: 1, b: "hello" }"#)
            .await
            .unwrap();
        assert_eq!(result.value["a"], 1);
        assert_eq!(result.value["b"], "hello");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_help_returns_reference() {
        let server = create_test_server().await;
        let result = execute_script(&server, "help()").await.unwrap();
        let text = result.value.as_str().unwrap();
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
        let result = execute_script(
            &server,
            r#"
            let data = [
                #{ name: "a", score: 0.1 },
                #{ name: "b", score: 0.9 },
                #{ name: "c", score: 0.5 },
            ];
            let top = summarize(data, 2);
            pick(top, ["name"])
        "#,
        )
        .await
        .unwrap();
        let arr = result.value.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "b");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_syntax_error() {
        let server = create_test_server().await;
        let result = execute_script(&server, "let x = !!!").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_print_captured() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"print("hello"); print("world"); 42"#)
            .await
            .unwrap();
        assert_eq!(result.value, serde_json::json!(42));
        assert_eq!(result.print_output, vec!["hello", "world"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_print_only() {
        // When the script only calls print() and returns unit, print output is still captured
        let server = create_test_server().await;
        let result = execute_script(&server, r#"print("just printing")"#)
            .await
            .unwrap();
        assert_eq!(result.value, serde_json::Value::Null); // unit -> null
        assert_eq!(result.print_output, vec!["just printing"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn script_long_running_detection() {
        // Scripts with index_project or diff get a longer timeout
        assert_eq!(timeout_for_script("index_project()"), LONG_RUNNING_TIMEOUT);
        assert_eq!(
            timeout_for_script("let r = diff(); format(r)"),
            LONG_RUNNING_TIMEOUT
        );
        assert_eq!(timeout_for_script("search(\"hello\")"), DEFAULT_TIMEOUT);
        assert_eq!(timeout_for_script("help()"), DEFAULT_TIMEOUT);
    }
}
