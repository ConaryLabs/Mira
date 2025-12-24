// src/tools/response.rs
// Helper functions for MCP tool responses - human-readable formatting

use rmcp::{ErrorData as McpError, model::{CallToolResult, Content}};
use serde::Serialize;
use serde_json::Value;
use super::format;

/// Convert an anyhow::Result to McpError
pub fn to_mcp_err(e: anyhow::Error) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// Create a success response with plain text
pub fn text_response(message: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(message.into())])
}

/// Smart JSON response - auto-detects type and formats nicely
pub fn json_response<T: Serialize>(result: T) -> CallToolResult {
    match serde_json::to_value(&result) {
        Ok(value) => text_response(format_value(&value)),
        Err(e) => text_response(format!("Serialization error: {}", e)),
    }
}

/// Smart vec response - formats list results nicely
pub fn vec_response<T: Serialize>(result: Vec<T>, empty_msg: impl Into<String>) -> CallToolResult {
    if result.is_empty() {
        text_response(empty_msg)
    } else {
        let values: Vec<Value> = result.into_iter()
            .filter_map(|r| serde_json::to_value(r).ok())
            .collect();
        let formatted = format_vec(&values);
        text_response(formatted)
    }
}

/// Create a response for an Option result
pub fn option_response<T: Serialize>(result: Option<T>, none_msg: impl Into<String>) -> CallToolResult {
    match result {
        Some(r) => json_response(r),
        None => text_response(none_msg),
    }
}

/// Format a single JSON value based on its structure
fn format_value(v: &Value) -> String {
    // Detect type by looking at keys
    if let Value::Object(obj) = v {
        // Remember response
        if obj.contains_key("status") && obj.get("status").and_then(|v| v.as_str()) == Some("remembered") {
            let key = obj.get("key").and_then(|v| v.as_str()).unwrap_or("?");
            let fact_type = obj.get("fact_type").and_then(|v| v.as_str()).unwrap_or("general");
            let category = obj.get("category").and_then(|v| v.as_str());
            return format::remember(key, fact_type, category);
        }

        // Forgotten response
        if obj.contains_key("status") && (obj.get("status").and_then(|v| v.as_str()) == Some("forgotten") ||
                                          obj.get("status").and_then(|v| v.as_str()) == Some("not_found")) {
            let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let found = obj.get("status").and_then(|v| v.as_str()) == Some("forgotten");
            return format::forgotten(id, found);
        }

        // Permission saved
        if obj.contains_key("status") && obj.get("status").and_then(|v| v.as_str()) == Some("saved")
            && obj.contains_key("tool_name") {
            let tool = obj.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
            let pattern = obj.get("input_pattern").and_then(|v| v.as_str());
            let match_type = obj.get("match_type").and_then(|v| v.as_str()).unwrap_or("prefix");
            let scope = obj.get("scope").and_then(|v| v.as_str()).unwrap_or("global");
            return format::permission_saved(tool, pattern, match_type, scope);
        }

        // Permission deleted
        if obj.contains_key("status") && obj.contains_key("rule_id") &&
           (obj.get("status").and_then(|v| v.as_str()) == Some("deleted") ||
            obj.get("status").and_then(|v| v.as_str()) == Some("not_found")) {
            let rule_id = obj.get("rule_id").and_then(|v| v.as_str()).unwrap_or("?");
            let found = obj.get("status").and_then(|v| v.as_str()) == Some("deleted");
            return format::permission_deleted(rule_id, found);
        }

        // Session stored
        if obj.contains_key("status") && obj.get("status").and_then(|v| v.as_str()) == Some("stored")
            && obj.contains_key("session_id") {
            let id = obj.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
            return format::session_stored(id);
        }

        // Task created/updated/completed
        if obj.contains_key("status") && obj.contains_key("task_id") {
            let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("?");
            let task_id = obj.get("task_id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            return format::task_action(status, task_id, title);
        }

        // Milestone added (check BEFORE goal - has milestone_id)
        if obj.contains_key("status") && obj.contains_key("milestone_id") && obj.contains_key("goal_id") {
            let milestone_id = obj.get("milestone_id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            return format::milestone_added(milestone_id, title);
        }

        // Goal created/updated (has goal_id but NOT milestone_id)
        if obj.contains_key("status") && obj.contains_key("goal_id") && !obj.contains_key("milestone_id") {
            let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("?");
            let goal_id = obj.get("goal_id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            return format::goal_action(status, goal_id, title);
        }

        // Goal detail (from get action - has id, title, status, progress_percent)
        if obj.contains_key("id") && obj.contains_key("title") && obj.contains_key("progress_percent") {
            return format::goal_detail(v);
        }

        // Correction recorded
        if obj.contains_key("status") && obj.get("status").and_then(|v| v.as_str()) == Some("recorded")
            && obj.contains_key("correction_type") {
            let ctype = obj.get("correction_type").and_then(|v| v.as_str()).unwrap_or("?");
            let scope = obj.get("scope").and_then(|v| v.as_str()).unwrap_or("global");
            return format::correction_recorded(ctype, scope);
        }

        // Project set
        if obj.contains_key("id") && obj.contains_key("path") && obj.contains_key("name")
            && !obj.contains_key("status") {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            return format::project_set(name, path);
        }

        // Get project - no active project
        if obj.contains_key("active") && obj.get("active").and_then(|v| v.as_bool()) == Some(false) {
            return "No active project. Call set_project() first.".to_string();
        }

        // Index status with stats
        if obj.contains_key("status") && obj.contains_key("path") {
            let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("complete");
            let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let stats = obj.get("stats");
            return format::index_status(status, path, stats);
        }

        // Call graph result
        if obj.contains_key("symbol") && obj.contains_key("callers") && obj.contains_key("callees") {
            let symbol = obj.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
            let callers = obj.get("callers").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            let callees = obj.get("callees").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            return format!("{}: {} callers, {} callees", symbol, callers, callees);
        }

        // Related files result
        if obj.contains_key("file") && obj.contains_key("related") {
            if let Some(related) = obj.get("related").and_then(|v| v.as_array()) {
                let as_values: Vec<Value> = related.clone();
                return format::related_files(&as_values);
            }
        }

        // Proactive context (keys: corrections, related_decisions, active_goals, relevant_memories)
        if obj.contains_key("corrections") || obj.contains_key("related_decisions") ||
           obj.contains_key("active_goals") || obj.contains_key("relevant_memories") {
            return format::proactive_context(v);
        }

        // Session context
        if obj.contains_key("recent_sessions") || obj.contains_key("pending_tasks") || obj.contains_key("active_goals") {
            return format::session_context(v);
        }

        // Query result with columns/rows
        if obj.contains_key("columns") && obj.contains_key("rows") {
            if let (Some(cols), Some(rows)) = (
                obj.get("columns").and_then(|v| v.as_array()),
                obj.get("rows").and_then(|v| v.as_array())
            ) {
                let columns: Vec<String> = cols.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                let row_data: Vec<Vec<Value>> = rows.iter()
                    .filter_map(|v| v.as_array().cloned())
                    .collect();
                return format::query_results(&columns, &row_data);
            }
        }

        // Tables list (from list_tables)
        if obj.contains_key("tables") {
            if let Some(tables) = obj.get("tables").and_then(|v| v.as_array()) {
                let table_list: Vec<(String, i64)> = tables.iter()
                    .filter_map(|t| {
                        let name = t.get("name").and_then(|v| v.as_str())?;
                        let count = t.get("row_count").and_then(|v| v.as_i64()).unwrap_or(0);
                        Some((name.to_string(), count))
                    })
                    .collect();
                return format::table_list(&table_list);
            }
        }

        // Generic status response
        if obj.contains_key("status") {
            let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
            return format!("Status: {}", status);
        }
    }

    // Array - delegate to vec formatter
    if let Value::Array(arr) = v {
        return format_vec(arr);
    }

    // Fallback: pretty JSON (but compact)
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// Format an array of values based on content type
fn format_vec(values: &[Value]) -> String {
    if values.is_empty() {
        return "No results.".to_string();
    }

    // Detect type from first element
    if let Some(Value::Object(first)) = values.first() {
        // Memory recall results
        if first.contains_key("value") && (first.contains_key("fact_type") || first.contains_key("score")) {
            return format::recall_results(values);
        }

        // Permission list
        if first.contains_key("tool_name") && first.contains_key("input_pattern") {
            return format::permission_list(values);
        }

        // Session search results
        if first.contains_key("summary") && first.contains_key("session_id") {
            return format::session_results(values);
        }

        // Task list
        if first.contains_key("title") && first.contains_key("status") && !first.contains_key("goal_id") && !first.contains_key("progress_percent") {
            return format::task_list(values);
        }

        // Goal list
        if first.contains_key("title") && first.contains_key("progress_percent") {
            return format::goal_list(values);
        }

        // Correction list
        if first.contains_key("what_was_wrong") && first.contains_key("what_is_right") {
            return format::correction_list(values);
        }

        // Commit list (commit_hash from core::ops::git)
        if first.contains_key("commit_hash") && first.contains_key("message") {
            return format::commit_list(values);
        }

        // Symbol list (type or symbol_type field)
        if first.contains_key("name") && (first.contains_key("symbol_type") || first.contains_key("type"))
            && first.contains_key("start_line") {
            return format::symbols_list(values);
        }

        // Code search results
        if first.contains_key("file_path") && (first.contains_key("symbol_name") || first.contains_key("score")) {
            return format::code_search_results(values);
        }

        // Build errors
        if first.contains_key("message") && first.contains_key("severity") {
            return format::build_errors(values);
        }

        // Guidelines
        if first.contains_key("content") && first.contains_key("category") && first.contains_key("priority") {
            return format::guidelines(values);
        }

        // Related files
        if first.contains_key("file_path") && first.contains_key("relation_type") {
            return format::related_files(values);
        }

        // Cochange patterns (file + cochange_count + confidence)
        if first.contains_key("file") && first.contains_key("cochange_count") {
            return format::cochange_patterns(values);
        }

        // Call graph edges
        if first.contains_key("caller") && first.contains_key("callee") {
            return format::call_graph(values);
        }

        // Work context entries (for session resume)
        if first.contains_key("context_type") && first.contains_key("context_key") {
            return format::work_context(values);
        }
    }

    // Fallback: count + compact representation
    let count = values.len();
    format!("{} result{}", count, if count == 1 { "" } else { "s" })
}
