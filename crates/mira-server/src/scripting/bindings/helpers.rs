//! Helper functions for Rhai scripts.
use crate::mcp::MiraServer;
use crate::scripting::convert::dynamic_to_value;
use rhai::{Dynamic, Engine};

const API_REFERENCE: &str = r#"Mira Script API Reference

== Code Navigation ==
search(query)              Semantic code search. Returns array of {file_path, line, score, snippet}.
search(query, limit)       Same, with result limit.
symbols(file_path)         List definitions in a file. Returns array of {name, kind, line, end_line}.
callers(function_name)     What calls this function? Returns array of {file_path, line, caller}.
callees(function_name)     What does this function call? Returns array of {file_path, line, callee}.

== Goals ==
goal_create(title)                    Create goal. Returns map with goal details.
goal_create(title, priority)          Create goal with priority (low/medium/high/critical).
goal_bulk_create(goals_array)         Bulk create goals from array of maps.
goal_list()                           List active goals.
goal_list(include_finished)           List goals, optionally including finished.
goal_get(goal_id)                     Get goal details including progress.
goal_update(goal_id, fields)          Update goal. fields: #{title, description, status, priority, progress_percent}.
goal_delete(goal_id)                  Delete a goal.
goal_sessions(goal_id)                Get sessions associated with a goal.
goal_add_milestone(goal_id, title)    Add milestone to goal.
goal_add_milestone(goal_id, title, weight)  Add weighted milestone.
goal_complete_milestone(milestone_id) Complete a milestone (auto-updates goal progress).
goal_delete_milestone(milestone_id)   Delete a milestone.

== Project ==
project_init()             Initialize/re-init project context.
project_init(path)         Initialize with specific path.
project_info()             Get current project state.

== Session ==
recap()                    Get session recap with context.
current_session()          Get current session info.

== Analysis ==
diff()                     Analyze uncommitted changes.
diff(from_ref, to_ref)     Analyze changes between refs (includes impact).
insights()                 Get background analysis insights.
dismiss_insight(id, source) Dismiss an insight.

== Index ==
index_project()            Index/re-index project files.
index_status()             Get indexing status.

== Teams ==
launch(team)               Launch a team for collaborative work.

== Helpers ==
format(data)               Pretty-print any value as formatted text.
summarize(results, max)    Sort by score descending, take top N.
pick(results, fields)      Select specific fields from array of maps.
help()                     This reference.
help(topic)                Help on a specific function (e.g., help("search"))."#;

fn topic_help(topic: &str) -> String {
    match topic {
        "search" => r#"search(query: String) -> Array
search(query: String, limit: Int) -> Array

Performs semantic code search over the indexed codebase.

Parameters:
  query  - Natural language or keyword query describing what you're looking for
  limit  - (optional) Maximum number of results to return (default: 10)

Returns an array of maps, each containing:
  file_path  - Absolute path to the file
  line       - Line number of the match
  score      - Relevance score (higher is better)
  snippet    - Code snippet around the match

Example:
  let results = search("error handling");
  let top5 = summarize(results, 5);
  for r in top5 { print(r.file_path + ":" + r.line); }"#.to_string(),

        "symbols" => r#"symbols(file_path: String) -> Array

List all symbol definitions in a file (functions, structs, enums, traits, etc.).

Parameters:
  file_path  - Absolute path to the file to inspect

Returns an array of maps, each containing:
  name      - Symbol name
  kind      - Symbol kind (function, struct, enum, trait, impl, etc.)
  line      - Start line number
  end_line  - End line number

Example:
  let syms = symbols("/path/to/file.rs");
  for s in syms { print(s.kind + " " + s.name + " at line " + s.line); }"#.to_string(),

        "callers" => r#"callers(function_name: String) -> Array

Find all call sites that call the given function.

Parameters:
  function_name  - Name of the function to find callers of

Returns an array of maps, each containing:
  file_path  - File where the call occurs
  line       - Line number of the call
  caller     - Name of the calling function

Example:
  let callers = callers("handle_request");
  for c in callers { print(c.caller + " -> handle_request at " + c.file_path + ":" + c.line); }"#.to_string(),

        "callees" => r#"callees(function_name: String) -> Array

Find all functions called by the given function.

Parameters:
  function_name  - Name of the function to inspect

Returns an array of maps, each containing:
  file_path  - File where the callee is defined
  line       - Line number of the call site
  callee     - Name of the called function

Example:
  let callees = callees("process_data");
  for c in callees { print("process_data calls " + c.callee); }"#.to_string(),

        "goal_create" => r#"goal_create(title: String) -> Map
goal_create(title: String, priority: String) -> Map

Create a new cross-session goal.

Parameters:
  title     - Goal title/description
  priority  - (optional) "low", "medium", "high", or "critical" (default: "medium")

Returns a map with the created goal's details including:
  id, title, priority, status, progress_percent, created_at

Example:
  let goal = goal_create("Implement caching layer", "high");
  print("Created goal #" + goal.id);"#.to_string(),

        "goal_list" => r#"goal_list() -> Array
goal_list(include_finished: Bool) -> Array

List goals tracked in Mira.

Parameters:
  include_finished  - (optional) If true, include completed/abandoned goals (default: false)

Returns an array of goal maps, each containing:
  id, title, priority, status, progress_percent, milestone_count

Example:
  let goals = goal_list();
  for g in goals { print(g.title + " - " + g.progress_percent + "%"); }"#.to_string(),

        "recap" => r#"recap() -> Map

Get the current session recap including context, recent activity, and pending tasks.

Returns a map containing:
  session_id    - Current session ID
  project       - Project name and path
  recent_files  - Recently accessed files
  pending_tasks - Outstanding tasks from previous sessions
  goals         - Active goals with progress

Example:
  let ctx = recap();
  print("Session: " + ctx.session_id);
  print("Project: " + ctx.project);"#.to_string(),

        "diff" => r#"diff() -> Map
diff(from_ref: String, to_ref: String) -> Map

Analyze code changes.

Parameters (optional):
  from_ref  - Starting git ref (commit, branch, tag)
  to_ref    - Ending git ref (commit, branch, tag)

Without parameters: analyzes uncommitted (working tree) changes.
With parameters: analyzes changes between two refs and includes impact analysis.

Returns a map containing:
  summary     - Human-readable summary of changes
  files       - Array of changed files with stats
  impact      - (ref mode only) Impact analysis of changes

Example:
  let changes = diff();
  print(changes.summary);

  let release_diff = diff("v1.0.0", "HEAD");
  print("Impact: " + release_diff.impact);"#.to_string(),

        "format" => r#"format(data: Dynamic) -> String

Pretty-print any value as formatted JSON text.

Parameters:
  data  - Any Rhai value (string, number, array, map, etc.)

Returns a formatted string representation of the value.

Example:
  let results = search("auth");
  print(format(results));

  let info = #{name: "test", value: 42};
  print(format(info));"#.to_string(),

        "summarize" => r#"summarize(results: Array, max: Int) -> Array

Sort an array of result maps by score (descending) and take the top N items.

Parameters:
  results  - Array of maps, each expected to have a "score" field
  max      - Maximum number of items to return

Returns a new array with at most `max` items, sorted by score descending.
Items without a "score" field are treated as score 0.

Example:
  let results = search("database");
  let top3 = summarize(results, 3);
  for r in top3 { print(r.score + ": " + r.file_path); }"#.to_string(),

        "pick" => r#"pick(results: Array, fields: Array) -> Array

Select specific fields from each map in an array, discarding the rest.

Parameters:
  results  - Array of maps
  fields   - Array of field name strings to keep

Returns a new array of maps containing only the requested fields.
Missing fields are omitted (not set to null).

Example:
  let results = search("config");
  let slim = pick(results, ["file_path", "line"]);
  for r in slim { print(r.file_path + ":" + r.line); }"#.to_string(),

        _ => format!(
            "No detailed help available for '{topic}'.\n\nCall help() for the full API reference."
        ),
    }
}

fn rhai_format(data: Dynamic) -> String {
    let value = dynamic_to_value(data);
    serde_json::to_string_pretty(&value).unwrap_or_else(|e| format!("<format error: {e}>"))
}

fn rhai_summarize(results: rhai::Array, max: i64) -> rhai::Array {
    let max = if max < 0 { 0usize } else { max as usize };

    let mut items: Vec<(f64, Dynamic)> = results
        .into_iter()
        .map(|item| {
            let score = if let Some(map) = item.read_lock::<rhai::Map>() {
                map.get("score")
                    .and_then(|v| {
                        if v.is::<f64>() {
                            Some(v.clone().cast::<f64>())
                        } else if v.is::<i64>() {
                            Some(v.clone().cast::<i64>() as f64)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            (score, item)
        })
        .collect();

    // Sort descending by score (NaN treated as 0)
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    items.into_iter().take(max).map(|(_, item)| item).collect()
}

fn rhai_pick(results: rhai::Array, fields: rhai::Array) -> rhai::Array {
    let field_names: Vec<rhai::ImmutableString> = fields
        .into_iter()
        .filter_map(|f| {
            if f.is::<rhai::ImmutableString>() {
                Some(f.cast::<rhai::ImmutableString>())
            } else if f.is::<String>() {
                Some(rhai::ImmutableString::from(f.cast::<String>()))
            } else {
                None
            }
        })
        .collect();

    results
        .into_iter()
        .map(|item| {
            let mut new_map = rhai::Map::new();
            if let Some(map) = item.read_lock::<rhai::Map>() {
                for field in &field_names {
                    if let Some(val) = map.get(field.as_str()) {
                        new_map.insert(field.clone().into(), val.clone());
                    }
                }
            }
            Dynamic::from(new_map)
        })
        .collect()
}

pub fn register(engine: &mut Engine, _server: MiraServer) {
    // help() - full API reference
    engine.register_fn("help", || API_REFERENCE.to_string());

    // help(topic) - topic-specific help
    engine.register_fn("help", |topic: &str| topic_help(topic));

    // format(data) - pretty-print any value
    engine.register_fn("format", rhai_format);

    // summarize(results, max) - sort by score desc, take top N
    engine.register_fn("summarize", rhai_summarize);

    // pick(results, fields) - select fields from maps
    engine.register_fn("pick", rhai_pick);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Engine, Scope};

    fn make_engine() -> Engine {
        let mut engine = Engine::new();
        // We can't easily create a real MiraServer in tests, so we test helpers directly.
        engine.register_fn("help", || API_REFERENCE.to_string());
        engine.register_fn("help", |topic: &str| topic_help(topic));
        engine.register_fn("format", rhai_format);
        engine.register_fn("summarize", rhai_summarize);
        engine.register_fn("pick", rhai_pick);
        engine
    }

    #[test]
    fn help_returns_reference() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: String = engine.eval_with_scope(&mut scope, "help()").unwrap();
        assert!(result.contains("Mira Script API Reference"));
        assert!(result.contains("search(query)"));
    }

    #[test]
    fn help_topic_search() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: String = engine
            .eval_with_scope(&mut scope, r#"help("search")"#)
            .unwrap();
        assert!(result.contains("search(query"));
        assert!(result.contains("score"));
    }

    #[test]
    fn help_topic_unknown() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: String = engine
            .eval_with_scope(&mut scope, r#"help("nonexistent")"#)
            .unwrap();
        assert!(result.contains("No detailed help available"));
    }

    #[test]
    fn format_map() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: String = engine
            .eval_with_scope(&mut scope, r#"format(#{a: 1, b: "hello"})"#)
            .unwrap();
        // Should be valid JSON
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], "hello");
    }

    #[test]
    fn format_array() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: String = engine
            .eval_with_scope(&mut scope, r#"format([1, 2, 3])"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 3);
    }

    #[test]
    fn summarize_sorts_and_truncates() {
        let engine = make_engine();
        let mut scope = Scope::new();
        // Create array with scores: 0.5, 0.9, 0.1
        // After summarize(arr, 2) should return scores 0.9, 0.5
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r#"
                let arr = [
                    #{score: 0.5, name: "b"},
                    #{score: 0.9, name: "a"},
                    #{score: 0.1, name: "c"}
                ];
                summarize(arr, 2)
            "#,
            )
            .unwrap();
        assert_eq!(result.len(), 2);
        // First should be the highest score item
        let first = result[0].clone().cast::<rhai::Map>();
        assert_eq!(first["name"].clone().cast::<String>(), "a");
        let second = result[1].clone().cast::<rhai::Map>();
        assert_eq!(second["name"].clone().cast::<String>(), "b");
    }

    #[test]
    fn summarize_max_larger_than_array() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r#"
                let arr = [#{score: 1.0}, #{score: 0.5}];
                summarize(arr, 100)
            "#,
            )
            .unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn pick_selects_fields() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r#"
                let arr = [
                    #{file_path: "a.rs", line: 10, score: 0.9, snippet: "fn foo()"},
                    #{file_path: "b.rs", line: 20, score: 0.5, snippet: "fn bar()"}
                ];
                pick(arr, ["file_path", "line"])
            "#,
            )
            .unwrap();
        assert_eq!(result.len(), 2);
        let first = result[0].clone().cast::<rhai::Map>();
        assert!(first.contains_key("file_path"));
        assert!(first.contains_key("line"));
        assert!(!first.contains_key("score"));
        assert!(!first.contains_key("snippet"));
    }

    #[test]
    fn pick_missing_field_omitted() {
        let engine = make_engine();
        let mut scope = Scope::new();
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r#"
                let arr = [#{name: "x"}];
                pick(arr, ["name", "missing_field"])
            "#,
            )
            .unwrap();
        let first = result[0].clone().cast::<rhai::Map>();
        assert!(first.contains_key("name"));
        assert!(!first.contains_key("missing_field"));
    }
}
