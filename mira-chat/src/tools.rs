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
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::responses::Tool;
use crate::semantic::{SemanticSearch, COLLECTION_MEMORY};

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

/// Tool executor handles tool invocation and result formatting
pub struct ToolExecutor {
    /// Working directory for file operations
    pub cwd: std::path::PathBuf,
    /// Semantic search client (optional)
    semantic: Option<Arc<SemanticSearch>>,
    /// SQLite database pool (optional)
    db: Option<SqlitePool>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            semantic: None,
            db: None,
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
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }

    async fn read_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading {}: {}", path, e)),
        }
    }

    async fn write_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

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
        assert_eq!(tools.len(), 10); // read, write, edit, glob, grep, bash, web_search, web_fetch, remember, recall
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[5].name, "edit_file");
        assert_eq!(tools[8].name, "remember");
        assert_eq!(tools[9].name, "recall");
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
