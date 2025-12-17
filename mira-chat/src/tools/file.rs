//! File operation tools: read, write, edit, glob, grep

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use crate::session::SessionManager;
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

    /// Maximum file size to read (1MB) - larger files should use offset/limit
    const MAX_READ_SIZE: usize = 1024 * 1024;

    pub async fn read_file(&self, args: &Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map(|l| l as usize);
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Check cache first (only for full file reads)
        if offset == 0 && limit.is_none() {
            if let Some(content) = self.cache.get(&full_path) {
                return Ok(content);
            }
        }

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                // Apply offset and limit if specified
                let result = if offset > 0 || limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = offset.min(lines.len());
                    let end = limit.map(|l| (start + l).min(lines.len())).unwrap_or(lines.len());
                    lines[start..end].join("\n")
                } else if content.len() > Self::MAX_READ_SIZE {
                    // Truncate very large files with a note
                    let truncated: String = content.chars().take(Self::MAX_READ_SIZE).collect();
                    let total_lines = content.lines().count();
                    let shown_lines = truncated.lines().count();
                    format!(
                        "{}\n\n... [truncated: showing {} of {} lines. Use offset/limit for full content]",
                        truncated, shown_lines, total_lines
                    )
                } else {
                    content.clone()
                };

                // Cache full content (for subsequent reads with offset)
                if offset == 0 && limit.is_none() && content.len() <= Self::MAX_READ_SIZE {
                    self.cache.put(full_path, content);
                }

                Ok(result)
            }
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
            Ok(()) => {
                // Update cache with new content
                self.cache.update(full_path, content.to_string());
                Ok(format!("Wrote {} bytes to {}", content.len(), path))
            }
            Err(e) => Ok(format!("Error writing {}: {}", path, e)),
        }
    }

    pub async fn write_file_rich(&self, args: &Value) -> Result<RichToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.resolve_path(path);

        self.track_file(&full_path.to_string_lossy());

        // Read existing content for diff (use cache if available)
        let old_content = self.cache.get(&full_path)
            .or_else(|| tokio::task::block_in_place(|| {
                std::fs::read_to_string(&full_path).ok()
            }));
        let is_new_file = old_content.is_none();

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        match tokio::fs::write(&full_path, content).await {
            Ok(()) => {
                // Update cache with new content
                self.cache.update(full_path, content.to_string());
                Ok(RichToolResult {
                    success: true,
                    output: format!("Wrote {} bytes to {}", content.len(), path),
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

        // Try cache first, then read from disk
        let content = if let Some(cached) = self.cache.get(&full_path) {
            cached
        } else {
            match tokio::fs::read_to_string(&full_path).await {
                Ok(c) => c,
                Err(e) => return Ok(format!("Error reading {}: {}", path, e)),
            }
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
                // Update cache with new content
                self.cache.update(full_path, new_content);
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

        // Try cache first, then read from disk
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
                // Update cache with new content
                self.cache.update(full_path, new_content.clone());
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
        let search_dir = path.as_deref().unwrap_or(self.cwd).to_path_buf();
        let pattern = pattern.to_string();

        // Run grep in blocking task to avoid blocking async runtime
        tokio::task::spawn_blocking(move || {
            Self::grep_sync(&pattern, &search_dir)
        }).await?
    }

    /// Synchronous grep implementation using ignore crate (respects .gitignore)
    fn grep_sync(pattern: &str, search_dir: &std::path::Path) -> Result<String> {
        use ignore::WalkBuilder;
        use regex::RegexBuilder;
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        // Build regex (case-insensitive by default for usability)
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(false)
            .build()
            .map_err(|e| anyhow::anyhow!("Invalid regex: {}", e))?;

        let mut matches = Vec::new();
        const MAX_MATCHES: usize = 250;   // Increased for fewer round-trips
        const MAX_LINE_LEN: usize = 1000; // Show more context per match

        // Walk directory respecting .gitignore
        let walker = WalkBuilder::new(search_dir)
            .hidden(true)       // Skip hidden files
            .git_ignore(true)   // Respect .gitignore
            .git_global(true)   // Respect global gitignore
            .git_exclude(true)  // Respect .git/info/exclude
            .build();

        for entry in walker {
            if matches.len() >= MAX_MATCHES {
                break;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Skip directories
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(true) {
                continue;
            }

            let path = entry.path();

            // Skip binary files (simple heuristic)
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "ico" | "woff" | "woff2" | "ttf" | "eot" | "pdf" | "zip" | "tar" | "gz" | "exe" | "dll" | "so" | "dylib" | "o" | "a") {
                    continue;
                }
            }

            // Open and search file
            let file = match File::open(path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let reader = BufReader::new(file);
            let rel_path = path.strip_prefix(search_dir).unwrap_or(path);

            for (line_num, line) in reader.lines().enumerate() {
                if matches.len() >= MAX_MATCHES {
                    break;
                }

                let line = match line {
                    Ok(l) => l,
                    Err(_) => continue, // Skip lines with encoding issues
                };

                if regex.is_match(&line) {
                    let display_line = if line.len() > MAX_LINE_LEN {
                        format!("{}...", &line[..MAX_LINE_LEN])
                    } else {
                        line
                    };
                    matches.push(format!("{}:{}:{}", rel_path.display(), line_num + 1, display_line));
                }
            }
        }

        if matches.is_empty() {
            Ok("No matches found".into())
        } else {
            let truncated = if matches.len() >= MAX_MATCHES {
                format!("\n... (truncated at {} matches)", MAX_MATCHES)
            } else {
                String::new()
            };
            Ok(format!("{}{}", matches.join("\n"), truncated))
        }
    }
}
