//! File operation tools: read, write, edit, glob, grep

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use crate::session::SessionManager;
use super::types::{DiffInfo, RichToolResult};

/// File tool implementations
pub struct FileTools<'a> {
    pub cwd: &'a Path,
    pub session: &'a Option<Arc<SessionManager>>,
}

impl<'a> FileTools<'a> {
    fn track_file(&self, path: &str) {
        if let Some(session) = self.session {
            session.track_file(path);
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

    pub async fn read_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading {}: {}", path, e)),
        }
    }

    pub async fn write_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        match tokio::fs::write(&full_path, content).await {
            Ok(()) => Ok(format!("Wrote {} bytes to {}", content.len(), path)),
            Err(e) => Ok(format!("Error writing {}: {}", path, e)),
        }
    }

    pub async fn write_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Read existing content for diff
        let old_content = tokio::fs::read_to_string(&full_path).await.ok();
        let is_new_file = old_content.is_none();

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

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

    pub async fn edit_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return Ok(format!("Error reading {}: {}", path, e)),
        };

        if !content.contains(old_string) {
            return Ok(format!(
                "Error: old_string not found in {}. Make sure to match exactly including whitespace.",
                path
            ));
        }

        if !replace_all {
            let count = content.matches(old_string).count();
            if count > 1 {
                return Ok(format!(
                    "Error: old_string found {} times in {}. Use replace_all=true or provide more context to make it unique.",
                    count, path
                ));
            }
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

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

    pub async fn edit_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(RichToolResult {
                    success: false,
                    output: format!("Error reading {}: {}", path, e),
                    diff: None,
                })
            }
        };

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

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

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
                        old_content: Some(content),
                        new_content,
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

    pub async fn glob(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let base_path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = base_path.as_deref().unwrap_or(self.cwd);

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

    pub async fn grep(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = path.as_deref().unwrap_or(self.cwd);

        let output = tokio::process::Command::new("rg")
            .args(["--line-number", "--no-heading", pattern])
            .current_dir(search_dir)
            .output()
            .await;

        match output {
            Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
            Err(_) => {
                let output = tokio::process::Command::new("grep")
                    .args(["-rn", pattern, "."])
                    .current_dir(search_dir)
                    .output()
                    .await?;
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
        }
    }
}
