# Script Runner Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all individual MCP tools with a single `run(code: string)` tool backed by a Rhai scripting engine.

**Architecture:** A new `scripting/` module embeds a sandboxed Rhai engine with registered bindings that call existing tool logic. Each binding is a thin wrapper converting Rhai types to Rust types, calling the async tool function via an async bridge, and converting results back. The MCP router shrinks to one `#[tool]` method.

**Tech Stack:** Rhai 1.x (embedded scripting), existing Mira tool internals (`tools/core/`), tokio async bridge.

**Spec:** `docs/superpowers/specs/2026-03-10-script-runner-design.md`

**Key types to know:**
- `GoalRequest.action` is `GoalAction` enum (not String) — variants: `Get`, `Create`, `BulkCreate`, `List`, `Update`, `Delete`, `AddMilestone`, `CompleteMilestone`, `DeleteMilestone`, `Sessions`
- `SessionRequest.action` is `SessionAction` enum — variants: `Recap`, `CurrentSession`, `Insights`, `DismissInsight`, etc.
- `ProjectAction` enum — variants: `Start`, `Get`, `Set`
- `IndexAction` enum — variants: `Project`, `Status`, `File`, `Compact`, `Summarize`
- Tool functions return `Result<Json<OutputType>, MiraError>` — `Json` is a newtype; access inner value with `.0`
- `MiraServer` implements `Clone` (cheap, clones Arcs) and `ToolContext`

---

## Chunk 1: Foundation

### Task 1: Add Rhai Dependency and Module Skeleton

**Files:**
- Modify: `crates/mira-server/Cargo.toml`
- Create: `crates/mira-server/src/scripting/mod.rs`
- Create: `crates/mira-server/src/scripting/engine.rs`
- Create: `crates/mira-server/src/scripting/convert.rs`
- Create: `crates/mira-server/src/scripting/bindings/mod.rs`
- Modify: `crates/mira-server/src/lib.rs`

- [ ] **Step 1: Add rhai to Cargo.toml**

Add to `crates/mira-server/Cargo.toml` under `[dependencies]`:

```toml
rhai = { version = "1", features = ["sync"] }
```

The `sync` feature makes `Engine` `Send + Sync`, required for use in async contexts.

- [ ] **Step 2: Create module skeleton**

Create `crates/mira-server/src/scripting/mod.rs`:

```rust
//! Rhai script execution engine for Mira's `run()` MCP tool.

mod bridge;
mod convert;
mod engine;
pub mod bindings;

pub use engine::execute_script;
```

Create `crates/mira-server/src/scripting/engine.rs`:

```rust
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
    let result = tokio::time::timeout(WALL_CLOCK_TIMEOUT, async {
        tokio::task::spawn_blocking(move || {
            let engine = create_engine(server);
            let mut scope = Scope::new();
            engine.eval_with_scope::<Dynamic>(&mut scope, &code)
        })
        .await
        .map_err(|e| MiraError::Other(format!("Script task panicked: {e}")))?
    })
    .await
    .map_err(|_| {
        MiraError::Other(format!(
            "Script timed out after {}s",
            WALL_CLOCK_TIMEOUT.as_secs()
        ))
    })?;

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
```

**Note on async bridge:** The engine is created _inside_ `spawn_blocking`. This means bindings run in a blocking thread context where `Handle::block_on` works correctly (no nesting). The wall-clock timeout wraps the entire `spawn_blocking` future.

Create `crates/mira-server/src/scripting/convert.rs`:

```rust
//! Conversion between Rhai Dynamic values and serde_json::Value.

use rhai::Dynamic;
use serde_json::Value;

/// Convert a serde_json::Value to a Rhai Dynamic.
pub fn value_to_dynamic(value: Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from(f)
            } else {
                Dynamic::UNIT
            }
        }
        Value::String(s) => Dynamic::from(s),
        Value::Array(arr) => {
            let rhai_arr: rhai::Array = arr.into_iter().map(value_to_dynamic).collect();
            Dynamic::from(rhai_arr)
        }
        Value::Object(map) => {
            let mut rhai_map = rhai::Map::new();
            for (k, v) in map {
                rhai_map.insert(k.into(), value_to_dynamic(v));
            }
            Dynamic::from(rhai_map)
        }
    }
}

/// Convert a Rhai Dynamic to a serde_json::Value.
pub fn dynamic_to_value(d: Dynamic) -> Value {
    if d.is_unit() {
        Value::Null
    } else if d.is::<bool>() {
        Value::Bool(d.cast::<bool>())
    } else if d.is::<i64>() {
        Value::Number(d.cast::<i64>().into())
    } else if d.is::<f64>() {
        serde_json::Number::from_f64(d.cast::<f64>())
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else if d.is::<String>() {
        Value::String(d.cast::<String>())
    } else if d.is::<rhai::ImmutableString>() {
        Value::String(d.cast::<rhai::ImmutableString>().to_string())
    } else if d.is::<rhai::Array>() {
        let arr = d.cast::<rhai::Array>();
        Value::Array(arr.into_iter().map(dynamic_to_value).collect())
    } else if d.is::<rhai::Map>() {
        let map = d.cast::<rhai::Map>();
        let obj: serde_json::Map<String, Value> = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_value(v)))
            .collect();
        Value::Object(obj)
    } else {
        Value::String(format!("{d}"))
    }
}

/// Convert a serializable Rust value to Rhai Dynamic via serde_json.
/// Used by bindings to convert tool output to Rhai values.
pub fn to_dynamic<T: serde::Serialize>(value: &T) -> Result<Dynamic, String> {
    let json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    Ok(value_to_dynamic(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_null() {
        let d = value_to_dynamic(Value::Null);
        assert!(d.is_unit());
        assert_eq!(dynamic_to_value(d), Value::Null);
    }

    #[test]
    fn roundtrip_bool() {
        let d = value_to_dynamic(Value::Bool(true));
        assert_eq!(dynamic_to_value(d), Value::Bool(true));
    }

    #[test]
    fn roundtrip_int() {
        let d = value_to_dynamic(serde_json::json!(42));
        assert_eq!(dynamic_to_value(d), serde_json::json!(42));
    }

    #[test]
    fn roundtrip_float() {
        let d = value_to_dynamic(serde_json::json!(3.14));
        let v = dynamic_to_value(d);
        assert!(v.as_f64().unwrap() - 3.14 < f64::EPSILON);
    }

    #[test]
    fn roundtrip_string() {
        let d = value_to_dynamic(serde_json::json!("hello"));
        assert_eq!(dynamic_to_value(d), serde_json::json!("hello"));
    }

    #[test]
    fn roundtrip_array() {
        let input = serde_json::json!([1, "two", true]);
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }

    #[test]
    fn roundtrip_map() {
        let input = serde_json::json!({"a": 1, "b": "two"});
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }

    #[test]
    fn nested_structure() {
        let input = serde_json::json!({
            "results": [
                {"name": "foo", "line": 10},
                {"name": "bar", "line": 20}
            ],
            "total": 2
        });
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }
}
```

Create `crates/mira-server/src/scripting/bindings/mod.rs`:

```rust
//! Mira API bindings for Rhai scripts.

pub mod helpers;

use crate::mcp::MiraServer;
use rhai::Engine;

/// Register all Mira API bindings on a Rhai engine.
pub fn register_all(engine: &mut Engine, server: MiraServer) {
    helpers::register(engine, server.clone());
}
```

- [ ] **Step 3: Add module to lib.rs**

Add `pub mod scripting;` to `crates/mira-server/src/lib.rs` alongside the other module declarations.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles with no errors. May have unused warnings (OK for now).

- [ ] **Step 5: Run conversion tests**

Run: `cargo test scripting::convert`
Expected: All 8 roundtrip tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/mira-server/src/scripting/ crates/mira-server/Cargo.toml crates/mira-server/src/lib.rs
git commit -m "feat: add rhai scripting module skeleton with engine and type conversion"
```

---

### Task 2: Async Bridge Helper

**Files:**
- Create: `crates/mira-server/src/scripting/bridge.rs`

The async bridge is the core pattern repeated in every binding. Bindings run inside `spawn_blocking` (from `execute_script`), so they can safely use `Handle::block_on` to call async Mira functions.

- [ ] **Step 1: Create bridge module**

Create `crates/mira-server/src/scripting/bridge.rs`:

```rust
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
```

**Key insight:** `call_async_json` accepts `Result<Json<T>, MiraError>` (matching what tool functions return) and unwraps the `Json<T>` newtype with `.0` before serializing. This avoids the issue of `Json<T>` itself not implementing `Serialize`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/mira-server/src/scripting/bridge.rs
git commit -m "feat: add async bridge helper for Rhai-to-Tokio calls"
```

---

### Task 3: Helper Bindings (format, summarize, pick, help)

**Files:**
- Create: `crates/mira-server/src/scripting/bindings/helpers.rs`

These are pure functions that don't need async or server access.

- [ ] **Step 1: Implement helpers.rs**

Create `crates/mira-server/src/scripting/bindings/helpers.rs`. See the spec for the full API reference text. Key functions:

- `help()` -> full API reference string
- `help(topic)` -> topic-specific help (search, symbols, callers, etc.)
- `format(data)` -> pretty-print any value as JSON string
- `summarize(results, max)` -> sort by `score` field descending, take top N
- `pick(results, fields)` -> select specific fields from array of maps

Use `&str` parameters (not `String`) where the value isn't captured by the closure, per Rhai best practices.

Register all helpers via `pub fn register(engine: &mut Engine, _server: MiraServer)`.

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo build && cargo test scripting::bindings::helpers`
Expected: Compiles and tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mira-server/src/scripting/bindings/helpers.rs
git commit -m "feat: add helper bindings (format, summarize, pick, help)"
```

---

## Chunk 2: API Bindings

### Task 4: Code Navigation Bindings

**Files:**
- Create: `crates/mira-server/src/scripting/bindings/code.rs`
- Modify: `crates/mira-server/src/scripting/bindings/mod.rs`

These bind `search`, `symbols`, `callers`, `callees` to existing functions in `tools/core/code/`.

- [ ] **Step 1: Check actual function exports**

Read `crates/mira-server/src/tools/core/mod.rs` to verify the exact names of re-exported functions. Expected: `search_code`, `get_symbols`, `find_function_callers`, `find_function_callees`. Adjust imports accordingly.

- [ ] **Step 2: Implement code bindings**

Create `crates/mira-server/src/scripting/bindings/code.rs`:

```rust
use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    // search(query) -> Array
    let srv = server.clone();
    engine.register_fn("search", move |query: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let query = query.to_string();
        call_async_json(async move {
            core::search_code(&srv, query, None).await
        })
    });

    // search(query, limit) -> Array
    let srv = server.clone();
    engine.register_fn("search", move |query: &str, limit: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let query = query.to_string();
        call_async_json(async move {
            core::search_code(&srv, query, Some(limit)).await
        })
    });

    // symbols(file_path) -> Array
    // get_symbols is sync — no async bridge needed
    engine.register_fn("symbols", |file_path: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        match core::get_symbols(file_path.to_string(), None) {
            Ok(json) => crate::scripting::convert::to_dynamic(&json.0)
                .map_err(|e| Box::new(EvalAltResult::ErrorRuntime(
                    Dynamic::from(e), rhai::Position::NONE,
                ))),
            Err(e) => Err(Box::new(EvalAltResult::ErrorRuntime(
                Dynamic::from(e.to_string()), rhai::Position::NONE,
            ))),
        }
    });

    // callers(function_name) -> Array
    let srv = server.clone();
    engine.register_fn("callers", move |function_name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let function_name = function_name.to_string();
        call_async_json(async move {
            core::find_function_callers(&srv, function_name, None).await
        })
    });

    // callees(function_name) -> Array
    let srv = server.clone();
    engine.register_fn("callees", move |function_name: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let function_name = function_name.to_string();
        call_async_json(async move {
            core::find_function_callees(&srv, function_name, None).await
        })
    });
}
```

- [ ] **Step 3: Register in bindings/mod.rs**

Add `pub mod code;` and `code::register(engine, server.clone());` to `register_all`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles. If function names don't match exports, adjust.

- [ ] **Step 5: Commit**

```bash
git add crates/mira-server/src/scripting/bindings/
git commit -m "feat: add code navigation bindings (search, symbols, callers, callees)"
```

---

### Task 5: Goal Bindings

**Files:**
- Create: `crates/mira-server/src/scripting/bindings/goals.rs`
- Modify: `crates/mira-server/src/scripting/bindings/mod.rs`

**Critical:** `GoalRequest.action` is `GoalAction` enum, not `String`. Use enum variants directly.

- [ ] **Step 1: Implement goal bindings**

Create `crates/mira-server/src/scripting/bindings/goals.rs`:

```rust
use crate::mcp::MiraServer;
use crate::mcp::requests::{GoalAction, GoalRequest};
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult, Map};

/// Build a GoalRequest with the given action and all optional fields as None.
fn make_request(action: GoalAction) -> GoalRequest {
    GoalRequest {
        action,
        goal_id: None,
        milestone_id: None,
        title: None,
        milestone_title: None,
        description: None,
        status: None,
        priority: None,
        progress_percent: None,
        weight: None,
        limit: None,
        goals: None,
        include_finished: None,
    }
}

pub fn register(engine: &mut Engine, server: MiraServer) {
    // goal_create(title) -> Map
    let srv = server.clone();
    engine.register_fn("goal_create", move |title: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Create);
        req.title = Some(title.to_string());
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_create(title, priority) -> Map
    let srv = server.clone();
    engine.register_fn("goal_create", move |title: &str, priority: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Create);
        req.title = Some(title.to_string());
        req.priority = Some(priority.to_string());
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_list() -> Array
    let srv = server.clone();
    engine.register_fn("goal_list", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move { core::goal(&srv, make_request(GoalAction::List)).await })
    });

    // goal_list(include_finished) -> Array
    // Note: the goal list action filters by include_finished, not by status string
    let srv = server.clone();
    engine.register_fn("goal_list", move |include_finished: bool| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::List);
        req.include_finished = Some(include_finished);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_get(goal_id) -> Map
    let srv = server.clone();
    engine.register_fn("goal_get", move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Get);
        req.goal_id = Some(goal_id);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_update(goal_id, fields) -> Map
    let srv = server.clone();
    engine.register_fn("goal_update", move |goal_id: i64, fields: Map| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Update);
        req.goal_id = Some(goal_id);
        req.title = fields.get("title").and_then(|v| v.clone().try_cast::<String>());
        req.description = fields.get("description").and_then(|v| v.clone().try_cast::<String>());
        req.status = fields.get("status").and_then(|v| v.clone().try_cast::<String>());
        req.priority = fields.get("priority").and_then(|v| v.clone().try_cast::<String>());
        req.progress_percent = fields.get("progress_percent").and_then(|v| v.clone().try_cast::<i64>()).map(|v| v as i32);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_delete(goal_id) -> Map
    let srv = server.clone();
    engine.register_fn("goal_delete", move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Delete);
        req.goal_id = Some(goal_id);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_sessions(goal_id) -> Array
    let srv = server.clone();
    engine.register_fn("goal_sessions", move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::Sessions);
        req.goal_id = Some(goal_id);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_bulk_create(goals_array) -> Array
    let srv = server.clone();
    engine.register_fn("goal_bulk_create", move |goals: rhai::Array| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let goals_json = serde_json::to_string(
            &goals.into_iter().map(crate::scripting::convert::dynamic_to_value).collect::<Vec<_>>()
        ).unwrap_or_default();
        let mut req = make_request(GoalAction::BulkCreate);
        req.goals = Some(goals_json);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_add_milestone(goal_id, title) -> Map
    let srv = server.clone();
    engine.register_fn("goal_add_milestone", move |goal_id: i64, title: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::AddMilestone);
        req.goal_id = Some(goal_id);
        req.milestone_title = Some(title.to_string());
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_add_milestone(goal_id, title, weight) -> Map
    let srv = server.clone();
    engine.register_fn("goal_add_milestone", move |goal_id: i64, title: &str, weight: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::AddMilestone);
        req.goal_id = Some(goal_id);
        req.milestone_title = Some(title.to_string());
        req.weight = Some(weight as i32);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_complete_milestone(milestone_id) -> Map
    let srv = server.clone();
    engine.register_fn("goal_complete_milestone", move |milestone_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::CompleteMilestone);
        req.milestone_id = Some(milestone_id);
        call_async_json(async move { core::goal(&srv, req).await })
    });

    // goal_delete_milestone(milestone_id) -> Map
    let srv = server.clone();
    engine.register_fn("goal_delete_milestone", move |milestone_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let mut req = make_request(GoalAction::DeleteMilestone);
        req.milestone_id = Some(milestone_id);
        call_async_json(async move { core::goal(&srv, req).await })
    });
}
```

**Note:** `goal_progress` is intentionally omitted — there is no `GoalAction::Progress` variant. Progress is available via `goal_get(id)` which returns the full goal including `progress_percent`. Update the `help()` text accordingly.

- [ ] **Step 2: Register in bindings/mod.rs**

Add `pub mod goals;` and `goals::register(engine, server.clone());`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`

- [ ] **Step 4: Commit**

```bash
git add crates/mira-server/src/scripting/bindings/
git commit -m "feat: add goal management bindings (full CRUD + milestones)"
```

---

### Task 6: Project, Session, and Insights Bindings

**Files:**
- Create: `crates/mira-server/src/scripting/bindings/project.rs`
- Create: `crates/mira-server/src/scripting/bindings/session.rs`
- Create: `crates/mira-server/src/scripting/bindings/insights.rs`
- Modify: `crates/mira-server/src/scripting/bindings/mod.rs`

**Critical:** `SessionRequest.action` is `SessionAction` enum. `SessionRequest` does NOT derive `Default` — construct all fields explicitly.

- [ ] **Step 1: Implement project bindings**

Create `crates/mira-server/src/scripting/bindings/project.rs`:

```rust
use crate::mcp::MiraServer;
use crate::mcp::requests::ProjectAction;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    let srv = server.clone();
    engine.register_fn("project_init", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::project(&srv, ProjectAction::Start, None, None, None).await
        })
    });

    let srv = server.clone();
    engine.register_fn("project_init", move |path: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let path = path.to_string();
        call_async_json(async move {
            core::project(&srv, ProjectAction::Start, Some(path), None, None).await
        })
    });

    let srv = server.clone();
    engine.register_fn("project_info", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::project(&srv, ProjectAction::Get, None, None, None).await
        })
    });
}
```

- [ ] **Step 2: Implement session bindings**

Create `crates/mira-server/src/scripting/bindings/session.rs`. Use `SessionAction` enum variants and construct `SessionRequest` with all fields:

```rust
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
    let srv = server.clone();
    engine.register_fn("recap", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::handle_session(&srv, make_session_request(SessionAction::Recap)).await
        })
    });

    let srv = server.clone();
    engine.register_fn("current_session", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::handle_session(&srv, make_session_request(SessionAction::CurrentSession)).await
        })
    });
}
```

- [ ] **Step 3: Implement insights bindings**

Create `crates/mira-server/src/scripting/bindings/insights.rs`. Use dedicated `query_insights` and `dismiss_insight` functions directly (they're re-exported from `tools/core`), avoiding the `SessionRequest` construction:

```rust
use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    let srv = server.clone();
    engine.register_fn("insights", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::query_insights(&srv, None, None, None, None).await
        })
    });

    let srv = server.clone();
    engine.register_fn("dismiss_insight", move |id: i64, source: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let source = source.to_string();
        call_async_json(async move {
            core::dismiss_insight(&srv, Some(id), Some(source)).await
        })
    });
}
```

- [ ] **Step 4: Register all in bindings/mod.rs and verify**

Run: `cargo build`

- [ ] **Step 5: Commit**

```bash
git add crates/mira-server/src/scripting/bindings/
git commit -m "feat: add project, session, and insights bindings"
```

---

### Task 7: Diff, Index, and Teams Bindings

**Files:**
- Create: `crates/mira-server/src/scripting/bindings/diff.rs`
- Create: `crates/mira-server/src/scripting/bindings/index.rs`
- Create: `crates/mira-server/src/scripting/bindings/teams.rs`
- Modify: `crates/mira-server/src/scripting/bindings/mod.rs`

- [ ] **Step 1: Implement diff bindings**

Create `crates/mira-server/src/scripting/bindings/diff.rs`:

```rust
use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    let srv = server.clone();
    engine.register_fn("diff", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::analyze_diff_tool(&srv, None, None, None).await
        })
    });

    let srv = server.clone();
    engine.register_fn("diff", move |from_ref: &str, to_ref: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let from_ref = from_ref.to_string();
        let to_ref = to_ref.to_string();
        call_async_json(async move {
            core::analyze_diff_tool(&srv, Some(from_ref), Some(to_ref), Some(true)).await
        })
    });
}
```

- [ ] **Step 2: Implement index bindings**

Create `crates/mira-server/src/scripting/bindings/index.rs`:

```rust
use crate::mcp::MiraServer;
use crate::mcp::requests::IndexAction;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    let srv = server.clone();
    engine.register_fn("index_project", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::index(&srv, IndexAction::Project, None, false).await
        })
    });

    let srv = server.clone();
    engine.register_fn("index_status", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        call_async_json(async move {
            core::index(&srv, IndexAction::Status, None, false).await
        })
    });
}
```

- [ ] **Step 3: Implement teams bindings**

Create `crates/mira-server/src/scripting/bindings/teams.rs`.

**Note:** Check the actual signature of `handle_launch` — it takes `team: String` directly (not `Option<String>`).

```rust
use crate::mcp::MiraServer;
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult};

pub fn register(engine: &mut Engine, server: MiraServer) {
    let srv = server.clone();
    engine.register_fn("launch", move |team: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let srv = srv.clone();
        let team = team.to_string();
        call_async_json(async move {
            // Verify handle_launch signature: (ctx, team: String, scope, members, context_budget)
            // vs (ctx, team: Option<String>, ...). Adjust accordingly.
            core::handle_launch(&srv, team, None, None, None).await
        })
    });
}
```

- [ ] **Step 4: Update bindings/mod.rs to final form**

```rust
pub mod code;
pub mod diff;
pub mod goals;
pub mod helpers;
pub mod index;
pub mod insights;
pub mod project;
pub mod session;
pub mod teams;

use crate::mcp::MiraServer;
use rhai::Engine;

pub fn register_all(engine: &mut Engine, server: MiraServer) {
    helpers::register(engine, server.clone());
    code::register(engine, server.clone());
    goals::register(engine, server.clone());
    project::register(engine, server.clone());
    session::register(engine, server.clone());
    diff::register(engine, server.clone());
    index::register(engine, server.clone());
    insights::register(engine, server.clone());
    teams::register(engine, server.clone());
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Fix any signature mismatches by checking the actual function exports in `tools/core/mod.rs`.

- [ ] **Step 6: Commit**

```bash
git add crates/mira-server/src/scripting/bindings/
git commit -m "feat: add diff, index, insights, and teams bindings"
```

---

## Chunk 3: MCP Integration and Cleanup

### Task 8: Register `run()` as Sole MCP Tool

**Files:**
- Modify: `crates/mira-server/src/mcp/router.rs`
- Modify: `crates/mira-server/src/mcp/requests.rs`

- [ ] **Step 1: Add RunRequest to requests.rs**

Add to `crates/mira-server/src/mcp/requests.rs`:

```rust
/// Request for the `run` tool — executes a Rhai script.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct RunRequest {
    /// Rhai script code to execute. Has access to Mira's full API.
    /// Call help() for the API reference, help("search") for specific functions.
    ///
    /// Available: search(query), symbols(path), callers(fn), callees(fn),
    /// goal_create/list/get/update/delete, goal_add_milestone, goal_complete_milestone,
    /// recap(), current_session(), project_init(), project_info(),
    /// diff(), index_project(), index_status(), insights(), dismiss_insight(id, source),
    /// launch(team), format(data), summarize(results, max), pick(results, fields), help().
    pub code: String,
}
```

- [ ] **Step 2: Rewrite router.rs tool surface**

Replace the `#[tool_router] impl MiraServer` block. Remove all existing `#[tool]` methods and replace with `run`:

```rust
#[tool_router]
impl MiraServer {
    #[tool(
        description = "Execute a Rhai script with access to Mira's API. Scripts can chain calls, filter results, and shape output. Call help() for the full API reference.\n\nAvailable: search(query), symbols(path), callers(fn), callees(fn), goal_create/list/get/update/delete, goal_add_milestone, goal_complete_milestone, recap(), diff(), insights(), format(data), summarize(results, n), pick(results, fields), help()."
    )]
    async fn run(
        &self,
        Parameters(req): Parameters<RunRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        // NOTE: Do NOT call get_or_create_session or maybe_auto_init_project here.
        // run_tool_call() in handler.rs already handles both for ALL tool calls.

        match crate::scripting::execute_script(self, &req.code).await {
            Ok(value) => {
                let text = serde_json::to_string_pretty(&value)
                    .unwrap_or_else(|_| "null".to_string());
                Ok(CallToolResult {
                    content: vec![Content::text(text)],
                    structured_content: Some(value),
                    is_error: Some(false),
                    meta: None,
                })
            }
            Err(e) => {
                Ok(CallToolResult::error(vec![Content::text(e.to_string())]))
            }
        }
    }
}
```

**Important:** The `output_schema` attribute is omitted. Verify that rmcp's `#[tool]` macro compiles without it. If rmcp requires it, you may need to omit or use a generic `serde_json::Value` schema.

- [ ] **Step 3: Clean up router.rs**

- Remove the `tool_result` helper function (no longer needed)
- Remove unused imports for old request/response types
- Keep `extract_result_text`, `log_tool_call`, `run_tool_call`, `submit_tool_task`, `auto_enqueue_task` — still used by handler
- Remove `tool_result` tests, keep `extract_result_text` tests

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles. Watch for: rmcp macro errors if `output_schema` is required, unused import warnings.

- [ ] **Step 5: Run tests**

Run: `cargo test`

- [ ] **Step 6: Commit**

```bash
git add crates/mira-server/src/mcp/
git commit -m "feat: replace all MCP tools with single run() script execution tool"
```

---

### Task 9: Remove Navigation Hooks

**Files:**
- Modify: `plugin/hooks/hooks.json`
- Modify or remove: `crates/mira-server/src/hooks/user_prompt.rs`
- Remove: `crates/mira-server/src/hooks/pre_tool.rs`
- Modify: `crates/mira-server/src/hooks/subagent.rs`
- Modify: `crates/mira-server/src/hooks/mod.rs`
- Modify: `crates/mira-server/src/main.rs` (CLI dispatcher)
- Modify: `crates/mira-server/src/cli/mod.rs` (HookAction enum)
- Modify: `crates/mira-server/src/utils/` or `hooks/mod.rs` (relocate `unix_now`)
- Modify: `crates/mira-server/src/ipc/ops.rs` (remove `get_user_prompt_context`)
- Modify: `crates/mira-server/src/ipc/handler.rs` (remove IPC dispatch entry)
- Modify: `crates/mira-server/src/ipc/client/` (remove client method)

- [ ] **Step 1: Remove hook entries from hooks.json**

Remove `UserPromptSubmit`, `PreToolUse`, and `SubagentStart` entries from `plugin/hooks/hooks.json`. Keep all other hooks.

- [ ] **Step 2: Relocate `unix_now` before deleting pre_tool.rs**

`pre_tool::unix_now()` is used by `hooks/precompact/` and `tools/core/launch.rs`. Move it to `hooks/mod.rs` (or `utils/`) as a `pub(crate)` function. Update all callers:
- `crates/mira-server/src/hooks/precompact/mod.rs`
- `crates/mira-server/src/hooks/precompact/tests.rs`
- `crates/mira-server/src/tools/core/launch.rs`

- [ ] **Step 3: Delete pre_tool.rs**

Remove `crates/mira-server/src/hooks/pre_tool.rs` entirely. Remove `pub mod pre_tool;` from `hooks/mod.rs`.

- [ ] **Step 4: Remove user_prompt.rs reactive logic**

The `get_team_context` function is used by `ipc/ops.rs::get_user_prompt_context()`. Since we're removing the UserPromptSubmit pipeline, remove the IPC operation too:
- Remove `get_user_prompt_context` from `ipc/ops.rs`
- Remove its dispatch entry from `ipc/handler.rs`
- Remove the client method from `ipc/client/`
- Then delete `user_prompt.rs` entirely (or keep only `get_team_context` if other hooks still need it — check references)

- [ ] **Step 5: Remove SubagentStart from subagent.rs**

Remove the `run_start()` function from `hooks/subagent.rs`. Keep `run_stop()` (SubagentStop is kept).

- [ ] **Step 6: Update CLI dispatcher**

In `crates/mira-server/src/main.rs`, remove the match arms for `HookAction::PreTool`, `HookAction::UserPrompt`, `HookAction::SubagentStart`.

In `crates/mira-server/src/cli/mod.rs`, remove the `PreTool`, `UserPrompt`, `SubagentStart` variants from `HookAction` enum.

- [ ] **Step 7: Update hooks/mod.rs**

Remove module declarations for deleted files. Update re-exports.

- [ ] **Step 8: Verify it compiles and tests pass**

Run: `cargo build && cargo test`

- [ ] **Step 9: Commit**

```bash
git add -A plugin/hooks/ crates/mira-server/src/hooks/ crates/mira-server/src/main.rs crates/mira-server/src/cli/ crates/mira-server/src/ipc/ crates/mira-server/src/tools/core/launch.rs
git commit -m "refactor: remove navigation hooks (UserPromptSubmit, PreToolUse, SubagentStart)"
```

---

### Task 10: Remove Context Injection Pipeline

**Files:**
- Remove or gut: `crates/mira-server/src/context/` directory
- Modify: `crates/mira-server/src/lib.rs`
- Modify: any remaining references

- [ ] **Step 1: Find all references to context module**

Search for `use crate::context` and `crate::context::` across the codebase. Expected consumers:
- `hooks/user_prompt.rs` (removed in Task 9)
- `ipc/ops.rs::get_user_prompt_context` (removed in Task 9)
- Possibly other references — check thoroughly

- [ ] **Step 2: Remove context module**

If all references were in removed code, delete `crates/mira-server/src/context/` entirely. Remove `pub mod context;` from `lib.rs`.

If any kept code still references it, keep only the needed items.

- [ ] **Step 3: Verify and commit**

Run: `cargo build && cargo test`

```bash
git add -A crates/mira-server/src/context/ crates/mira-server/src/lib.rs
git commit -m "refactor: remove context injection pipeline (replaced by script runner)"
```

---

### Task 11: Update Documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: `.claude/rules/tool-selection.md`
- Modify: `.claude/rules/sub-agents.md`
- Modify: `.claude/rules/task-management.md`
- Modify: `plugin/skills/` (any referencing old tool syntax)

- [ ] **Step 1: Update CLAUDE.md**

Key changes:
- Replace "Tool Selection" section — now just: use `run()` for anything semantic, Grep/Glob for literal strings
- Replace "Code Navigation Quick Reference" with Rhai script examples
- Update "Hook Integration" table to remove the three removed hooks
- Update "Anti-Patterns" — remove tool handler coordination row
- Update "Architecture" to mention scripting engine
- Replace all `code(action="search")` style notation with `run()` script examples
- Update "Mira Skills" if any skills reference old tools

- [ ] **Step 2: Update .claude/rules/ files**

- `tool-selection.md` — rewrite for `run()` scripts
- `sub-agents.md` — remove references to `code(action="search")`, update to script runner
- `task-management.md` — update `goal(action="create")` notation to `goal_create(title)` Rhai syntax

- [ ] **Step 3: Update plugin skills**

Check all files in `plugin/skills/` for old tool syntax and update.

- [ ] **Step 4: Search for stale references**

Search for `code(action=`, `goal(action=`, `session(action=`, `project(action=` in all markdown files. Update or remove.

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md .claude/rules/ plugin/skills/
git commit -m "docs: update CLAUDE.md and rules for script runner architecture"
```

---

### Task 12: Integration Tests

**Files:**
- Add tests to: `crates/mira-server/src/scripting/engine.rs` (or create separate test file)

- [ ] **Step 1: Create test server helper**

Check `crates/mira-server/src/tools/core/test_utils.rs` for existing helpers. The existing `MockToolContext` is for unit-testing individual tools. For integration testing the script runner, you need a real `MiraServer`. Check `crates/mira-server/src/ipc/tests.rs` for how it constructs one — it uses `MiraServer::new(pool, code_pool, None)` with in-memory database pools.

Create a `create_test_server()` helper in the scripting test module that:
1. Opens in-memory `DatabasePool` (main + code) using shared-cache URIs
2. Runs migrations
3. Constructs `MiraServer::new(pool, code_pool, None)`

- [ ] **Step 2: Write integration tests**

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;

    // create_test_server() helper here (see Step 1)

    #[tokio::test]
    async fn script_returns_literal() {
        let server = create_test_server().await;
        let result = execute_script(&server, "42").await.unwrap();
        assert_eq!(result, serde_json::json!(42));
    }

    #[tokio::test]
    async fn script_returns_map() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"#{ a: 1, b: "hello" }"#).await.unwrap();
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], "hello");
    }

    #[tokio::test]
    async fn script_help_returns_reference() {
        let server = create_test_server().await;
        let result = execute_script(&server, "help()").await.unwrap();
        let text = result.as_str().unwrap();
        assert!(text.contains("search"));
        assert!(text.contains("goal_create"));
    }

    #[tokio::test]
    async fn script_eval_disabled() {
        let server = create_test_server().await;
        let result = execute_script(&server, r#"eval("42")"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn script_operation_limit() {
        let server = create_test_server().await;
        let result = execute_script(&server, "loop { }").await;
        assert!(result.is_err());
    }

    #[tokio::test]
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

    #[tokio::test]
    async fn script_syntax_error() {
        let server = create_test_server().await;
        let result = execute_script(&server, "let x = !!!").await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test scripting`
Expected: All tests pass.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass. No regressions.

- [ ] **Step 5: Commit**

```bash
git add crates/mira-server/src/scripting/
git commit -m "test: add script runner integration tests"
```
