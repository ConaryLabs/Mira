// tests/mcp_integration.rs
// Integration tests for MCP tools - tests what Claude Code actually uses

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use tempfile::TempDir;

/// MCP test client that communicates with the server via JSON-RPC
struct McpTestClient {
    process: Child,
    reader: BufReader<std::process::ChildStdout>,
    request_id: i64,
    #[allow(dead_code)]
    temp_dir: TempDir,
}

impl McpTestClient {
    /// Start a new MCP server with a fresh test database
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");

        // Database URL for sqlx (needs sqlite:// prefix)
        let database_url = format!("sqlite://{}", db_path.display());

        // Run migrations using sqlx CLI
        let create_output = Command::new("sqlx")
            .args(["database", "create"])
            .env("DATABASE_URL", &database_url)
            .current_dir("/home/peter/Mira/backend")
            .output()
            .expect("Failed to run sqlx database create");

        if !create_output.status.success() {
            eprintln!(
                "sqlx database create failed: {}",
                String::from_utf8_lossy(&create_output.stderr)
            );
        }

        let migrate_output = Command::new("sqlx")
            .args(["migrate", "run"])
            .env("DATABASE_URL", &database_url)
            .current_dir("/home/peter/Mira/backend")
            .output()
            .expect("Failed to run sqlx migrate");

        if !migrate_output.status.success() {
            panic!(
                "Failed to run migrations (db: {}): {}",
                database_url,
                String::from_utf8_lossy(&migrate_output.stderr)
            );
        }

        // Verify tables were created
        let verify_output = Command::new("sqlite3")
            .args([db_path.to_str().unwrap(), ".tables"])
            .output();

        if let Ok(output) = verify_output {
            let tables = String::from_utf8_lossy(&output.stdout);
            if tables.trim().is_empty() {
                eprintln!("WARNING: Database appears empty after migrations!");
                eprintln!("DB path: {}", db_path.display());
                eprintln!("Migrate stdout: {}", String::from_utf8_lossy(&migrate_output.stdout));
            }
        }

        let mut process = Command::new("./target/release/mira")
            .env("DATABASE_URL", &database_url)
            .current_dir("/home/peter/Mira/backend")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to start MCP server");

        let stdout = process.stdout.take().expect("Failed to get stdout");
        let reader = BufReader::new(stdout);

        let mut client = Self {
            process,
            reader,
            request_id: 0,
            temp_dir,
        };

        // Initialize the MCP session
        client.initialize();

        client
    }

    /// Send a JSON-RPC request and get the response
    fn send_request(&mut self, method: &str, params: Value) -> Value {
        self.request_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params
        });

        let stdin = self.process.stdin.as_mut().expect("Failed to get stdin");
        writeln!(stdin, "{}", request).expect("Failed to write request");
        stdin.flush().expect("Failed to flush");

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("Failed to read response");

        serde_json::from_str(&response_line).expect("Failed to parse response")
    }

    /// Send a notification (no response expected)
    fn send_notification(&mut self, method: &str, params: Value) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        let stdin = self.process.stdin.as_mut().expect("Failed to get stdin");
        writeln!(stdin, "{}", notification).expect("Failed to write notification");
        stdin.flush().expect("Failed to flush");
    }

    /// Initialize the MCP session
    fn initialize(&mut self) {
        let response = self.send_request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "mcp-integration-test",
                    "version": "1.0.0"
                }
            }),
        );

        assert!(
            response.get("result").is_some(),
            "Initialize failed: {:?}",
            response
        );

        // Send initialized notification
        self.send_notification("notifications/initialized", json!({}));
    }

    /// Call an MCP tool and return the result
    fn call_tool(&mut self, name: &str, arguments: Value) -> ToolResult {
        let response = self.send_request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments
            }),
        );

        if let Some(error) = response.get("error") {
            return ToolResult {
                success: false,
                content: error.to_string(),
                raw: response,
            };
        }

        let result = response.get("result").expect("No result in response");
        let content = result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let is_error = result
            .get("isError")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);

        ToolResult {
            success: !is_error,
            content: content.to_string(),
            raw: response,
        }
    }

    /// List available tools
    fn list_tools(&mut self) -> Vec<String> {
        let response = self.send_request("tools/list", json!({}));
        response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for McpTestClient {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Result from calling an MCP tool
struct ToolResult {
    success: bool,
    content: String,
    #[allow(dead_code)]
    raw: Value,
}

impl ToolResult {
    /// Parse the content as JSON
    fn parse_json(&self) -> Option<Value> {
        serde_json::from_str(&self.content).ok()
    }
}

// ============================================================================
// Tool Discovery Tests
// ============================================================================

#[test]
fn test_all_tools_exposed() {
    let mut client = McpTestClient::new();
    let tools = client.list_tools();

    let expected_tools = vec![
        "list_sessions",
        "get_session",
        "search_memories",
        "get_recent_messages",
        "list_operations",
        "get_budget_status",
        "get_cache_stats",
        "get_tool_usage",
        "list_tables",
        "query",
        "remember",
        "recall",
        "forget",
        "get_symbols",
        "get_call_graph",
        "get_related_files",
        "get_file_experts",
        "find_similar_fixes",
        "get_change_risk",
        "find_cochange_patterns",
        "get_guidelines",
        "add_guideline",
        "create_task",
        "list_tasks",
        "get_task",
        "update_task",
        "complete_task",
        "delete_task",
        "list_documents",
        "search_documents",
        "get_document",
    ];

    for tool in &expected_tools {
        assert!(
            tools.contains(&tool.to_string()),
            "Missing tool: {}. Available: {:?}",
            tool,
            tools
        );
    }

    assert_eq!(tools.len(), 31, "Expected 31 tools, got {}", tools.len());
}

// ============================================================================
// Memory Tools Tests
// ============================================================================

#[test]
fn test_remember_and_recall() {
    let mut client = McpTestClient::new();

    // Remember something
    let result = client.call_tool(
        "remember",
        json!({
            "content": "The user prefers dark mode",
            "category": "preference",
            "project": "test-project",
            "tags": "ui,settings"
        }),
    );

    assert!(result.success, "Remember failed: {}", result.content);
    let json = result.parse_json().expect("Failed to parse remember result");
    assert_eq!(json["status"], "remembered");

    // Recall it
    let result = client.call_tool(
        "recall",
        json!({
            "query": "dark mode",
            "limit": 10
        }),
    );

    assert!(result.success, "Recall failed: {}", result.content);
    let json = result.parse_json().expect("Failed to parse recall result");
    let memories = json.as_array().expect("Expected array");
    assert!(!memories.is_empty(), "Should find at least one memory");
    assert!(
        memories[0]["content"]
            .as_str()
            .unwrap()
            .contains("dark mode"),
        "Memory content should contain 'dark mode'"
    );
}

#[test]
fn test_remember_and_forget() {
    let mut client = McpTestClient::new();

    // Remember something
    let result = client.call_tool(
        "remember",
        json!({
            "content": "Temporary fact to forget",
            "category": "fact"
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    let memory_id = json["id"].as_str().expect("Should have memory ID");

    // Forget it
    let result = client.call_tool(
        "forget",
        json!({
            "memory_id": memory_id
        }),
    );

    assert!(result.success, "Forget failed: {}", result.content);
    assert!(
        result.content.contains("forgotten"),
        "Should confirm deletion"
    );

    // Verify it's gone
    let result = client.call_tool(
        "recall",
        json!({
            "query": "Temporary fact to forget"
        }),
    );

    assert!(result.success);
    // Should either be empty array or "no memories found" message
    if let Some(json) = result.parse_json() {
        if let Some(arr) = json.as_array() {
            assert!(arr.is_empty(), "Memory should be deleted");
        }
    }
}

#[test]
fn test_recall_with_filters() {
    let mut client = McpTestClient::new();

    // Remember facts in different categories
    client.call_tool(
        "remember",
        json!({
            "content": "API key is stored in .env",
            "category": "fact",
            "project": "project-a"
        }),
    );

    client.call_tool(
        "remember",
        json!({
            "content": "Use 4 spaces for indentation",
            "category": "preference",
            "project": "project-b"
        }),
    );

    // Recall with category filter
    let result = client.call_tool(
        "recall",
        json!({
            "query": "spaces",
            "category": "preference"
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    let memories = json.as_array().unwrap();
    assert!(!memories.is_empty());
    assert_eq!(memories[0]["category"], "preference");
}

// ============================================================================
// Task Management Tests
// ============================================================================

#[test]
fn test_task_lifecycle() {
    let mut client = McpTestClient::new();

    // Create a task (no project = uses default)
    let result = client.call_tool(
        "create_task",
        json!({
            "title": "Implement user authentication",
            "description": "Add login/logout functionality",
            "priority": 2,
            "tags": "security,backend"
        }),
    );

    assert!(result.success, "Create task failed: {}", result.content);
    let json = result.parse_json().unwrap();
    assert_eq!(json["status"], "created");
    let task_id = json["task_id"].as_i64().expect("Should have task_id");

    // List tasks (no project filter)
    let result = client.call_tool(
        "list_tasks",
        json!({}),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    let tasks = json.as_array().unwrap();
    assert!(!tasks.is_empty());
    assert_eq!(tasks[0]["title"], "Implement user authentication");

    // Update task to in_progress
    let result = client.call_tool(
        "update_task",
        json!({
            "task_id": task_id,
            "status": "in_progress",
            "progress_notes": "Started working on login form"
        }),
    );

    assert!(result.success);

    // Get task details
    let result = client.call_tool(
        "get_task",
        json!({
            "task_id": task_id
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    assert_eq!(json["status"], "in_progress");

    // Complete the task
    let result = client.call_tool(
        "complete_task",
        json!({
            "task_id": task_id,
            "notes": "Authentication implemented with JWT"
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    assert_eq!(json["status"], "completed");
}

#[test]
fn test_task_with_subtasks() {
    let mut client = McpTestClient::new();

    // Create parent task (no project = uses default)
    let result = client.call_tool(
        "create_task",
        json!({
            "title": "Refactor database layer"
        }),
    );

    assert!(result.success, "Create parent task failed: {}", result.content);
    let json = result.parse_json().unwrap();
    let parent_id = json["task_id"].as_i64().expect("Should have task_id");

    // Create subtasks (uses same default project)
    let result = client.call_tool(
        "create_task",
        json!({
            "title": "Add connection pooling",
            "parent_task_id": parent_id
        }),
    );
    assert!(result.success, "Create subtask 1 failed: {}", result.content);

    let result = client.call_tool(
        "create_task",
        json!({
            "title": "Implement query caching",
            "parent_task_id": parent_id
        }),
    );
    assert!(result.success, "Create subtask 2 failed: {}", result.content);

    // Get parent task with subtasks
    let result = client.call_tool(
        "get_task",
        json!({
            "task_id": parent_id,
            "include_subtasks": true
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    let subtasks = json["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 2);
}

#[test]
fn test_delete_task() {
    let mut client = McpTestClient::new();

    // Create and delete a task
    let result = client.call_tool(
        "create_task",
        json!({
            "title": "Task to delete"
        }),
    );

    let json = result.parse_json().unwrap();
    let task_id = json["task_id"].as_i64().unwrap();

    let result = client.call_tool(
        "delete_task",
        json!({
            "task_id": task_id
        }),
    );

    assert!(result.success);
    let json = result.parse_json().unwrap();
    assert_eq!(json["status"], "deleted");

    // Verify it's gone
    let result = client.call_tool(
        "get_task",
        json!({
            "task_id": task_id
        }),
    );

    assert!(result.content.contains("not found"));
}

// ============================================================================
// Project Guidelines Tests
// ============================================================================

#[test]
fn test_guidelines() {
    let mut client = McpTestClient::new();

    // Add a guideline (global, no project)
    let result = client.call_tool(
        "add_guideline",
        json!({
            "content": "Use snake_case for function names",
            "category": "naming"
        }),
    );

    assert!(result.success, "Add guideline failed: {}", result.content);
    let json = result.parse_json().unwrap();
    assert_eq!(json["status"], "added");

    // Add another guideline
    let result = client.call_tool(
        "add_guideline",
        json!({
            "content": "All public functions must have doc comments",
            "category": "style"
        }),
    );
    assert!(result.success, "Add second guideline failed: {}", result.content);

    // Get all guidelines
    let result = client.call_tool("get_guidelines", json!({}));

    assert!(result.success, "Get guidelines failed: {}", result.content);
    let json = result.parse_json().unwrap();
    let guidelines = json.as_array().unwrap();
    assert_eq!(guidelines.len(), 2);

    // Filter by category
    let result = client.call_tool(
        "get_guidelines",
        json!({
            "category": "naming"
        }),
    );

    assert!(result.success, "Get guidelines by category failed: {}", result.content);
    let json = result.parse_json().unwrap();
    let guidelines = json.as_array().unwrap();
    assert_eq!(guidelines.len(), 1);
    assert!(guidelines[0]["content"]
        .as_str()
        .unwrap()
        .contains("snake_case"));
}

// ============================================================================
// Analytics Tools Tests
// ============================================================================

#[test]
fn test_list_tables() {
    let mut client = McpTestClient::new();

    let result = client.call_tool("list_tables", json!({}));

    assert!(result.success, "list_tables failed: {}", result.content);
    let json = result.parse_json().unwrap();
    let tables = json.as_array().unwrap();

    // Should have key tables (note: key is "table" not "name")
    let table_names: Vec<&str> = tables
        .iter()
        .filter_map(|t| t["table"].as_str())
        .collect();

    // Check for tables we care about
    assert!(
        table_names.contains(&"memory_facts"),
        "Should have memory_facts table. Found: {:?}",
        table_names
    );
    assert!(
        table_names.contains(&"project_tasks"),
        "Should have project_tasks table"
    );
    assert!(
        table_names.contains(&"coding_guidelines"),
        "Should have coding_guidelines table"
    );
}

#[test]
fn test_query_tool() {
    let mut client = McpTestClient::new();

    // First add some data
    client.call_tool(
        "remember",
        json!({
            "content": "Test memory for query",
            "category": "fact"
        }),
    );

    // Query using actual schema columns (fact_value, fact_category)
    let result = client.call_tool(
        "query",
        json!({
            "sql": "SELECT fact_value, fact_category FROM memory_facts WHERE fact_category = 'fact'",
            "limit": 10
        }),
    );

    assert!(result.success, "Query failed: {}", result.content);
    let json = result.parse_json().unwrap();
    // Query returns row_count, not actual rows
    let row_count = json["row_count"].as_i64().unwrap_or(0);
    assert!(row_count > 0, "Should have found at least one row");
}

#[test]
fn test_query_rejects_writes() {
    let mut client = McpTestClient::new();

    // Try to execute a write query
    let result = client.call_tool(
        "query",
        json!({
            "sql": "DELETE FROM memory_facts"
        }),
    );

    // Should fail or return error
    assert!(
        !result.success || result.content.to_lowercase().contains("error")
            || result.content.to_lowercase().contains("select"),
        "Write query should be rejected"
    );
}

#[test]
fn test_budget_status() {
    let mut client = McpTestClient::new();

    let result = client.call_tool("get_budget_status", json!({}));

    assert!(result.success, "get_budget_status failed: {}", result.content);
    let json = result.parse_json().unwrap();

    // Should have budget info structure (daily_spent, monthly_spent, etc.)
    assert!(
        json.get("daily_spent").is_some() || json.get("daily_limit").is_some(),
        "Should have budget information. Got: {}",
        result.content
    );
}

#[test]
fn test_cache_stats() {
    let mut client = McpTestClient::new();

    let result = client.call_tool("get_cache_stats", json!({}));

    assert!(result.success, "get_cache_stats failed: {}", result.content);
    // Even with empty cache, should return valid response
}

// ============================================================================
// Code Intelligence Tools Tests (with empty data)
// ============================================================================

#[test]
fn test_get_symbols_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_symbols",
        json!({
            "file_path": "src/main.rs"
        }),
    );

    // Should succeed but return empty or "no symbols" message
    assert!(result.success);
}

#[test]
fn test_get_call_graph() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_call_graph",
        json!({
            "function_name": "main",
            "depth": 2
        }),
    );

    assert!(result.success);
}

#[test]
fn test_get_related_files() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_related_files",
        json!({
            "file_path": "src/lib.rs",
            "limit": 5
        }),
    );

    assert!(result.success);
}

// ============================================================================
// Git Intelligence Tools Tests (with empty data)
// ============================================================================

#[test]
fn test_get_file_experts_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_file_experts",
        json!({
            "file_path": "src/main.rs"
        }),
    );

    assert!(result.success);
    // With no git data, should return empty or appropriate message
}

#[test]
fn test_find_similar_fixes_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "find_similar_fixes",
        json!({
            "error": "cannot borrow as mutable"
        }),
    );

    assert!(result.success);
}

#[test]
fn test_get_change_risk() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_change_risk",
        json!({
            "file_path": "src/main.rs"
        }),
    );

    assert!(result.success, "get_change_risk failed: {}", result.content);
    let json = result.parse_json().unwrap();

    // Should have risk assessment structure
    assert!(json.get("risk_score").is_some() || json.get("risk_level").is_some(),
        "Should have risk assessment");
}

#[test]
fn test_find_cochange_patterns_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "find_cochange_patterns",
        json!({
            "file_path": "src/main.rs"
        }),
    );

    assert!(result.success);
}

// ============================================================================
// Document Tools Tests (with empty data)
// ============================================================================

#[test]
fn test_list_documents_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool("list_documents", json!({}));

    assert!(result.success);
    // Should return empty list or "no documents" message
}

#[test]
fn test_search_documents_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "search_documents",
        json!({
            "query": "test query"
        }),
    );

    assert!(result.success);
}

#[test]
fn test_get_document_not_found() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_document",
        json!({
            "document_id": "nonexistent-doc-id"
        }),
    );

    assert!(result.success);
    assert!(result.content.contains("not found"));
}

// ============================================================================
// Session Tools Tests
// ============================================================================

#[test]
fn test_list_sessions_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool("list_sessions", json!({}));

    assert!(result.success);
}

#[test]
fn test_search_memories_empty() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "search_memories",
        json!({
            "query": "nonexistent search term"
        }),
    );

    assert!(result.success);
}

#[test]
fn test_get_tool_usage() {
    let mut client = McpTestClient::new();

    let result = client.call_tool(
        "get_tool_usage",
        json!({
            "limit": 10
        }),
    );

    assert!(result.success);
}
