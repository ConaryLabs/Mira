//! Tool definitions for GPT-5.2 function calling
//!
//! Implements coding assistant tools:
//! - File operations (read, write, edit, glob, grep)
//! - Shell execution
//! - Web search/fetch
//!
//! Tools are executed locally, results returned to GPT-5.2

use anyhow::Result;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

use crate::responses::Tool;

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
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
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
        assert_eq!(tools.len(), 8); // read, write, edit, glob, grep, bash, web_search, web_fetch
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[5].name, "edit_file");
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
