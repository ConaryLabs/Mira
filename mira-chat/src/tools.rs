//! Tool definitions for GPT-5.2 function calling
//!
//! Implements coding assistant tools:
//! - File operations (read, write, edit, glob, grep)
//! - Shell execution
//! - Web search/fetch
//!
//! Tools are executed locally, results returned to GPT-5.2

use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

use crate::responses::{Function, Tool};

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
            "glob" => self.glob(&args).await,
            "grep" => self.grep(&args).await,
            "bash" => self.bash(&args).await,
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
            r#type: "function".into(),
            function: Function {
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
        },
        Tool {
            r#type: "function".into(),
            function: Function {
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
        },
        Tool {
            r#type: "function".into(),
            function: Function {
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
        },
        Tool {
            r#type: "function".into(),
            function: Function {
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
        },
        Tool {
            r#type: "function".into(),
            function: Function {
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
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0].function.name, "read_file");
    }

    #[tokio::test]
    async fn test_executor_read_file() {
        let executor = ToolExecutor::new();
        let result = executor.read_file(&json!({"path": "Cargo.toml"})).await;
        assert!(result.is_ok());
    }
}
