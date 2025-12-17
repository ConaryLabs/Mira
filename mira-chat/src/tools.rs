//! Tool definitions for GPT-5.2 function calling
//!
//! Implements coding assistant tools:
//! - File operations (read, write, edit, glob, grep)
//! - Shell execution
//! - Web search/fetch
//! - Memory (remember, recall)
//!
//! Tools are executed locally, results returned to GPT-5.2

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::responses::Tool;
use crate::semantic::{SemanticSearch, COLLECTION_MEMORY};

/// Diff information for file modifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub path: String,
    pub old_content: Option<String>,
    pub new_content: String,
    pub is_new_file: bool,
}

/// Rich tool result with diff information for file operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichToolResult {
    pub success: bool,
    pub output: String,
    pub diff: Option<DiffInfo>,
}

/// Convert HTML to plain text (basic implementation)
fn html_to_text(html: &str) -> String {
    // Remove script and style tags with their content
    let script_re = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style_re = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let text = script_re.replace_all(html, "");
    let text = style_re.replace_all(&text, "");

    // Replace common block elements with newlines
    let block_re = Regex::new(r"(?i)</?(p|div|br|h[1-6]|li|tr)[^>]*>").unwrap();
    let text = block_re.replace_all(&text, "\n");

    // Remove all remaining HTML tags
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let text = tag_re.replace_all(&text, "");

    // Decode common HTML entities
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Collapse multiple newlines and spaces
    let multi_newline = Regex::new(r"\n{3,}").unwrap();
    let multi_space = Regex::new(r" {2,}").unwrap();
    let text = multi_newline.replace_all(&text, "\n\n");
    let text = multi_space.replace_all(&text, " ");

    text.trim().to_string()
}

use crate::session::SessionManager;

/// Tool executor handles tool invocation and result formatting
pub struct ToolExecutor {
    /// Working directory for file operations
    pub cwd: std::path::PathBuf,
    /// Semantic search client (optional)
    semantic: Option<Arc<SemanticSearch>>,
    /// SQLite database pool (optional)
    db: Option<SqlitePool>,
    /// Session manager for file tracking (optional)
    session: Option<Arc<SessionManager>>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            semantic: None,
            db: None,
            session: None,
        }
    }

    /// Configure with semantic search
    pub fn with_semantic(mut self, semantic: Arc<SemanticSearch>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Configure with database
    pub fn with_db(mut self, db: SqlitePool) -> Self {
        self.db = Some(db);
        self
    }

    /// Configure with session manager for file tracking
    pub fn with_session(mut self, session: Arc<SessionManager>) -> Self {
        self.session = Some(session);
        self
    }

    /// Track a file access (for compaction context)
    fn track_file(&self, path: &str) {
        if let Some(ref session) = self.session {
            session.track_file(path);
        }
    }

    /// Execute a tool by name with JSON arguments
    pub async fn execute(&self, name: &str, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            "read_file" => self.read_file(&args).await,
            "write_file" => self.write_file(&args).await,
            "edit_file" => self.edit_file(&args).await,
            "glob" => self.glob(&args).await,
            "grep" => self.grep(&args).await,
            "bash" => self.bash(&args).await,
            "web_search" => self.web_search(&args).await,
            "web_fetch" => self.web_fetch(&args).await,
            "remember" => self.remember(&args).await,
            "recall" => self.recall(&args).await,
            // Mira power armor tools
            "task" => self.task(&args).await,
            "goal" => self.goal(&args).await,
            "correction" => self.correction(&args).await,
            "store_decision" => self.store_decision(&args).await,
            "record_rejected_approach" => self.record_rejected_approach(&args).await,
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }

    /// Execute a tool and return rich result with diff information
    ///
    /// For write_file and edit_file, captures before/after content for diff display.
    /// Other tools return simple output without diff info.
    pub async fn execute_rich(&self, name: &str, arguments: &str) -> Result<RichToolResult> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            "write_file" => self.write_file_rich(&args).await,
            "edit_file" => self.edit_file_rich(&args).await,
            // All other tools don't produce diffs
            _ => {
                let output = self.execute(name, arguments).await?;
                let success = !output.starts_with("Error") && !output.contains("Error:");
                Ok(RichToolResult {
                    success,
                    output,
                    diff: None,
                })
            }
        }
    }

    /// Write file with diff capture
    async fn write_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        // Check if file exists and read old content
        let old_content = tokio::fs::read_to_string(&full_path).await.ok();
        let is_new_file = old_content.is_none();

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(RichToolResult {
                    success: false,
                    output: format!("Error creating directories for {}: {}", path, e),
                    diff: None,
                });
            }
        }

        // Write the file
        match tokio::fs::write(&full_path, content).await {
            Ok(()) => Ok(RichToolResult {
                success: true,
                output: format!("Wrote {} bytes to {}", content.len(), path),
                diff: Some(DiffInfo {
                    path: path.to_string(),
                    old_content,
                    new_content: content.to_string(),
                    is_new_file,
                }),
            }),
            Err(e) => Ok(RichToolResult {
                success: false,
                output: format!("Error writing {}: {}", path, e),
                diff: None,
            }),
        }
    }

    /// Edit file with diff capture
    async fn edit_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        // Read current content
        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(RichToolResult {
                    success: false,
                    output: format!("Error reading {}: {}", path, e),
                    diff: None,
                });
            }
        };

        // Check if old_string exists
        if !content.contains(old_string) {
            return Ok(RichToolResult {
                success: false,
                output: format!(
                    "Error: old_string not found in {}. Make sure to match exactly including whitespace.",
                    path
                ),
                diff: None,
            });
        }

        // Check for uniqueness if not replace_all
        if !replace_all {
            let count = content.matches(old_string).count();
            if count > 1 {
                return Ok(RichToolResult {
                    success: false,
                    output: format!(
                        "Error: old_string found {} times in {}. Use replace_all=true or provide more context to make it unique.",
                        count, path
                    ),
                    diff: None,
                });
            }
        }

        // Perform replacement
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Write back
        match tokio::fs::write(&full_path, &new_content).await {
            Ok(()) => {
                let old_lines = old_string.lines().count();
                let new_lines = new_string.lines().count();
                Ok(RichToolResult {
                    success: true,
                    output: format!(
                        "Edited {}: replaced {} lines with {} lines",
                        path, old_lines, new_lines
                    ),
                    diff: Some(DiffInfo {
                        path: path.to_string(),
                        old_content: Some(old_string.to_string()),
                        new_content: new_string.to_string(),
                        is_new_file: false,
                    }),
                })
            }
            Err(e) => Ok(RichToolResult {
                success: false,
                output: format!("Error writing {}: {}", path, e),
                diff: None,
            }),
        }
    }

    async fn read_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        // Track file access for compaction context
        self.track_file(&full_path.to_string_lossy());

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading {}: {}", path, e)),
        }
    }

    async fn write_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        // Track file access for compaction context
        self.track_file(&full_path.to_string_lossy());

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        match tokio::fs::write(&full_path, content).await {
            Ok(()) => Ok(format!("Wrote {} bytes to {}", content.len(), path)),
            Err(e) => Ok(format!("Error writing {}: {}", path, e)),
        }
    }

    async fn glob(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let base_path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = base_path.as_ref().unwrap_or(&self.cwd);

        let mut matches = Vec::new();
        let glob_pattern = format!("{}/{}", search_dir.display(), pattern);

        for entry in glob::glob(&glob_pattern)? {
            if let Ok(path) = entry {
                matches.push(path.display().to_string());
            }
        }

        if matches.is_empty() {
            Ok("No matches found".into())
        } else {
            Ok(matches.join("\n"))
        }
    }

    async fn grep(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = path.as_ref().unwrap_or(&self.cwd);

        // Use ripgrep if available, fall back to grep
        let output = tokio::process::Command::new("rg")
            .args(["--line-number", "--no-heading", pattern])
            .current_dir(search_dir)
            .output()
            .await;

        match output {
            Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
            Err(_) => {
                // Fallback to grep
                let output = tokio::process::Command::new("grep")
                    .args(["-rn", pattern, "."])
                    .current_dir(search_dir)
                    .output()
                    .await?;
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
        }
    }

    async fn bash(&self, args: &Value) -> Result<String> {
        let command = args["command"].as_str().unwrap_or("");

        let output = tokio::process::Command::new("bash")
            .args(["-c", command])
            .current_dir(&self.cwd)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(stdout.to_string())
        } else {
            Ok(format!("Exit code: {}\n{}\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            ))
        }
    }

    async fn edit_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        // Track file access for compaction context
        self.track_file(&full_path.to_string_lossy());

        // Read current content
        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return Ok(format!("Error reading {}: {}", path, e)),
        };

        // Check if old_string exists
        if !content.contains(old_string) {
            return Ok(format!(
                "Error: old_string not found in {}. Make sure to match exactly including whitespace.",
                path
            ));
        }

        // Check for uniqueness if not replace_all
        if !replace_all {
            let count = content.matches(old_string).count();
            if count > 1 {
                return Ok(format!(
                    "Error: old_string found {} times in {}. Use replace_all=true or provide more context to make it unique.",
                    count, path
                ));
            }
        }

        // Perform replacement
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Write back
        match tokio::fs::write(&full_path, &new_content).await {
            Ok(()) => {
                let old_lines = old_string.lines().count();
                let new_lines = new_string.lines().count();
                Ok(format!(
                    "Edited {}: replaced {} lines with {} lines",
                    path, old_lines, new_lines
                ))
            }
            Err(e) => Ok(format!("Error writing {}: {}", path, e)),
        }
    }

    async fn web_search(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        // Use DuckDuckGo HTML search (no API key required)
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .build()?;

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = client.get(&url).send().await?;
        let html = response.text().await?;

        // Parse results from HTML
        let mut results = Vec::new();
        for (i, chunk) in html.split("result__a").enumerate().skip(1) {
            if i > limit {
                break;
            }
            // Extract href and title
            if let Some(href_start) = chunk.find("href=\"") {
                let href_rest = &chunk[href_start + 6..];
                if let Some(href_end) = href_rest.find('"') {
                    let href = &href_rest[..href_end];
                    // Decode DuckDuckGo redirect URL
                    let actual_url = if href.contains("uddg=") {
                        href.split("uddg=")
                            .nth(1)
                            .and_then(|s| s.split('&').next())
                            .map(|s| urlencoding::decode(s).unwrap_or_default().to_string())
                            .unwrap_or_else(|| href.to_string())
                    } else {
                        href.to_string()
                    };

                    // Extract title (text before </a>)
                    if let Some(title_end) = href_rest.find("</a>") {
                        let title_chunk = &href_rest[href_end + 2..title_end];
                        let title = title_chunk
                            .replace("<b>", "")
                            .replace("</b>", "")
                            .trim()
                            .to_string();
                        if !title.is_empty() && !actual_url.is_empty() {
                            results.push(format!("{}. {} - {}", results.len() + 1, title, actual_url));
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(format!("No results found for: {}", query))
        } else {
            Ok(results.join("\n"))
        }
    }

    async fn web_fetch(&self, args: &Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        let max_length = args["max_length"].as_u64().unwrap_or(10000) as usize;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return Ok(format!("Error fetching {}: {}", url, e)),
        };

        let status = response.status();
        if !status.is_success() {
            return Ok(format!("HTTP {}: {}", status.as_u16(), url));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Only process text content
        if !content_type.contains("text") && !content_type.contains("json") {
            return Ok(format!("Non-text content: {} ({})", content_type, url));
        }

        let body = response.text().await?;
        let is_html = content_type.contains("html");

        // Convert HTML to plain text (basic)
        let text = if is_html {
            html_to_text(&body)
        } else {
            body
        };

        // Truncate if too long
        if text.len() > max_length {
            Ok(format!("{}...\n\n[Truncated, {} total bytes]", &text[..max_length], text.len()))
        } else {
            Ok(text)
        }
    }

    async fn remember(&self, args: &Value) -> Result<String> {
        let content = args["content"].as_str().unwrap_or("");
        let fact_type = args["fact_type"].as_str().unwrap_or("general");
        let category = args["category"].as_str();

        if content.is_empty() {
            return Ok("Error: content is required".into());
        }

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        // Generate key from content (first 50 chars, normalized)
        let key: String = content
            .chars()
            .take(50)
            .collect::<String>()
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
            .trim()
            .to_string();

        // Store in SQLite if available
        if let Some(db) = &self.db {
            let _ = sqlx::query(r#"
                INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, 'mira-chat', 1.0, 0, $6, $6)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    fact_type = excluded.fact_type,
                    category = COALESCE(excluded.category, memory_facts.category),
                    updated_at = excluded.updated_at
            "#)
            .bind(&id)
            .bind(fact_type)
            .bind(&key)
            .bind(content)
            .bind(category)
            .bind(now)
            .execute(db)
            .await;
        }

        // Store in Qdrant for semantic search
        let mut semantic_stored = false;
        if let Some(semantic) = &self.semantic {
            if semantic.is_available() {
                let mut metadata = HashMap::new();
                metadata.insert("fact_type".into(), json!(fact_type));
                metadata.insert("key".into(), json!(key));
                if let Some(cat) = category {
                    metadata.insert("category".into(), json!(cat));
                }

                if let Err(e) = semantic.store(COLLECTION_MEMORY, &id, content, metadata).await {
                    tracing::warn!("Failed to store in Qdrant: {}", e);
                } else {
                    semantic_stored = true;
                }
            }
        }

        Ok(json!({
            "status": "remembered",
            "key": key,
            "fact_type": fact_type,
            "category": category,
            "semantic_search": semantic_stored,
        }).to_string())
    }

    async fn recall(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let fact_type = args["fact_type"].as_str();

        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        // Try semantic search first
        if let Some(semantic) = &self.semantic {
            if semantic.is_available() {
                // Build filter for fact_type if specified
                let filter = fact_type.map(|ft| {
                    qdrant_client::qdrant::Filter::must([
                        qdrant_client::qdrant::Condition::matches("fact_type", ft.to_string())
                    ])
                });

                match semantic.search(COLLECTION_MEMORY, query, limit, filter).await {
                    Ok(results) if !results.is_empty() => {
                        let items: Vec<Value> = results.iter().map(|r| {
                            json!({
                                "content": r.content,
                                "score": r.score,
                                "search_type": "semantic",
                                "fact_type": r.metadata.get("fact_type"),
                                "category": r.metadata.get("category"),
                            })
                        }).collect();

                        return Ok(json!({
                            "results": items,
                            "search_type": "semantic",
                            "count": items.len(),
                        }).to_string());
                    }
                    Ok(_) => {
                        // Fall through to SQLite
                    }
                    Err(e) => {
                        tracing::warn!("Semantic search failed: {}", e);
                    }
                }
            }
        }

        // Fallback to SQLite text search
        if let Some(db) = &self.db {
            let pattern = format!("%{}%", query);

            let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
                r#"
                SELECT fact_type, key, value, category
                FROM memory_facts
                WHERE value LIKE $1 OR key LIKE $1 OR category LIKE $1
                ORDER BY times_used DESC, updated_at DESC
                LIMIT $2
                "#
            )
            .bind(&pattern)
            .bind(limit as i64)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if !rows.is_empty() {
                let items: Vec<Value> = rows.iter().map(|(ft, key, value, cat)| {
                    json!({
                        "content": value,
                        "search_type": "text",
                        "fact_type": ft,
                        "key": key,
                        "category": cat,
                    })
                }).collect();

                return Ok(json!({
                    "results": items,
                    "search_type": "text",
                    "count": items.len(),
                }).to_string());
            }
        }

        Ok(json!({
            "results": [],
            "search_type": "none",
            "count": 0,
            "message": "No memories found matching query",
        }).to_string())
    }

    // ========================================================================
    // Mira Power Armor Tools
    // ========================================================================

    /// Task management - create, list, update, complete tasks
    async fn task(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }
                let description = args["description"].as_str();
                let priority = args["priority"].as_str().unwrap_or("medium");
                let parent_id = args["parent_id"].as_str();

                let id = Uuid::new_v4().to_string();

                let _ = sqlx::query(r#"
                    INSERT INTO tasks (id, parent_id, title, description, status, priority, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, 'pending', $5, $6, $6)
                "#)
                .bind(&id)
                .bind(parent_id)
                .bind(title)
                .bind(description)
                .bind(priority)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "created",
                    "task_id": id,
                    "title": title,
                    "priority": priority,
                }).to_string())
            }

            "list" => {
                let include_completed = args["include_completed"].as_bool().unwrap_or(false);
                let limit = args["limit"].as_i64().unwrap_or(20);

                let rows: Vec<(String, Option<String>, String, Option<String>, String, String, String, String)> = sqlx::query_as(r#"
                    SELECT id, parent_id, title, description, status, priority,
                           datetime(created_at, 'unixepoch', 'localtime') as created_at,
                           datetime(updated_at, 'unixepoch', 'localtime') as updated_at
                    FROM tasks
                    WHERE ($1 = 1 OR status != 'completed')
                    ORDER BY
                        CASE status WHEN 'in_progress' THEN 0 WHEN 'blocked' THEN 1 WHEN 'pending' THEN 2 ELSE 3 END,
                        CASE priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END,
                        created_at DESC
                    LIMIT $2
                "#)
                .bind(if include_completed { 1 } else { 0 })
                .bind(limit)
                .fetch_all(db)
                .await
                .unwrap_or_default();

                let tasks: Vec<Value> = rows.into_iter().map(|(id, parent_id, title, desc, status, priority, created, updated)| {
                    json!({
                        "id": id,
                        "parent_id": parent_id,
                        "title": title,
                        "description": desc,
                        "status": status,
                        "priority": priority,
                        "created_at": created,
                        "updated_at": updated,
                    })
                }).collect();

                Ok(json!({
                    "tasks": tasks,
                    "count": tasks.len(),
                }).to_string())
            }

            "update" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                let _ = sqlx::query(r#"
                    UPDATE tasks
                    SET updated_at = $1,
                        title = COALESCE($2, title),
                        description = COALESCE($3, description),
                        status = COALESCE($4, status),
                        priority = COALESCE($5, priority)
                    WHERE id = $6 OR id LIKE $7
                "#)
                .bind(now)
                .bind(args["title"].as_str())
                .bind(args["description"].as_str())
                .bind(args["status"].as_str())
                .bind(args["priority"].as_str())
                .bind(task_id)
                .bind(format!("{}%", task_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "updated",
                    "task_id": task_id,
                }).to_string())
            }

            "complete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }
                let notes = args["notes"].as_str();

                let _ = sqlx::query(r#"
                    UPDATE tasks
                    SET status = 'completed', completed_at = $1, updated_at = $1, completion_notes = $2
                    WHERE id = $3 OR id LIKE $4
                "#)
                .bind(now)
                .bind(notes)
                .bind(task_id)
                .bind(format!("{}%", task_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "completed",
                    "task_id": task_id,
                    "completed_at": Utc::now().to_rfc3339(),
                }).to_string())
            }

            "delete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                let _ = sqlx::query("DELETE FROM tasks WHERE id = $1 OR id LIKE $2")
                    .bind(task_id)
                    .bind(format!("{}%", task_id))
                    .execute(db)
                    .await;

                Ok(json!({
                    "status": "deleted",
                    "task_id": task_id,
                }).to_string())
            }

            _ => Ok(format!("Unknown action: {}. Use create/list/update/complete/delete", action)),
        }
    }

    /// Goal management - create, list, update goals with milestones
    async fn goal(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }
                let description = args["description"].as_str();
                let priority = args["priority"].as_str().unwrap_or("medium");
                let success_criteria = args["success_criteria"].as_str();

                let id = format!("goal-{}", &Uuid::new_v4().to_string()[..8]);

                // Get project_id from cwd
                let project_path = self.cwd.to_string_lossy().to_string();
                let project_id: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
                    .bind(&project_path)
                    .fetch_optional(db)
                    .await
                    .ok()
                    .flatten();

                let _ = sqlx::query(r#"
                    INSERT INTO goals (id, title, description, success_criteria, status, priority, project_id, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, 'planning', $5, $6, $7, $7)
                "#)
                .bind(&id)
                .bind(title)
                .bind(description)
                .bind(success_criteria)
                .bind(priority)
                .bind(project_id)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "created",
                    "goal_id": id,
                    "title": title,
                    "priority": priority,
                }).to_string())
            }

            "list" => {
                let include_finished = args["include_finished"].as_bool().unwrap_or(false);
                let limit = args["limit"].as_i64().unwrap_or(10);

                let rows: Vec<(String, String, Option<String>, String, String, i32, String)> = if include_finished {
                    sqlx::query_as(r#"
                        SELECT id, title, description, status, priority, progress_percent,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM goals
                        ORDER BY
                            CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 ELSE 4 END,
                            CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                            updated_at DESC
                        LIMIT $1
                    "#)
                    .bind(limit)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                } else {
                    sqlx::query_as(r#"
                        SELECT id, title, description, status, priority, progress_percent,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM goals
                        WHERE status IN ('planning', 'in_progress', 'blocked')
                        ORDER BY
                            CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 END,
                            CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                            updated_at DESC
                        LIMIT $1
                    "#)
                    .bind(limit)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                };

                let goals: Vec<Value> = rows.into_iter().map(|(id, title, desc, status, priority, progress, updated)| {
                    json!({
                        "id": id,
                        "title": title,
                        "description": desc,
                        "status": status,
                        "priority": priority,
                        "progress_percent": progress,
                        "updated_at": updated,
                    })
                }).collect();

                Ok(json!({
                    "goals": goals,
                    "count": goals.len(),
                }).to_string())
            }

            "update" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                if goal_id.is_empty() {
                    return Ok("Error: goal_id is required".into());
                }

                let _ = sqlx::query(r#"
                    UPDATE goals
                    SET updated_at = $1,
                        title = COALESCE($2, title),
                        description = COALESCE($3, description),
                        status = COALESCE($4, status),
                        priority = COALESCE($5, priority),
                        progress_percent = COALESCE($6, progress_percent)
                    WHERE id = $7 OR id LIKE $8
                "#)
                .bind(now)
                .bind(args["title"].as_str())
                .bind(args["description"].as_str())
                .bind(args["status"].as_str())
                .bind(args["priority"].as_str())
                .bind(args["progress_percent"].as_i64().map(|v| v as i32))
                .bind(goal_id)
                .bind(format!("{}%", goal_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "updated",
                    "goal_id": goal_id,
                }).to_string())
            }

            "add_milestone" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                let title = args["title"].as_str().unwrap_or("");
                if goal_id.is_empty() || title.is_empty() {
                    return Ok("Error: goal_id and title are required".into());
                }

                let id = Uuid::new_v4().to_string();
                let weight = args["weight"].as_i64().unwrap_or(1) as i32;

                let _ = sqlx::query(r#"
                    INSERT INTO milestones (id, goal_id, title, description, weight, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $6)
                "#)
                .bind(&id)
                .bind(goal_id)
                .bind(title)
                .bind(args["description"].as_str())
                .bind(weight)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "added",
                    "milestone_id": id,
                    "goal_id": goal_id,
                    "title": title,
                }).to_string())
            }

            "complete_milestone" => {
                let milestone_id = args["milestone_id"].as_str().unwrap_or("");
                if milestone_id.is_empty() {
                    return Ok("Error: milestone_id is required".into());
                }

                let _ = sqlx::query(r#"
                    UPDATE milestones
                    SET status = 'completed', completed_at = $1, updated_at = $1
                    WHERE id = $2 OR id LIKE $3
                "#)
                .bind(now)
                .bind(milestone_id)
                .bind(format!("{}%", milestone_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "completed",
                    "milestone_id": milestone_id,
                }).to_string())
            }

            _ => Ok(format!("Unknown action: {}. Use create/list/update/add_milestone/complete_milestone", action)),
        }
    }

    /// Correction management - record when user corrects the assistant
    async fn correction(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("record");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "record" => {
                let what_was_wrong = args["what_was_wrong"].as_str().unwrap_or("");
                let what_is_right = args["what_is_right"].as_str().unwrap_or("");
                if what_was_wrong.is_empty() || what_is_right.is_empty() {
                    return Ok("Error: what_was_wrong and what_is_right are required".into());
                }

                let correction_type = args["correction_type"].as_str().unwrap_or("approach");
                let rationale = args["rationale"].as_str();
                let scope = args["scope"].as_str().unwrap_or("project");
                let keywords = args["keywords"].as_str();

                let id = Uuid::new_v4().to_string();

                // Get project_id
                let project_path = self.cwd.to_string_lossy().to_string();
                let project_id: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
                    .bind(&project_path)
                    .fetch_optional(db)
                    .await
                    .ok()
                    .flatten();

                let _ = sqlx::query(r#"
                    INSERT INTO corrections (id, correction_type, what_was_wrong, what_is_right, rationale, scope, project_id, keywords, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
                "#)
                .bind(&id)
                .bind(correction_type)
                .bind(what_was_wrong)
                .bind(what_is_right)
                .bind(rationale)
                .bind(scope)
                .bind(project_id)
                .bind(keywords)
                .bind(now)
                .execute(db)
                .await;

                // Store in Qdrant for semantic matching
                if let Some(semantic) = &self.semantic {
                    if semantic.is_available() {
                        let content = format!(
                            "Correction: {} -> {}. Rationale: {}",
                            what_was_wrong, what_is_right, rationale.unwrap_or("")
                        );
                        let mut metadata = HashMap::new();
                        metadata.insert("type".into(), json!("correction"));
                        metadata.insert("correction_type".into(), json!(correction_type));
                        metadata.insert("scope".into(), json!(scope));
                        metadata.insert("id".into(), json!(id));

                        let _ = semantic.store(COLLECTION_MEMORY, &id, &content, metadata).await;
                    }
                }

                Ok(json!({
                    "status": "recorded",
                    "correction_id": id,
                    "correction_type": correction_type,
                    "scope": scope,
                }).to_string())
            }

            "list" => {
                let limit = args["limit"].as_i64().unwrap_or(10);
                let correction_type = args["correction_type"].as_str();

                let rows: Vec<(String, String, String, String, Option<String>, String, f64, i64)> = sqlx::query_as(r#"
                    SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope, confidence, times_applied
                    FROM corrections
                    WHERE status = 'active'
                      AND ($1 IS NULL OR correction_type = $1)
                    ORDER BY confidence DESC, times_validated DESC
                    LIMIT $2
                "#)
                .bind(correction_type)
                .bind(limit)
                .fetch_all(db)
                .await
                .unwrap_or_default();

                let corrections: Vec<Value> = rows.into_iter().map(|(id, ctype, wrong, right, rationale, scope, confidence, applied)| {
                    json!({
                        "id": id,
                        "correction_type": ctype,
                        "what_was_wrong": wrong,
                        "what_is_right": right,
                        "rationale": rationale,
                        "scope": scope,
                        "confidence": confidence,
                        "times_applied": applied,
                    })
                }).collect();

                Ok(json!({
                    "corrections": corrections,
                    "count": corrections.len(),
                }).to_string())
            }

            "validate" => {
                let correction_id = args["correction_id"].as_str().unwrap_or("");
                let outcome = args["outcome"].as_str().unwrap_or("validated");

                if correction_id.is_empty() {
                    return Ok("Error: correction_id is required".into());
                }

                match outcome {
                    "validated" => {
                        let _ = sqlx::query(r#"
                            UPDATE corrections
                            SET times_validated = times_validated + 1,
                                confidence = MIN(1.0, confidence + 0.05),
                                updated_at = $1
                            WHERE id = $2 OR id LIKE $3
                        "#)
                        .bind(now)
                        .bind(correction_id)
                        .bind(format!("{}%", correction_id))
                        .execute(db)
                        .await;
                    }
                    "deprecated" => {
                        let _ = sqlx::query(r#"
                            UPDATE corrections SET status = 'deprecated', updated_at = $1
                            WHERE id = $2 OR id LIKE $3
                        "#)
                        .bind(now)
                        .bind(correction_id)
                        .bind(format!("{}%", correction_id))
                        .execute(db)
                        .await;
                    }
                    _ => {}
                }

                Ok(json!({
                    "status": "validated",
                    "correction_id": correction_id,
                    "outcome": outcome,
                }).to_string())
            }

            _ => Ok(format!("Unknown action: {}. Use record/list/validate", action)),
        }
    }

    /// Store an important decision with context
    async fn store_decision(&self, args: &Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");
        let decision = args["decision"].as_str().unwrap_or("");
        if key.is_empty() || decision.is_empty() {
            return Ok("Error: key and decision are required".into());
        }

        let category = args["category"].as_str();
        let context = args["context"].as_str();
        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        // Get project_id
        let project_path = self.cwd.to_string_lossy().to_string();
        let project_id: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
            .bind(&project_path)
            .fetch_optional(db)
            .await
            .ok()
            .flatten();

        // Store in memory_facts with fact_type='decision'
        let _ = sqlx::query(r#"
            INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
            VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                project_id = COALESCE(excluded.project_id, memory_facts.project_id),
                updated_at = excluded.updated_at
        "#)
        .bind(&id)
        .bind(key)
        .bind(decision)
        .bind(category)
        .bind(context)
        .bind(now)
        .bind(project_id)
        .execute(db)
        .await;

        // Store in Qdrant for semantic search
        if let Some(semantic) = &self.semantic {
            if semantic.is_available() {
                let mut metadata = HashMap::new();
                metadata.insert("fact_type".into(), json!("decision"));
                metadata.insert("key".into(), json!(key));
                if let Some(cat) = category {
                    metadata.insert("category".into(), json!(cat));
                }

                let _ = semantic.store(COLLECTION_MEMORY, &id, decision, metadata).await;
            }
        }

        Ok(json!({
            "status": "stored",
            "key": key,
            "decision": decision,
            "category": category,
        }).to_string())
    }

    /// Record a rejected approach to avoid re-suggesting it
    async fn record_rejected_approach(&self, args: &Value) -> Result<String> {
        let problem_context = args["problem_context"].as_str().unwrap_or("");
        let approach = args["approach"].as_str().unwrap_or("");
        let rejection_reason = args["rejection_reason"].as_str().unwrap_or("");

        if problem_context.is_empty() || approach.is_empty() || rejection_reason.is_empty() {
            return Ok("Error: problem_context, approach, and rejection_reason are required".into());
        }

        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        // Get project_id
        let project_path = self.cwd.to_string_lossy().to_string();
        let project_id: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
            .bind(&project_path)
            .fetch_optional(db)
            .await
            .ok()
            .flatten();

        let related_files = args["related_files"].as_str();
        let related_topics = args["related_topics"].as_str();

        let _ = sqlx::query(r#"
            INSERT INTO rejected_approaches (id, project_id, problem_context, approach, rejection_reason, related_files, related_topics, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#)
        .bind(&id)
        .bind(project_id)
        .bind(problem_context)
        .bind(approach)
        .bind(rejection_reason)
        .bind(related_files)
        .bind(related_topics)
        .bind(now)
        .execute(db)
        .await;

        // Store in Qdrant for semantic matching
        if let Some(semantic) = &self.semantic {
            if semantic.is_available() {
                let content = format!(
                    "Rejected approach for {}: {} - Reason: {}",
                    problem_context, approach, rejection_reason
                );
                let mut metadata = HashMap::new();
                metadata.insert("type".into(), json!("rejected_approach"));
                metadata.insert("id".into(), json!(id));

                let _ = semantic.store(COLLECTION_MEMORY, &id, &content, metadata).await;
            }
        }

        Ok(json!({
            "status": "recorded",
            "id": id,
            "problem_context": problem_context,
            "approach": approach,
        }).to_string())
    }

    fn resolve_path(&self, path: &str) -> std::path::PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }
}

/// Get all tool definitions for GPT-5.2
pub fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "read_file".into(),
            description: Some("Read the contents of a file".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    }
                },
                "required": ["path"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "write_file".into(),
            description: Some("Write content to a file".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to write to"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "glob".into(),
            description: Some("Find files matching a glob pattern".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., **/*.rs)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Base directory to search from"
                    }
                },
                "required": ["pattern"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "grep".into(),
            description: Some("Search for a pattern in files".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in"
                    }
                },
                "required": ["pattern"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "bash".into(),
            description: Some("Execute a shell command".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "edit_file".into(),
            description: Some("Edit a file by replacing old_string with new_string. The old_string must match exactly and be unique in the file.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to find and replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The text to replace old_string with"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "If true, replace all occurrences. Default false."
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "web_search".into(),
            description: Some("Search the web using DuckDuckGo".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "web_fetch".into(),
            description: Some("Fetch content from a URL and convert to text".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Max content length (default 10000)"
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "remember".into(),
            description: Some("Store a fact, decision, or preference for future recall. Uses semantic search for intelligent retrieval.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The fact or information to remember"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Type of fact: preference, decision, context, general (default)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Optional category for organization"
                    }
                },
                "required": ["content"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "recall".into(),
            description: Some("Search for previously stored memories using semantic similarity. Returns relevant facts, decisions, and preferences.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query - uses semantic similarity matching"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Filter by fact type: preference, decision, context, general"
                    }
                },
                "required": ["query"]
            }),
        },
        // ================================================================
        // Mira Power Armor Tools
        // ================================================================
        Tool {
            tool_type: "function".into(),
            name: "task".into(),
            description: Some("Manage persistent tasks. Actions: create, list, update, complete, delete.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/complete/delete"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (for update/complete/delete). Supports short prefixes."
                    },
                    "title": {
                        "type": "string",
                        "description": "Task title (for create/update)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Task description"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/urgent"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: pending/in_progress/completed/blocked (for update)"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent task ID for subtasks"
                    },
                    "notes": {
                        "type": "string",
                        "description": "Completion notes (for complete)"
                    },
                    "include_completed": {
                        "type": "boolean",
                        "description": "Include completed tasks in list (default false)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list (default 20)"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "goal".into(),
            description: Some("Manage high-level goals with milestones. Actions: create, list, update, add_milestone, complete_milestone.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/add_milestone/complete_milestone"
                    },
                    "goal_id": {
                        "type": "string",
                        "description": "Goal ID (for update/add_milestone)"
                    },
                    "milestone_id": {
                        "type": "string",
                        "description": "Milestone ID (for complete_milestone)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Title (for create/update/add_milestone)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Description"
                    },
                    "success_criteria": {
                        "type": "string",
                        "description": "Success criteria (for create)"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/critical"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: planning/in_progress/blocked/completed/abandoned"
                    },
                    "progress_percent": {
                        "type": "integer",
                        "description": "Progress 0-100 (for update)"
                    },
                    "weight": {
                        "type": "integer",
                        "description": "Milestone weight for progress calculation"
                    },
                    "include_finished": {
                        "type": "boolean",
                        "description": "Include finished goals in list"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "correction".into(),
            description: Some("Record and manage corrections. When the user corrects your approach, record it to avoid the same mistake. Actions: record, list, validate.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: record/list/validate"
                    },
                    "correction_id": {
                        "type": "string",
                        "description": "Correction ID (for validate)"
                    },
                    "correction_type": {
                        "type": "string",
                        "description": "Type: style/approach/pattern/preference/anti_pattern"
                    },
                    "what_was_wrong": {
                        "type": "string",
                        "description": "What you did wrong (for record)"
                    },
                    "what_is_right": {
                        "type": "string",
                        "description": "What you should do instead (for record)"
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Why this is the right approach"
                    },
                    "scope": {
                        "type": "string",
                        "description": "Scope: global/project/file/topic"
                    },
                    "keywords": {
                        "type": "string",
                        "description": "Comma-separated keywords for matching"
                    },
                    "outcome": {
                        "type": "string",
                        "description": "Outcome for validate: validated/deprecated"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "store_decision".into(),
            description: Some("Store an important architectural or design decision with context. Decisions are recalled semantically when relevant.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Unique key for this decision (e.g., 'auth-method', 'database-choice')"
                    },
                    "decision": {
                        "type": "string",
                        "description": "The decision that was made"
                    },
                    "category": {
                        "type": "string",
                        "description": "Category: architecture/design/tech-stack/workflow"
                    },
                    "context": {
                        "type": "string",
                        "description": "Context and rationale for the decision"
                    }
                },
                "required": ["key", "decision"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "record_rejected_approach".into(),
            description: Some("Record an approach that was tried and rejected. This prevents re-suggesting failed approaches in similar contexts.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "problem_context": {
                        "type": "string",
                        "description": "What problem you were trying to solve"
                    },
                    "approach": {
                        "type": "string",
                        "description": "The approach that was tried"
                    },
                    "rejection_reason": {
                        "type": "string",
                        "description": "Why this approach was rejected"
                    },
                    "related_files": {
                        "type": "string",
                        "description": "Comma-separated related file paths"
                    },
                    "related_topics": {
                        "type": "string",
                        "description": "Comma-separated related topics"
                    }
                },
                "required": ["problem_context", "approach", "rejection_reason"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        // 10 core tools + 5 power armor tools = 15
        assert_eq!(tools.len(), 15);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[5].name, "edit_file");
        assert_eq!(tools[8].name, "remember");
        assert_eq!(tools[9].name, "recall");
        // Power armor tools
        assert_eq!(tools[10].name, "task");
        assert_eq!(tools[11].name, "goal");
        assert_eq!(tools[12].name, "correction");
        assert_eq!(tools[13].name, "store_decision");
        assert_eq!(tools[14].name, "record_rejected_approach");
    }

    #[tokio::test]
    async fn test_executor_read_file() {
        let executor = ToolExecutor::new();
        let result = executor.read_file(&json!({"path": "Cargo.toml"})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_edit_file_success() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello world").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let result = executor.edit_file(&json!({
            "path": path,
            "old_string": "Hello",
            "new_string": "Goodbye"
        })).await.unwrap();

        assert!(result.contains("Edited"));

        // Verify content changed
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Goodbye"));
        assert!(!content.contains("Hello"));
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello world").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let result = executor.edit_file(&json!({
            "path": path,
            "old_string": "NotInFile",
            "new_string": "Replacement"
        })).await.unwrap();

        assert!(result.contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_file_not_unique() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "foo bar foo").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let result = executor.edit_file(&json!({
            "path": path,
            "old_string": "foo",
            "new_string": "baz"
        })).await.unwrap();

        assert!(result.contains("2 times"));
    }

    #[tokio::test]
    async fn test_edit_file_replace_all() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "foo bar foo").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let result = executor.edit_file(&json!({
            "path": path,
            "old_string": "foo",
            "new_string": "baz",
            "replace_all": true
        })).await.unwrap();

        assert!(result.contains("Edited"));

        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content.matches("baz").count(), 2);
        assert!(!content.contains("foo"));
    }

    #[test]
    fn test_html_to_text() {
        let html = r#"
            <html>
            <head><script>alert('hi')</script></head>
            <body>
                <h1>Title</h1>
                <p>Hello <b>world</b>!</p>
                <div>Another &amp; line</div>
            </body>
            </html>
        "#;

        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world!"));
        assert!(text.contains("Another & line"));
        assert!(!text.contains("<script>"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_html_to_text_entities() {
        let html = "&lt;code&gt; &amp; &quot;test&quot;";
        let text = html_to_text(html);
        assert_eq!(text, "<code> & \"test\"");
    }
}
