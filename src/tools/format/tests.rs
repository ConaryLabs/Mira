// src/tools/format/tests.rs
// Tests for all formatters

use super::*;
use serde_json::json;

// === Memory Formatter Tests ===

#[test]
fn test_remember_with_category() {
    let result = remember("test fact", "preference", Some("coding"));
    assert!(result.contains("test fact"));
    assert!(result.contains("preference"));
    assert!(result.contains("coding"));
}

#[test]
fn test_remember_without_category() {
    let result = remember("test fact", "decision", None);
    assert!(result.contains("test fact"));
    assert!(result.contains("decision"));
}

#[test]
fn test_recall_results_empty() {
    let results: Vec<serde_json::Value> = vec![];
    let output = recall_results(&results);
    assert_eq!(output, "No memories found.");
}

#[test]
fn test_recall_results_with_data() {
    let results = vec![
        json!({
            "fact_type": "preference",
            "value": "Use tabs for indentation",
            "times_used": 5
        }),
        json!({
            "fact_type": "decision",
            "value": "Chose SQLite over PostgreSQL",
            "times_used": 2,
            "score": 0.95
        }),
    ];
    let output = recall_results(&results);
    assert!(output.contains("Found 2 memories"));
    assert!(output.contains("preference"));
    assert!(output.contains("decision"));
    assert!(output.contains("(5x)"));
    assert!(output.contains("[95%]"));
}

#[test]
fn test_forgotten() {
    assert!(forgotten("abc123", true).contains("Forgotten"));
    assert!(forgotten("abc123", false).contains("Not found"));
}

// === Task/Goal Formatter Tests ===

#[test]
fn test_task_list_empty() {
    let tasks: Vec<serde_json::Value> = vec![];
    assert_eq!(task_list(&tasks), "No tasks.");
}

#[test]
fn test_task_list_with_items() {
    let tasks = vec![
        json!({"status": "completed", "title": "Fix bug", "priority": "high"}),
        json!({"status": "in_progress", "title": "Add feature"}),
        json!({"status": "pending", "title": "Write tests"}),
    ];
    let output = task_list(&tasks);
    assert!(output.contains("Fix bug [high]"));
    assert!(output.contains("Add feature"));
    assert!(output.contains("Write tests"));
}

#[test]
fn test_goal_list_with_progress() {
    let goals = vec![
        json!({"status": "in_progress", "title": "Release v1.0", "progress_percent": 75}),
        json!({"status": "completed", "title": "Setup CI", "progress_percent": 100}),
    ];
    let output = goal_list(&goals);
    assert!(output.contains("Release v1.0 (75%)"));
    assert!(output.contains("Setup CI (100%)"));
}

// === Code Intelligence Formatter Tests ===

#[test]
fn test_symbols_list_empty() {
    let symbols: Vec<serde_json::Value> = vec![];
    assert_eq!(symbols_list(&symbols), "No symbols.");
}

#[test]
fn test_symbols_list_with_data() {
    let symbols = vec![
        json!({"name": "MyStruct", "symbol_type": "struct", "start_line": 10, "end_line": 20}),
        json!({"name": "process", "symbol_type": "function", "start_line": 25, "end_line": 25}),
    ];
    let output = symbols_list(&symbols);
    assert!(output.contains("2 symbols:"));
    assert!(output.contains("MyStruct (struct) lines 10-20"));
    assert!(output.contains("process (function) line 25"));
}

#[test]
fn test_commit_list() {
    let commits = vec![
        json!({"commit_hash": "abc123def456", "message": "Fix critical bug", "author": "alice"}),
        json!({"commit_hash": "xyz789000000", "message": "Add new feature with a very long description that should be truncated at some point to keep the output clean"}),
    ];
    let output = commit_list(&commits);
    assert!(output.contains("2 commits:"));
    assert!(output.contains("abc123d Fix critical bug (alice)"));
    assert!(output.contains("xyz7890"));
    assert!(output.contains("...")); // Long message truncated
}

// === Admin Formatter Tests ===

#[test]
fn test_table_list() {
    let tables = vec![
        ("memories".to_string(), 150i64),
        ("sessions".to_string(), 25i64),
    ];
    let output = table_list(&tables);
    assert!(output.contains("2 tables"));
    assert!(output.contains("memories (150)"));
    assert!(output.contains("sessions (25)"));
}

#[test]
fn test_query_results_empty() {
    let columns: Vec<String> = vec![];
    let rows: Vec<Vec<serde_json::Value>> = vec![];
    assert_eq!(query_results(&columns, &rows), "No results.");
}

#[test]
fn test_query_results_with_data() {
    let columns = vec!["id".to_string(), "name".to_string()];
    let rows = vec![
        vec![json!(1), json!("Alice")],
        vec![json!(2), json!("Bob")],
    ];
    let output = query_results(&columns, &rows);
    assert!(output.contains("2 rows"));
    assert!(output.contains("id"));
    assert!(output.contains("name"));
    assert!(output.contains("Alice"));
    assert!(output.contains("Bob"));
}

// === Correction Formatter Tests ===

#[test]
fn test_correction_list_empty() {
    let corrections: Vec<serde_json::Value> = vec![];
    assert_eq!(correction_list(&corrections), "No corrections.");
}

#[test]
fn test_correction_list_with_data() {
    let corrections = vec![
        json!({
            "correction_type": "code_style",
            "what_was_wrong": "Using var",
            "what_is_right": "Use const/let"
        }),
    ];
    let output = correction_list(&corrections);
    assert!(output.contains("1 correction"));
    assert!(output.contains("code_style"));
    assert!(output.contains("Using var"));
    assert!(output.contains("Use const/let"));
}

// === Proactive Context Formatter Tests ===

#[test]
fn test_proactive_context_empty() {
    let ctx = json!({});
    assert_eq!(proactive_context(&ctx), "No relevant context.");
}

#[test]
fn test_proactive_context_with_corrections() {
    let ctx = json!({
        "corrections": [
            {"what_was_wrong": "Old approach", "what_is_right": "New approach"}
        ],
        "goals": [
            {"title": "Complete refactor", "progress_percent": 50}
        ]
    });
    let output = proactive_context(&ctx);
    assert!(output.contains("Corrections to follow"));
    assert!(output.contains("Old approach"));
    assert!(output.contains("Active goals"));
    assert!(output.contains("Complete refactor"));
}

// === Permission Formatter Tests ===

#[test]
fn test_permission_saved() {
    let output = permission_saved("Bash", Some("cargo "), "prefix", "project");
    assert!(output.contains("Bash"));
    assert!(output.contains("cargo"));
    assert!(output.contains("prefix"));
}

#[test]
fn test_permission_list_grouped() {
    let rules = vec![
        json!({"tool_name": "Bash", "input_pattern": "cargo ", "match_type": "prefix", "scope": "project"}),
        json!({"tool_name": "Bash", "input_pattern": "git ", "match_type": "prefix", "scope": "global"}),
        json!({"tool_name": "Read", "input_pattern": "*", "match_type": "any", "scope": "global"}),
    ];
    let output = permission_list(&rules);
    assert!(output.contains("3 permission rules"));
    assert!(output.contains("Bash:"));
    assert!(output.contains("Read:"));
}
