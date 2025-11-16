// src/operations/engine/external_handlers.rs
// Handlers for external operations (web search, URL fetch, command execution)

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

/// Handles external operations (web, commands)
pub struct ExternalHandlers {
    project_dir: PathBuf,
    http_client: reqwest::Client,
}

impl ExternalHandlers {
    pub fn new(project_dir: PathBuf) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mira-Bot/1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            project_dir,
            http_client,
        }
    }

    /// Execute an external tool call
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        match tool_name {
            "web_search_internal" => self.web_search(args).await,
            "fetch_url_internal" => self.fetch_url(args).await,
            "execute_command_internal" => self.execute_command(args).await,
            _ => Err(anyhow::anyhow!("Unknown external tool: {}", tool_name)),
        }
    }

    /// Search the web for information
    async fn web_search(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .context("Missing query parameter")?;

        let num_results = args
            .get("num_results")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5)
            .min(10);

        let search_type = args
            .get("search_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        info!(
            "[EXTERNAL] Web search: '{}' (type: {}, results: {})",
            query, search_type, num_results
        );

        // Build search query with site filters based on search type
        let enhanced_query = match search_type {
            "documentation" => format!("{} site:docs.rs OR site:developer.mozilla.org OR site:doc.rust-lang.org", query),
            "stackoverflow" => format!("{} site:stackoverflow.com", query),
            "github" => format!("{} site:github.com", query),
            _ => query.to_string(),
        };

        // Use DuckDuckGo HTML search (simple, no API key needed)
        let search_url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(&enhanced_query)
        );

        match self.http_client.get(&search_url).send().await {
            Ok(response) => {
                let html = response.text().await?;
                let results = self.parse_duckduckgo_results(&html, num_results);

                Ok(json!({
                    "success": true,
                    "query": query,
                    "results": results,
                    "message": format!("Found {} results for '{}'", results.len(), query)
                }))
            }
            Err(e) => {
                warn!("[EXTERNAL] Web search failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": format!("Search failed: {}", e),
                    "message": "Could not complete web search. Try a different query or check internet connection."
                }))
            }
        }
    }

    /// Parse DuckDuckGo HTML results (simple extraction)
    fn parse_duckduckgo_results(&self, html: &str, limit: usize) -> Vec<Value> {
        let mut results = Vec::new();

        // Simple regex-based extraction (not perfect but works)
        // Look for result blocks in DuckDuckGo HTML
        let re_result = regex::Regex::new(r#"<a rel="nofollow" class="result__a" href="([^"]+)">([^<]+)</a>"#)
            .unwrap();
        let re_snippet = regex::Regex::new(r#"<a class="result__snippet"[^>]*>([^<]+)</a>"#)
            .unwrap();

        let urls: Vec<_> = re_result.captures_iter(html).collect();
        let snippets: Vec<_> = re_snippet.captures_iter(html).collect();

        for (i, url_capture) in urls.iter().enumerate().take(limit) {
            let url = url_capture.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = url_capture.get(2).map(|m| m.as_str()).unwrap_or("Untitled");
            let snippet = snippets
                .get(i)
                .and_then(|s| s.get(1))
                .map(|m| m.as_str())
                .unwrap_or("");

            // Decode HTML entities
            let title = html_escape::decode_html_entities(title).to_string();
            let snippet = html_escape::decode_html_entities(snippet).to_string();

            results.push(json!({
                "title": title,
                "url": url,
                "snippet": snippet
            }));
        }

        // If regex parsing failed, return a fallback message
        if results.is_empty() {
            results.push(json!({
                "title": "Search completed",
                "url": "",
                "snippet": "No results could be parsed. Try using fetch_url with a specific documentation URL instead."
            }));
        }

        results
    }

    /// Fetch content from a URL
    async fn fetch_url(&self, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .context("Missing url parameter")?;

        let extract_mode = args
            .get("extract_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("main_content");

        info!("[EXTERNAL] Fetching URL: {} (mode: {})", url, extract_mode);

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(json!({
                "success": false,
                "error": "Invalid URL: must start with http:// or https://",
                "content": ""
            }));
        }

        match self.http_client.get(url).send().await {
            Ok(response) => {
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let text = response.text().await?;

                // Extract content based on mode
                let extracted = if content_type.contains("text/plain") {
                    // Plain text - return as-is
                    text
                } else {
                    // HTML - extract based on mode
                    match extract_mode {
                        "full" => self.html_to_text(&text),
                        "code_blocks" => self.extract_code_blocks(&text),
                        _ => self.extract_main_content(&text),
                    }
                };

                Ok(json!({
                    "success": true,
                    "url": url,
                    "content_type": content_type,
                    "content": extracted,
                    "length": extracted.len()
                }))
            }
            Err(e) => {
                warn!("[EXTERNAL] URL fetch failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": format!("Failed to fetch URL: {}", e),
                    "content": ""
                }))
            }
        }
    }

    /// Convert HTML to plain text (simple version)
    fn html_to_text(&self, html: &str) -> String {
        // Remove script and style tags
        let re_script = regex::Regex::new(r"(?s)<script[^>]*>.*?</script>").unwrap();
        let re_style = regex::Regex::new(r"(?s)<style[^>]*>.*?</style>").unwrap();
        let text = re_script.replace_all(html, "");
        let text = re_style.replace_all(&text, "");

        // Remove HTML tags
        let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
        let text = re_tags.replace_all(&text, " ");

        // Decode HTML entities
        let text = html_escape::decode_html_entities(&text);

        // Clean up whitespace
        let re_whitespace = regex::Regex::new(r"\s+").unwrap();
        re_whitespace.replace_all(&text, " ").trim().to_string()
    }

    /// Extract main content from HTML (heuristic approach)
    fn extract_main_content(&self, html: &str) -> String {
        // Look for common content containers
        let content_patterns = [
            r"(?s)<main[^>]*>(.*?)</main>",
            r"(?s)<article[^>]*>(.*?)</article>",
            r#"(?s)<div[^>]*class=['"][^'"]*content[^'"]*['"][^>]*>(.*?)</div>"#,
            r#"(?s)<div[^>]*id=['"]content['"][^>]*>(.*?)</div>"#,
        ];

        for pattern in &content_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(capture) = re.captures(html) {
                    if let Some(content_match) = capture.get(1) {
                        return self.html_to_text(content_match.as_str());
                    }
                }
            }
        }

        // Fallback: extract all text
        self.html_to_text(html)
    }

    /// Extract code blocks from HTML
    fn extract_code_blocks(&self, html: &str) -> String {
        let mut code_blocks = Vec::new();

        // Extract <pre> and <code> blocks
        let re_pre = regex::Regex::new(r"(?s)<pre[^>]*>(.*?)</pre>").unwrap();
        let re_code = regex::Regex::new(r"(?s)<code[^>]*>(.*?)</code>").unwrap();

        for capture in re_pre.captures_iter(html) {
            if let Some(code) = capture.get(1) {
                let text = self.html_to_text(code.as_str());
                if !text.trim().is_empty() {
                    code_blocks.push(format!("```\n{}\n```", text.trim()));
                }
            }
        }

        for capture in re_code.captures_iter(html) {
            if let Some(code) = capture.get(1) {
                let text = self.html_to_text(code.as_str());
                if !text.trim().is_empty() && text.len() > 20 {
                    // Filter out short inline code
                    code_blocks.push(format!("`{}`", text.trim()));
                }
            }
        }

        if code_blocks.is_empty() {
            "No code blocks found".to_string()
        } else {
            code_blocks.join("\n\n")
        }
    }

    /// Execute a shell command
    async fn execute_command(&self, args: Value) -> Result<Value> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .context("Missing command parameter")?;

        let working_dir = args
            .get("working_directory")
            .and_then(|v| v.as_str())
            .map(|s| self.project_dir.join(s))
            .unwrap_or_else(|| self.project_dir.clone());

        let timeout_secs = args
            .get("timeout_seconds")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30)
            .min(300); // Max 5 minutes

        info!("[EXTERNAL] Executing command: '{}' in {:?}", command, working_dir);

        // Safety check: block dangerous commands
        let dangerous_patterns = [
            "rm -rf /",
            "dd if=",
            "mkfs",
            "> /dev/",
            "curl.*|.*sh",
            "wget.*|.*sh",
        ];

        for pattern in &dangerous_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if re.is_match(command) {
                    return Ok(json!({
                        "success": false,
                        "error": "Command blocked: potentially dangerous operation",
                        "output": "",
                        "exit_code": -1
                    }));
                }
            }
        }

        // Parse command (simple space-split for now)
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "Empty command",
                "output": "",
                "exit_code": -1
            }));
        }

        let program = parts[0];
        let args_list = &parts[1..];

        // Execute with timeout
        let command_future = Command::new(program)
            .args(args_list)
            .current_dir(&working_dir)
            .output();

        match timeout(Duration::from_secs(timeout_secs), command_future).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(json!({
                    "success": output.status.success(),
                    "output": stdout,
                    "error": stderr,
                    "exit_code": exit_code
                }))
            }
            Ok(Err(e)) => {
                warn!("[EXTERNAL] Command execution failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": format!("Execution failed: {}", e),
                    "output": "",
                    "exit_code": -1
                }))
            }
            Err(_) => {
                warn!("[EXTERNAL] Command timed out after {}s", timeout_secs);
                Ok(json!({
                    "success": false,
                    "error": format!("Command timed out after {} seconds", timeout_secs),
                    "output": "",
                    "exit_code": -1
                }))
            }
        }
    }
}
