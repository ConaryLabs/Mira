// src/operations/engine/external_handlers.rs
// Handlers for external operations (web search, URL fetch, command execution)

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::sudo::{AuthorizationDecision, SudoAuditEntry, SudoPermissionService};

/// Handles external operations (web, commands)
pub struct ExternalHandlers {
    project_dir: PathBuf,
    http_client: reqwest::Client,
    sudo_service: Option<Arc<SudoPermissionService>>,
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
            sudo_service: None,
        }
    }

    /// Set the sudo permission service for command authorization
    pub fn with_sudo_service(mut self, sudo_service: Arc<SudoPermissionService>) -> Self {
        self.sudo_service = Some(sudo_service);
        self
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

        let use_sudo = args
            .get("use_sudo")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let operation_id = args
            .get("operation_id")
            .and_then(|v| v.as_str());

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str());

        let reason = args
            .get("reason")
            .and_then(|v| v.as_str());

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

        // Handle sudo commands
        if use_sudo {
            if let Some(ref sudo_service) = self.sudo_service {
                info!("[EXTERNAL] Checking sudo authorization for: '{}'", command);

                match sudo_service
                    .check_authorization(command, operation_id, session_id, reason)
                    .await?
                {
                    AuthorizationDecision::Allowed { permission_id } => {
                        info!("[EXTERNAL] Sudo command auto-allowed (permission: {})", permission_id);
                        // Execute with sudo
                        return self
                            .execute_sudo_command(
                                command,
                                &working_dir,
                                timeout_secs,
                                operation_id,
                                session_id,
                                Some(permission_id),
                                None,
                            )
                            .await;
                    }
                    AuthorizationDecision::RequiresApproval {
                        approval_request_id,
                    } => {
                        info!("[EXTERNAL] Sudo command requires approval: {}", approval_request_id);
                        // Return special response indicating approval needed
                        return Ok(json!({
                            "success": false,
                            "requires_approval": true,
                            "approval_request_id": approval_request_id,
                            "command": command,
                            "message": "This command requires user approval before execution"
                        }));
                    }
                    AuthorizationDecision::Denied { reason } => {
                        warn!("[EXTERNAL] Sudo command denied: {}", reason);
                        return Ok(json!({
                            "success": false,
                            "error": format!("Permission denied: {}", reason),
                            "output": "",
                            "exit_code": -1
                        }));
                    }
                }
            } else {
                return Ok(json!({
                    "success": false,
                    "error": "Sudo permissions system not configured",
                    "output": "",
                    "exit_code": -1
                }));
            }
        }

        // Regular (non-sudo) command execution
        info!("[EXTERNAL] Executing command: '{}' in {:?}", command, working_dir);

        // Safety check: block dangerous commands (for non-sudo)
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

        // Execute regular command
        self.execute_regular_command(command, &working_dir, timeout_secs)
            .await
    }

    /// Execute a regular (non-sudo) command
    async fn execute_regular_command(
        &self,
        command: &str,
        working_dir: &std::path::Path,
        timeout_secs: u64,
    ) -> Result<Value> {
        if command.trim().is_empty() {
            return Ok(json!({
                "success": false,
                "error": "Empty command",
                "output": "",
                "exit_code": -1
            }));
        }

        // Execute through shell to properly handle quoting, redirection, pipes, etc.
        let command_future = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
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

    /// Execute a sudo command and log to audit trail
    #[allow(clippy::too_many_arguments)]
    async fn execute_sudo_command(
        &self,
        command: &str,
        working_dir: &std::path::Path,
        timeout_secs: u64,
        operation_id: Option<&str>,
        session_id: Option<&str>,
        permission_id: Option<i64>,
        approval_request_id: Option<String>,
    ) -> Result<Value> {
        info!("[EXTERNAL] Executing sudo command: '{}'", command);

        if command.trim().is_empty() {
            return Ok(json!({
                "success": false,
                "error": "Empty command",
                "output": "",
                "exit_code": -1
            }));
        }

        // Execute through shell with sudo to properly handle quoting, redirection, pipes, etc.
        let command_future = Command::new("sudo")
            .arg("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .output();

        let result = match timeout(Duration::from_secs(timeout_secs), command_future).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);
                let success = output.status.success();

                // Log to audit trail
                if let Some(ref sudo_service) = self.sudo_service {
                    let audit_entry = SudoAuditEntry {
                        command: command.to_string(),
                        working_dir: Some(working_dir.display().to_string()),
                        permission_id,
                        approval_request_id: approval_request_id.clone(),
                        authorization_type: if permission_id.is_some() {
                            "whitelist".to_string()
                        } else {
                            "approval".to_string()
                        },
                        operation_id: operation_id.map(|s| s.to_string()),
                        session_id: session_id.map(|s| s.to_string()),
                        executed_by: "llm".to_string(),
                        exit_code: Some(exit_code),
                        stdout: Some(stdout.clone()),
                        stderr: Some(stderr.clone()),
                        success,
                        error_message: if success { None } else { Some(stderr.clone()) },
                    };

                    if let Err(e) = sudo_service.log_execution(audit_entry).await {
                        warn!("[EXTERNAL] Failed to log sudo execution: {}", e);
                    }
                }

                json!({
                    "success": success,
                    "output": stdout,
                    "error": stderr,
                    "exit_code": exit_code
                })
            }
            Ok(Err(e)) => {
                warn!("[EXTERNAL] Sudo command execution failed: {}", e);

                // Log failure
                if let Some(ref sudo_service) = self.sudo_service {
                    let audit_entry = SudoAuditEntry {
                        command: command.to_string(),
                        working_dir: Some(working_dir.display().to_string()),
                        permission_id,
                        approval_request_id: approval_request_id.clone(),
                        authorization_type: if permission_id.is_some() {
                            "whitelist".to_string()
                        } else {
                            "approval".to_string()
                        },
                        operation_id: operation_id.map(|s| s.to_string()),
                        session_id: session_id.map(|s| s.to_string()),
                        executed_by: "llm".to_string(),
                        exit_code: None,
                        stdout: None,
                        stderr: None,
                        success: false,
                        error_message: Some(e.to_string()),
                    };

                    let _ = sudo_service.log_execution(audit_entry).await;
                }

                json!({
                    "success": false,
                    "error": format!("Execution failed: {}", e),
                    "output": "",
                    "exit_code": -1
                })
            }
            Err(_) => {
                warn!("[EXTERNAL] Sudo command timed out after {}s", timeout_secs);

                // Log timeout
                if let Some(ref sudo_service) = self.sudo_service {
                    let audit_entry = SudoAuditEntry {
                        command: command.to_string(),
                        working_dir: Some(working_dir.display().to_string()),
                        permission_id,
                        approval_request_id,
                        authorization_type: if permission_id.is_some() {
                            "whitelist".to_string()
                        } else {
                            "approval".to_string()
                        },
                        operation_id: operation_id.map(|s| s.to_string()),
                        session_id: session_id.map(|s| s.to_string()),
                        executed_by: "llm".to_string(),
                        exit_code: None,
                        stdout: None,
                        stderr: None,
                        success: false,
                        error_message: Some(format!("Command timed out after {} seconds", timeout_secs)),
                    };

                    let _ = sudo_service.log_execution(audit_entry).await;
                }

                json!({
                    "success": false,
                    "error": format!("Command timed out after {} seconds", timeout_secs),
                    "output": "",
                    "exit_code": -1
                })
            }
        };

        Ok(result)
    }
}
