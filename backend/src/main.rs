// backend/src/main.rs
// Mira Power Suit - MCP Server for Claude Code
// Provides memory, code intelligence, git intelligence, and project context

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_router, tool_handler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use chrono::Utc;

// === Request/Response Types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct ListSessionsRequest {
    #[schemars(description = "Maximum number of sessions to return (default: 20)")]
    limit: Option<i64>,
    #[schemars(description = "Filter by session type: 'voice' or 'codex'")]
    session_type: Option<String>,
    #[schemars(description = "Filter by status: 'active', 'committed', 'archived'")]
    status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetSessionRequest {
    #[schemars(description = "The session ID to look up")]
    session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchMemoriesRequest {
    #[schemars(description = "Search query to match against message content")]
    query: String,
    #[schemars(description = "Maximum results to return (default: 20)")]
    limit: Option<i64>,
    #[schemars(description = "Filter by session ID")]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetRecentMessagesRequest {
    #[schemars(description = "The session ID to get messages from")]
    session_id: String,
    #[schemars(description = "Maximum messages to return (default: 20)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListOperationsRequest {
    #[schemars(description = "Filter by session ID")]
    session_id: Option<String>,
    #[schemars(description = "Filter by status: 'pending', 'started', 'completed', 'failed'")]
    status: Option<String>,
    #[schemars(description = "Maximum results to return (default: 20)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetToolUsageRequest {
    #[schemars(description = "Maximum results to return (default: 20)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindCochangeRequest {
    #[schemars(description = "File path to find co-change patterns for")]
    file_path: String,
    #[schemars(description = "Maximum patterns to return (default: 10)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct QueryRequest {
    #[schemars(description = "SQL SELECT query to execute")]
    sql: String,
    #[schemars(description = "Maximum rows to return (default: 100)")]
    limit: Option<i64>,
}

// === Memory Tool Request Types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct RememberRequest {
    #[schemars(description = "The fact, decision, or preference to remember")]
    content: String,
    #[schemars(description = "Category: 'fact', 'decision', 'preference', 'context'")]
    category: Option<String>,
    #[schemars(description = "Optional project context")]
    project: Option<String>,
    #[schemars(description = "Tags for organization (comma-separated)")]
    tags: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallRequest {
    #[schemars(description = "What to search for in memories")]
    query: String,
    #[schemars(description = "Maximum results (default: 10)")]
    limit: Option<i64>,
    #[schemars(description = "Filter by category")]
    category: Option<String>,
    #[schemars(description = "Filter by project")]
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ForgetRequest {
    #[schemars(description = "ID of the memory to forget")]
    memory_id: String,
}

// === Code Intelligence Request Types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct AnalyzeFileRequest {
    #[schemars(description = "Path to the file to analyze")]
    file_path: String,
    #[schemars(description = "Project ID for context")]
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetCallGraphRequest {
    #[schemars(description = "Function or method name to trace")]
    function_name: String,
    #[schemars(description = "How many levels deep to trace (default: 2)")]
    depth: Option<i32>,
    #[schemars(description = "Project ID for context")]
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetRelatedFilesRequest {
    #[schemars(description = "File path to find related files for")]
    file_path: String,
    #[schemars(description = "Maximum results (default: 10)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetSymbolsRequest {
    #[schemars(description = "File path to get symbols from")]
    file_path: String,
    #[schemars(description = "Filter by symbol type: 'function', 'class', 'struct', 'const', 'type'")]
    symbol_type: Option<String>,
}

// === Git Intelligence Request Types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct GetFileExpertsRequest {
    #[schemars(description = "File path or pattern to find experts for")]
    file_path: String,
    #[schemars(description = "Maximum experts to return (default: 5)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindSimilarFixesRequest {
    #[schemars(description = "Error message or description to find fixes for")]
    error: String,
    #[schemars(description = "File path for context")]
    file_path: Option<String>,
    #[schemars(description = "Maximum results (default: 5)")]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetChangeRiskRequest {
    #[schemars(description = "File path to assess risk for")]
    file_path: String,
}

// === Project Context Request Types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct GetProjectContextRequest {
    #[schemars(description = "Project ID or path")]
    project: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetGuidelinesRequest {
    #[schemars(description = "Project ID or path (optional - returns global if not specified)")]
    project: Option<String>,
    #[schemars(description = "Category filter: 'style', 'architecture', 'testing', 'naming'")]
    category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddGuidelineRequest {
    #[schemars(description = "The guideline or convention to add")]
    content: String,
    #[schemars(description = "Category: 'style', 'architecture', 'testing', 'naming', 'other'")]
    category: String,
    #[schemars(description = "Project ID or path (optional - global if not specified)")]
    project: Option<String>,
}

// === Mira MCP Server ===

#[derive(Clone)]
pub struct MiraServer {
    db: Arc<SqlitePool>,
    tool_router: ToolRouter<Self>,
}

impl MiraServer {
    pub async fn new(database_url: &str) -> Result<Self> {
        info!("Connecting to database: {}", database_url);
        let db = SqlitePool::connect(database_url).await?;
        info!("Database connected successfully");
        Ok(Self {
            db: Arc::new(db),
            tool_router: Self::tool_router(),
        })
    }
}

#[tool_router]
impl MiraServer {
    /// List chat sessions with optional filters
    #[tool(description = "List chat sessions. Returns session ID, name, type, project path, message count, and last activity time.")]
    async fn list_sessions(
        &self,
        Parameters(req): Parameters<ListSessionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);

        let query = r#"
            SELECT id, name, session_type, project_path, message_count,
                   datetime(last_active, 'unixepoch', 'localtime') as last_active,
                   status
            FROM chat_sessions
            WHERE ($1 IS NULL OR session_type = $1)
              AND ($2 IS NULL OR status = $2)
            ORDER BY last_active DESC
            LIMIT $3
        "#;

        let rows = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, i64, String, String)>(query)
            .bind(&req.session_type)
            .bind(&req.status)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let sessions: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, name, session_type, project_path, message_count, last_active, status)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "session_type": session_type,
                    "project_path": project_path,
                    "message_count": message_count,
                    "last_active": last_active,
                    "status": status,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&sessions)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get details for a specific session
    #[tool(description = "Get detailed information about a specific chat session by ID.")]
    async fn get_session(
        &self,
        Parameters(req): Parameters<GetSessionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT id, name, session_type, project_path, message_count,
                   datetime(last_active, 'unixepoch', 'localtime') as last_active,
                   status, branch, last_commit_hash
            FROM chat_sessions
            WHERE id = $1
        "#;

        let row = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, i64, String, String, Option<String>, Option<String>)>(query)
            .bind(&req.session_id)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match row {
            Some((id, name, session_type, project_path, message_count, last_active, status, branch, last_commit)) => {
                let result = serde_json::json!({
                    "id": id,
                    "name": name,
                    "session_type": session_type,
                    "project_path": project_path,
                    "message_count": message_count,
                    "last_active": last_active,
                    "status": status,
                    "branch": branch,
                    "last_commit_hash": last_commit,
                });
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&result).unwrap()
                )]))
            }
            None => Ok(CallToolResult::success(vec![Content::text(
                format!("Session '{}' not found", req.session_id)
            )])),
        }
    }

    /// Search memory entries
    #[tool(description = "Search through memory entries (chat messages) using text matching.")]
    async fn search_memories(
        &self,
        Parameters(req): Parameters<SearchMemoriesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);
        let search_pattern = format!("%{}%", req.query);

        let sql = r#"
            SELECT id, session_id, role, content,
                   datetime(created_at, 'unixepoch', 'localtime') as created_at
            FROM memory_entries
            WHERE content LIKE $1
              AND ($2 IS NULL OR session_id = $2)
            ORDER BY created_at DESC
            LIMIT $3
        "#;

        let rows = sqlx::query_as::<_, (String, String, String, String, String)>(sql)
            .bind(&search_pattern)
            .bind(&req.session_id)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let entries: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, session_id, role, content, created_at)| {
                serde_json::json!({
                    "id": id,
                    "session_id": session_id,
                    "role": role,
                    "content": if content.len() > 500 {
                        format!("{}...", &content[..500])
                    } else {
                        content
                    },
                    "created_at": created_at,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get recent messages from a session
    #[tool(description = "Get the most recent messages from a specific chat session.")]
    async fn get_recent_messages(
        &self,
        Parameters(req): Parameters<GetRecentMessagesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);

        let query = r#"
            SELECT id, role, content,
                   datetime(created_at, 'unixepoch', 'localtime') as created_at
            FROM memory_entries
            WHERE session_id = $1
            ORDER BY created_at DESC
            LIMIT $2
        "#;

        let rows = sqlx::query_as::<_, (String, String, String, String)>(query)
            .bind(&req.session_id)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let messages: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, role, content, created_at)| {
                serde_json::json!({
                    "id": id,
                    "role": role,
                    "content": if content.len() > 1000 {
                        format!("{}...", &content[..1000])
                    } else {
                        content
                    },
                    "created_at": created_at,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&messages)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// List operations
    #[tool(description = "List LLM operations (code generation tasks, queries, etc.) with optional filters.")]
    async fn list_operations(
        &self,
        Parameters(req): Parameters<ListOperationsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);

        let query = r#"
            SELECT id, session_id, kind, status,
                   datetime(created_at, 'unixepoch', 'localtime') as created_at,
                   datetime(started_at, 'unixepoch', 'localtime') as started_at,
                   datetime(completed_at, 'unixepoch', 'localtime') as completed_at
            FROM operations
            WHERE ($1 IS NULL OR session_id = $1)
              AND ($2 IS NULL OR status = $2)
            ORDER BY created_at DESC
            LIMIT $3
        "#;

        let rows = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>)>(query)
            .bind(&req.session_id)
            .bind(&req.status)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let operations: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, session_id, kind, status, created_at, started_at, completed_at)| {
                serde_json::json!({
                    "id": id,
                    "session_id": session_id,
                    "kind": kind,
                    "status": status,
                    "created_at": created_at,
                    "started_at": started_at,
                    "completed_at": completed_at,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&operations)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get budget status
    #[tool(description = "Get current API budget usage showing daily and monthly spending vs limits.")]
    async fn get_budget_status(&self) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT
                COALESCE(SUM(CASE WHEN date(timestamp, 'unixepoch') = date('now') THEN cost_usd ELSE 0 END), 0) as daily_spent,
                COALESCE(SUM(cost_usd), 0) as monthly_spent,
                datetime('now', 'localtime') as last_updated
            FROM budget_tracking
            WHERE date(timestamp, 'unixepoch') >= date('now', 'start of month')
        "#;

        let row = sqlx::query_as::<_, (f64, f64, String)>(query)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Get limits from budget_summary or use defaults
        let limits_query = r#"
            SELECT daily_limit, monthly_limit
            FROM budget_summary
            LIMIT 1
        "#;

        let limits = sqlx::query_as::<_, (f64, f64)>(limits_query)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .unwrap_or((5.0, 150.0));

        let (daily_spent, monthly_spent, last_updated) = row.unwrap_or((0.0, 0.0, "unknown".to_string()));

        let status = serde_json::json!({
            "daily_spent": daily_spent,
            "daily_limit": limits.0,
            "daily_remaining": (limits.0 - daily_spent).max(0.0),
            "monthly_spent": monthly_spent,
            "monthly_limit": limits.1,
            "monthly_remaining": (limits.1 - monthly_spent).max(0.0),
            "last_updated": last_updated,
        });

        let json = serde_json::to_string_pretty(&status)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get cache statistics
    #[tool(description = "Get LLM response cache statistics including hit rate and tokens saved.")]
    async fn get_cache_stats(&self) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT
                COUNT(*) as total_entries,
                COALESCE(SUM(hit_count), 0) as total_hits,
                COALESCE(SUM(hit_count * token_count), 0) as total_tokens_saved
            FROM llm_cache
        "#;

        let row = sqlx::query_as::<_, (i64, i64, i64)>(query)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .unwrap_or((0, 0, 0));

        let (total_entries, total_hits, total_tokens_saved) = row;

        // Calculate hit rate
        let total_requests = total_entries + total_hits;
        let hit_rate = if total_requests > 0 {
            (total_hits as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };

        let stats = serde_json::json!({
            "total_entries": total_entries,
            "total_hits": total_hits,
            "total_tokens_saved": total_tokens_saved,
            "hit_rate_percent": hit_rate,
        });

        let json = serde_json::to_string_pretty(&stats)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Get tool usage statistics
    #[tool(description = "Get statistics about which tools have been executed most frequently.")]
    async fn get_tool_usage(
        &self,
        Parameters(req): Parameters<GetToolUsageRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);

        let query = r#"
            SELECT
                tool_name,
                COUNT(*) as execution_count,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as success_count,
                AVG(duration_ms) as avg_duration_ms
            FROM tool_executions
            GROUP BY tool_name
            ORDER BY execution_count DESC
            LIMIT $1
        "#;

        let rows = sqlx::query_as::<_, (String, i64, i64, f64)>(query)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let stats: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(tool_name, execution_count, success_count, avg_duration_ms)| {
                serde_json::json!({
                    "tool_name": tool_name,
                    "execution_count": execution_count,
                    "success_count": success_count,
                    "avg_duration_ms": avg_duration_ms,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&stats)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Find co-change patterns
    #[tool(description = "Find files that frequently change together with a given file. Useful for understanding code dependencies.")]
    async fn find_cochange_patterns(
        &self,
        Parameters(req): Parameters<FindCochangeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);

        let query = r#"
            SELECT file_a, file_b, co_change_count, confidence
            FROM file_cochange_patterns
            WHERE file_a = $1 OR file_b = $1
            ORDER BY confidence DESC
            LIMIT $2
        "#;

        let rows = sqlx::query_as::<_, (String, String, i64, f64)>(query)
            .bind(&req.file_path)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let patterns: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(file_a, file_b, co_change_count, confidence)| {
                serde_json::json!({
                    "file_a": file_a,
                    "file_b": file_b,
                    "co_change_count": co_change_count,
                    "confidence": confidence,
                })
            })
            .collect();

        if patterns.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                format!("No co-change patterns found for '{}'", req.file_path)
            )]))
        } else {
            let json = serde_json::to_string_pretty(&patterns)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
    }

    /// List database tables
    #[tool(description = "List all tables in the Mira database with row counts.")]
    async fn list_tables(&self) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT name FROM sqlite_master
            WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%'
            ORDER BY name
        "#;

        let tables: Vec<String> = sqlx::query_scalar(query)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Get row counts for each table
        let mut results = Vec::new();
        for table in &tables {
            let count_query = format!("SELECT COUNT(*) FROM \"{}\"", table);
            let count: i64 = sqlx::query_scalar(&count_query)
                .fetch_one(self.db.as_ref())
                .await
                .unwrap_or(0);
            results.push(serde_json::json!({
                "table": table,
                "row_count": count
            }));
        }

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Execute a read-only SQL query
    #[tool(description = "Execute a read-only SQL query against the Mira database. Only SELECT statements are allowed.")]
    async fn query(
        &self,
        Parameters(req): Parameters<QueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Security: Only allow SELECT statements
        let sql_upper = req.sql.trim().to_uppercase();
        if !sql_upper.starts_with("SELECT") {
            return Ok(CallToolResult::error(vec![Content::text(
                "Only SELECT queries are allowed for safety"
            )]));
        }

        // Prevent dangerous operations even in SELECT
        let forbidden = ["DROP", "DELETE", "INSERT", "UPDATE", "ALTER", "CREATE", "TRUNCATE", "EXEC", "EXECUTE"];
        for word in forbidden {
            if sql_upper.contains(word) {
                return Ok(CallToolResult::error(vec![Content::text(
                    format!("Query contains forbidden keyword: {}", word)
                )]));
            }
        }

        let limit = req.limit.unwrap_or(100);

        // Add LIMIT if not present
        let final_sql = if sql_upper.contains("LIMIT") {
            req.sql
        } else {
            format!("{} LIMIT {}", req.sql, limit)
        };

        // Execute query and return results as JSON
        let rows = sqlx::query(&final_sql)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(format!("Query error: {}", e), None))?;

        let result = serde_json::json!({
            "query": final_sql,
            "row_count": rows.len(),
            "message": format!("Query returned {} rows", rows.len())
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap()
        )]))
    }

    // ========================================================================
    // Memory Tools - Persistent knowledge across sessions
    // ========================================================================

    /// Store a fact, decision, or preference to remember
    #[tool(description = "Remember a fact, decision, preference, or context for future sessions. Use this to store important information that should persist.")]
    async fn remember(
        &self,
        Parameters(req): Parameters<RememberRequest>,
    ) -> Result<CallToolResult, McpError> {
        let id = uuid::Uuid::new_v4().to_string();
        let category = req.category.unwrap_or_else(|| "fact".to_string());
        let now = Utc::now().timestamp();
        let tags_json = req.tags.as_ref()
            .map(|t| serde_json::to_string(&t.split(',').map(|s| s.trim()).collect::<Vec<_>>()).unwrap())
            .unwrap_or_else(|| "[]".to_string());

        sqlx::query(r#"
            INSERT INTO memory_facts (id, content, category, project, tags, created_at, updated_at, access_count)
            VALUES ($1, $2, $3, $4, $5, $6, $6, 0)
        "#)
        .bind(&id)
        .bind(&req.content)
        .bind(&category)
        .bind(&req.project)
        .bind(&tags_json)
        .bind(now)
        .execute(self.db.as_ref())
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = serde_json::json!({
            "id": id,
            "status": "remembered",
            "category": category,
            "content": req.content,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap()
        )]))
    }

    /// Search and recall stored memories
    #[tool(description = "Search through stored memories (facts, decisions, preferences). Returns relevant memories matching the query.")]
    async fn recall(
        &self,
        Parameters(req): Parameters<RecallRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);
        let search_pattern = format!("%{}%", req.query);

        let query = r#"
            SELECT id, content, category, project, tags,
                   datetime(created_at, 'unixepoch', 'localtime') as created_at,
                   access_count
            FROM memory_facts
            WHERE content LIKE $1
              AND ($2 IS NULL OR category = $2)
              AND ($3 IS NULL OR project = $3)
            ORDER BY access_count DESC, created_at DESC
            LIMIT $4
        "#;

        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, String, String, i64)>(query)
            .bind(&search_pattern)
            .bind(&req.category)
            .bind(&req.project)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Update access counts for returned memories
        for (id, _, _, _, _, _, _) in &rows {
            let _ = sqlx::query("UPDATE memory_facts SET access_count = access_count + 1 WHERE id = $1")
                .bind(id)
                .execute(self.db.as_ref())
                .await;
        }

        let memories: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, content, category, project, tags, created_at, access_count)| {
                serde_json::json!({
                    "id": id,
                    "content": content,
                    "category": category,
                    "project": project,
                    "tags": serde_json::from_str::<Vec<String>>(&tags).unwrap_or_default(),
                    "created_at": created_at,
                    "access_count": access_count,
                })
            })
            .collect();

        if memories.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                format!("No memories found matching '{}'", req.query)
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&memories).unwrap()
            )]))
        }
    }

    /// Remove a stored memory
    #[tool(description = "Forget (delete) a stored memory by its ID.")]
    async fn forget(
        &self,
        Parameters(req): Parameters<ForgetRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = sqlx::query("DELETE FROM memory_facts WHERE id = $1")
            .bind(&req.memory_id)
            .execute(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if result.rows_affected() > 0 {
            Ok(CallToolResult::success(vec![Content::text(
                format!("Memory '{}' has been forgotten", req.memory_id)
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                format!("Memory '{}' not found", req.memory_id)
            )]))
        }
    }

    // ========================================================================
    // Code Intelligence Tools
    // ========================================================================

    /// Get symbols from a file
    #[tool(description = "Get all symbols (functions, classes, structs, etc.) defined in a file.")]
    async fn get_symbols(
        &self,
        Parameters(req): Parameters<GetSymbolsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT ce.id, ce.name, ce.element_type, ce.file_path, ce.start_line, ce.end_line
            FROM code_elements ce
            WHERE ce.file_path LIKE $1
              AND ($2 IS NULL OR ce.element_type = $2)
            ORDER BY ce.start_line
        "#;

        let file_pattern = format!("%{}", req.file_path);
        let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, Option<i64>, Option<i64>)>(query)
            .bind(&file_pattern)
            .bind(&req.symbol_type)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let symbols: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, name, element_type, file_path, start_line, end_line)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "type": element_type,
                    "file": file_path,
                    "start_line": start_line,
                    "end_line": end_line,
                })
            })
            .collect();

        if symbols.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                format!("No symbols found in '{}'", req.file_path)
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&symbols).unwrap()
            )]))
        }
    }

    /// Get call graph for a function
    #[tool(description = "Get the call graph showing what functions call and are called by a given function.")]
    async fn get_call_graph(
        &self,
        Parameters(req): Parameters<GetCallGraphRequest>,
    ) -> Result<CallToolResult, McpError> {
        let depth = req.depth.unwrap_or(2);

        // Get callers (functions that call this function)
        let callers_query = r#"
            SELECT DISTINCT ce.name, ce.file_path, ce.element_type
            FROM call_graph cg
            JOIN code_elements ce ON cg.caller_id = ce.id
            JOIN code_elements callee ON cg.callee_id = callee.id
            WHERE callee.name = $1
              AND ($2 IS NULL OR ce.project_id = $2)
            LIMIT 20
        "#;

        let callers = sqlx::query_as::<_, (String, Option<String>, String)>(callers_query)
            .bind(&req.function_name)
            .bind(&req.project_id)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Get callees (functions that this function calls)
        let callees_query = r#"
            SELECT DISTINCT ce.name, ce.file_path, ce.element_type
            FROM call_graph cg
            JOIN code_elements ce ON cg.callee_id = ce.id
            JOIN code_elements caller ON cg.caller_id = caller.id
            WHERE caller.name = $1
              AND ($2 IS NULL OR ce.project_id = $2)
            LIMIT 20
        "#;

        let callees = sqlx::query_as::<_, (String, Option<String>, String)>(callees_query)
            .bind(&req.function_name)
            .bind(&req.project_id)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = serde_json::json!({
            "function": req.function_name,
            "depth": depth,
            "called_by": callers.iter().map(|(name, file, typ)| serde_json::json!({
                "name": name,
                "file": file,
                "type": typ,
            })).collect::<Vec<_>>(),
            "calls": callees.iter().map(|(name, file, typ)| serde_json::json!({
                "name": name,
                "file": file,
                "type": typ,
            })).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap()
        )]))
    }

    /// Get files related to a given file
    #[tool(description = "Find files that are related to a given file through imports, semantic similarity, or co-change patterns.")]
    async fn get_related_files(
        &self,
        Parameters(req): Parameters<GetRelatedFilesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);

        // Get files from semantic edges (imports, references)
        let semantic_query = r#"
            SELECT DISTINCT
                CASE WHEN sn1.file_path = $1 THEN sn2.file_path ELSE sn1.file_path END as related_file,
                se.edge_type,
                se.weight
            FROM semantic_edges se
            JOIN semantic_nodes sn1 ON se.source_id = sn1.id
            JOIN semantic_nodes sn2 ON se.target_id = sn2.id
            WHERE sn1.file_path = $1 OR sn2.file_path = $1
            ORDER BY se.weight DESC
            LIMIT $2
        "#;

        let semantic_rows = sqlx::query_as::<_, (Option<String>, String, f64)>(semantic_query)
            .bind(&req.file_path)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .unwrap_or_default();

        // Get co-change patterns
        let cochange_query = r#"
            SELECT
                CASE WHEN file_a = $1 THEN file_b ELSE file_a END as related_file,
                co_change_count,
                confidence
            FROM file_cochange_patterns
            WHERE file_a = $1 OR file_b = $1
            ORDER BY confidence DESC
            LIMIT $2
        "#;

        let cochange_rows = sqlx::query_as::<_, (String, i64, f64)>(cochange_query)
            .bind(&req.file_path)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .unwrap_or_default();

        let result = serde_json::json!({
            "file": req.file_path,
            "semantic_relations": semantic_rows.iter().map(|(file, edge_type, weight)| serde_json::json!({
                "file": file,
                "relation": edge_type,
                "strength": weight,
            })).collect::<Vec<_>>(),
            "cochange_patterns": cochange_rows.iter().map(|(file, count, conf)| serde_json::json!({
                "file": file,
                "co_change_count": count,
                "confidence": conf,
            })).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap()
        )]))
    }

    // ========================================================================
    // Git Intelligence Tools
    // ========================================================================

    /// Get experts for a file based on git history
    #[tool(description = "Find the developers with the most expertise on a file based on git commit history.")]
    async fn get_file_experts(
        &self,
        Parameters(req): Parameters<GetFileExpertsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(5);

        let query = r#"
            SELECT author_name, author_email, file_pattern, commit_count, expertise_score,
                   datetime(last_commit_date, 'unixepoch', 'localtime') as last_commit
            FROM author_expertise
            WHERE file_pattern LIKE $1
            ORDER BY expertise_score DESC
            LIMIT $2
        "#;

        let file_pattern = format!("%{}%", req.file_path);
        let rows = sqlx::query_as::<_, (String, String, String, i64, f64, String)>(query)
            .bind(&file_pattern)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let experts: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(name, email, pattern, commits, score, last_commit)| {
                serde_json::json!({
                    "author": name,
                    "email": email,
                    "file_pattern": pattern,
                    "commit_count": commits,
                    "expertise_score": score,
                    "last_commit": last_commit,
                })
            })
            .collect();

        if experts.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                format!("No expertise data found for '{}'", req.file_path)
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&experts).unwrap()
            )]))
        }
    }

    /// Find similar historical fixes
    #[tool(description = "Search for similar errors/issues that were fixed in the past and what fixed them.")]
    async fn find_similar_fixes(
        &self,
        Parameters(req): Parameters<FindSimilarFixesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(5);
        let error_pattern = format!("%{}%", req.error);

        let query = r#"
            SELECT id, error_type, error_message, fix_description, file_path, commit_hash,
                   datetime(fixed_at, 'unixepoch', 'localtime') as fixed_at
            FROM historical_fixes
            WHERE error_message LIKE $1
              AND ($2 IS NULL OR file_path LIKE $2)
            ORDER BY fixed_at DESC
            LIMIT $3
        "#;

        let file_pattern = req.file_path.as_ref().map(|f| format!("%{}%", f));
        let rows = sqlx::query_as::<_, (i64, String, String, String, Option<String>, Option<String>, String)>(query)
            .bind(&error_pattern)
            .bind(&file_pattern)
            .bind(limit)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let fixes: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, error_type, error_msg, fix_desc, file, commit, fixed_at)| {
                serde_json::json!({
                    "id": id,
                    "error_type": error_type,
                    "error_message": error_msg,
                    "fix_description": fix_desc,
                    "file": file,
                    "commit": commit,
                    "fixed_at": fixed_at,
                })
            })
            .collect();

        if fixes.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                format!("No similar fixes found for: {}", req.error)
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&fixes).unwrap()
            )]))
        }
    }

    /// Assess change risk for a file
    #[tool(description = "Get a risk assessment for changing a file based on its complexity, change history, and dependencies.")]
    async fn get_change_risk(
        &self,
        Parameters(req): Parameters<GetChangeRiskRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Get change frequency
        let change_query = r#"
            SELECT COUNT(*) as change_count
            FROM file_cochange_patterns
            WHERE file_a = $1 OR file_b = $1
        "#;

        let (change_count,): (i64,) = sqlx::query_as(change_query)
            .bind(&req.file_path)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .unwrap_or((0,));

        // Get number of dependencies (callers/callees)
        let dep_query = r#"
            SELECT COUNT(DISTINCT cg.id) as dep_count
            FROM call_graph cg
            JOIN code_elements ce ON cg.caller_id = ce.id OR cg.callee_id = ce.id
            WHERE ce.file_path LIKE $1
        "#;

        let file_pattern = format!("%{}", req.file_path);
        let (dep_count,): (i64,) = sqlx::query_as(dep_query)
            .bind(&file_pattern)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .unwrap_or((0,));

        // Get number of symbols (complexity indicator)
        let symbol_query = r#"
            SELECT COUNT(*) as symbol_count
            FROM code_elements
            WHERE file_path LIKE $1
        "#;

        let (symbol_count,): (i64,) = sqlx::query_as(symbol_query)
            .bind(&file_pattern)
            .fetch_optional(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .unwrap_or((0,));

        // Calculate risk score (0-100)
        let change_risk = (change_count as f64 / 10.0).min(30.0);
        let dep_risk = (dep_count as f64 / 5.0).min(40.0);
        let complexity_risk = (symbol_count as f64 / 20.0).min(30.0);
        let total_risk = (change_risk + dep_risk + complexity_risk).min(100.0);

        let risk_level = if total_risk < 30.0 {
            "low"
        } else if total_risk < 60.0 {
            "medium"
        } else {
            "high"
        };

        let result = serde_json::json!({
            "file": req.file_path,
            "risk_score": total_risk.round(),
            "risk_level": risk_level,
            "factors": {
                "change_frequency": change_count,
                "dependencies": dep_count,
                "complexity": symbol_count,
            },
            "recommendations": if total_risk > 60.0 {
                vec![
                    "Consider adding tests before making changes",
                    "Review related files that often change together",
                    "Get review from file experts"
                ]
            } else if total_risk > 30.0 {
                vec![
                    "Normal caution advised",
                    "Check for existing tests"
                ]
            } else {
                vec!["Low risk - standard development practices apply"]
            },
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap()
        )]))
    }

    // ========================================================================
    // Project Context Tools
    // ========================================================================

    /// Get project guidelines
    #[tool(description = "Get coding guidelines and conventions for a project.")]
    async fn get_guidelines(
        &self,
        Parameters(req): Parameters<GetGuidelinesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let query = r#"
            SELECT id, content, category, project_id,
                   datetime(created_at, 'unixepoch', 'localtime') as created_at
            FROM project_guidelines
            WHERE ($1 IS NULL OR project_id = $1 OR project_id IS NULL)
              AND ($2 IS NULL OR category = $2)
            ORDER BY category, created_at
        "#;

        let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, String)>(query)
            .bind(&req.project)
            .bind(&req.category)
            .fetch_all(self.db.as_ref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let guidelines: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, content, category, project, created_at)| {
                serde_json::json!({
                    "id": id,
                    "content": content,
                    "category": category,
                    "project": project,
                    "created_at": created_at,
                })
            })
            .collect();

        if guidelines.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "No guidelines found. Use 'add_guideline' to add project conventions."
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&guidelines).unwrap()
            )]))
        }
    }

    /// Add a project guideline
    #[tool(description = "Add a coding guideline or convention for a project. These are persisted and can be recalled later.")]
    async fn add_guideline(
        &self,
        Parameters(req): Parameters<AddGuidelineRequest>,
    ) -> Result<CallToolResult, McpError> {
        let now = Utc::now().timestamp();

        let result = sqlx::query(r#"
            INSERT INTO project_guidelines (content, category, project_id, created_at)
            VALUES ($1, $2, $3, $4)
        "#)
        .bind(&req.content)
        .bind(&req.category)
        .bind(&req.project)
        .bind(now)
        .execute(self.db.as_ref())
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = serde_json::json!({
            "status": "added",
            "id": result.last_insert_rowid(),
            "category": req.category,
            "content": req.content,
            "project": req.project,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap()
        )]))
    }
}

#[tool_handler]
impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Mira Power Suit - Memory and Intelligence Layer for Claude Code. Provides persistent memory (remember/recall/forget), code intelligence (symbols, call graphs, related files), git intelligence (experts, similar fixes, change risk), and project context (guidelines). Use 'remember' to store facts for future sessions, 'recall' to search memories.".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up logging to stderr (stdout is used for MCP protocol)
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Mira MCP Server...");

    // Get database URL from environment
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());

    // Create server
    let server = MiraServer::new(&database_url).await?;
    info!("Server initialized");

    // Run as stdio server
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
