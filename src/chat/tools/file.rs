//! File operation tools: read, write, edit, glob, grep
//!
//! Thin wrapper delegating to core::ops::file for shared implementation.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use crate::chat::session::SessionManager;
use crate::core::ops::file as core_file;
use super::types::{DiffInfo, RichToolResult};
use super::FileCache;

/// File tool implementations
pub struct FileTools<'a> {
    pub cwd: &'a Path,
    pub session: &'a Option<Arc<SessionManager>>,
    pub cache: &'a FileCache,
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
        let offset = args["offset"].as_u64().map(|o| o as usize);
        let limit = args["limit"].as_u64().map(|l| l as usize);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Check cache first (only for full file reads)
        if offset.is_none() && limit.is_none() {
            if let Some(content) = self.cache.get(&full_path) {
                return Ok(content);
            }
        }

        let input = core_file::ReadFileInput {
            path: full_path.clone(),
            offset,
            limit,
        };

        match core_file::read_file(input).await {
            Ok(output) => {
                // Cache full content for subsequent reads
                if offset.is_none() && limit.is_none() && !output.truncated {
                    self.cache.put(full_path, output.content.clone());
                }
                Ok(output.content)
            }
            Err(e) => Ok(format!("Error reading {}: {}", path, e)),
        }
    }

    pub async fn write_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        let input = core_file::WriteFileInput {
            path: full_path.clone(),
            content: content.to_string(),
            create_dirs: true,
        };

        match core_file::write_file(input).await {
            Ok(output) => {
                self.cache.update(full_path, content.to_string());
                Ok(format!("Wrote {} bytes to {}", output.bytes_written, path))
            }
            Err(e) => Ok(format!("Error writing {}: {}", path, e)),
        }
    }

    pub async fn write_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Read existing content for diff
        let old_content = self.cache.get(&full_path)
            .or_else(|| tokio::task::block_in_place(|| {
                std::fs::read_to_string(&full_path).ok()
            }));
        let is_new_file = old_content.is_none();

        let input = core_file::WriteFileInput {
            path: full_path.clone(),
            content: content.to_string(),
            create_dirs: true,
        };

        match core_file::write_file(input).await {
            Ok(output) => {
                self.cache.update(full_path, content.to_string());
                Ok(RichToolResult {
                    success: true,
                    output: format!("Wrote {} bytes to {}", output.bytes_written, path),
                    diff: Some(DiffInfo {
                        path: path.to_string(),
                        old_content,
                        new_content: content.to_string(),
                        is_new_file,
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

    pub async fn edit_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        let input = core_file::EditFileInput {
            path: full_path.clone(),
            old_string: old_string.to_string(),
            new_string: new_string.to_string(),
            replace_all,
        };

        match core_file::edit_file(input).await {
            Ok(output) => {
                // Invalidate cache since file changed
                self.cache.invalidate(&full_path);
                Ok(format!(
                    "Edited {}: replaced {} lines with {} lines",
                    path, output.old_lines, output.new_lines
                ))
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    pub async fn edit_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Read current content for diff
        let content = if let Some(cached) = self.cache.get(&full_path) {
            cached
        } else {
            match tokio::fs::read_to_string(&full_path).await {
                Ok(c) => c,
                Err(e) => {
                    return Ok(RichToolResult {
                        success: false,
                        output: format!("Error reading {}: {}", path, e),
                        diff: None,
                    })
                }
            }
        };

        let input = core_file::EditFileInput {
            path: full_path.clone(),
            old_string: old_string.to_string(),
            new_string: new_string.to_string(),
            replace_all,
        };

        match core_file::edit_file(input).await {
            Ok(output) => {
                let new_content = if replace_all {
                    content.replace(old_string, new_string)
                } else {
                    content.replacen(old_string, new_string, 1)
                };

                self.cache.update(full_path, new_content.clone());

                Ok(RichToolResult {
                    success: true,
                    output: format!(
                        "Edited {}: replaced {} lines with {} lines",
                        path, output.old_lines, output.new_lines
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
                output: format!("Error: {}", e),
                diff: None,
            }),
        }
    }

    pub async fn glob(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let base_path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = base_path.as_deref().unwrap_or(self.cwd);

        let input = core_file::GlobInput {
            pattern: pattern.to_string(),
            base_path: search_dir.to_path_buf(),
        };

        match core_file::glob_files(input) {
            Ok(matches) => {
                if matches.is_empty() {
                    Ok("No matches found".into())
                } else {
                    Ok(matches.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"))
                }
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    pub async fn grep(&self, args: &Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let path = args["path"].as_str().map(|p| self.resolve_path(p));
        let search_dir = path.as_deref().unwrap_or(self.cwd).to_path_buf();

        let input = core_file::GrepInput {
            pattern: pattern.to_string(),
            search_path: search_dir,
            max_matches: Some(50),
            max_line_len: Some(200),
            case_insensitive: false,
        };

        // Run in blocking task since grep is synchronous
        let result = tokio::task::spawn_blocking(move || {
            core_file::grep_files(input)
        }).await?;

        match result {
            Ok(matches) => {
                if matches.is_empty() {
                    Ok("No matches found".into())
                } else {
                    let lines: Vec<String> = matches.iter()
                        .map(|m| format!("{}:{}:{}", m.file, m.line_number, m.content))
                        .collect();
                    let truncated = if lines.len() >= 50 {
                        format!("\n... (truncated at {} matches)", 50)
                    } else {
                        String::new()
                    };
                    Ok(format!("{}{}", lines.join("\n"), truncated))
                }
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
