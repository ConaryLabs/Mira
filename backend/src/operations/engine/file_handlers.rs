// src/operations/engine/file_handlers.rs
// File operation handlers for LLM tool calling

use anyhow::{Context, Result};
use glob::glob;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use tokio::fs;
use tracing::{info, warn};

// Pre-compiled regex patterns for symbol extraction (compiled once at module load)
// Rust patterns
static RE_RUST_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(pub\s+)?(?:async\s+)?fn\s+(\w+)").expect("Invalid RE_RUST_FN regex")
});
static RE_RUST_STRUCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(pub\s+)?struct\s+(\w+)").expect("Invalid RE_RUST_STRUCT regex")
});
static RE_RUST_ENUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(pub\s+)?enum\s+(\w+)").expect("Invalid RE_RUST_ENUM regex")
});
static RE_RUST_TRAIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(pub\s+)?trait\s+(\w+)").expect("Invalid RE_RUST_TRAIT regex")
});

// TypeScript/JavaScript patterns
static RE_TS_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)").expect("Invalid RE_TS_FN regex")
});
static RE_TS_CLASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:export\s+)?class\s+(\w+)").expect("Invalid RE_TS_CLASS regex")
});
static RE_TS_INTERFACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:export\s+)?interface\s+(\w+)").expect("Invalid RE_TS_INTERFACE regex")
});
static RE_TS_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:export\s+)?type\s+(\w+)").expect("Invalid RE_TS_TYPE regex")
});

// Generic function pattern for other languages
static RE_GENERIC_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:def|function|fn)\s+(\w+)").expect("Invalid RE_GENERIC_FN regex")
});

/// Handler for file operation tool calls from LLM
pub struct FileHandlers {
    /// Base directory for all file operations (project root)
    base_dir: PathBuf,
    /// Optional project-specific working directory (overrides base_dir when set)
    /// Uses RwLock for interior mutability - allows updating without &mut self
    project_dir: RwLock<Option<PathBuf>>,
}

impl FileHandlers {
    /// Create a new file handlers instance
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            project_dir: RwLock::new(None),
        }
    }

    /// Set the project directory for file operations
    /// This overrides the default base_dir
    pub fn set_project_dir(&self, path: PathBuf) {
        info!("[FileHandlers] Setting project directory: {}", path.display());
        if let Ok(mut guard) = self.project_dir.write() {
            *guard = Some(path);
        }
    }

    /// Clear the project directory override
    pub fn clear_project_dir(&self) {
        if let Ok(mut guard) = self.project_dir.write() {
            *guard = None;
        }
    }

    /// Get the effective base directory (project_dir if set, else base_dir)
    fn effective_base_dir(&self) -> PathBuf {
        if let Ok(guard) = self.project_dir.read() {
            if let Some(ref path) = *guard {
                return path.clone();
            }
        }
        self.base_dir.clone()
    }

    /// Execute a file operation tool call
    pub async fn execute_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        info!("Executing file tool: {}", tool_name);

        match tool_name {
            "read_file" => self.read_file(arguments).await,
            "write_file" => self.write_file(arguments).await,
            "edit_file" => self.edit_file(arguments).await,
            "list_files" => self.list_files(arguments).await,
            "grep_files" => self.grep_files(arguments).await,
            "summarize_file" => self.summarize_file(arguments).await,
            "extract_symbols" => self.extract_symbols(arguments).await,
            "count_lines" => self.count_lines(arguments).await,
            _ => Err(anyhow::anyhow!("Unknown file tool: {}", tool_name)),
        }
    }

    /// Read a file from the project directory with optional offset/limit
    ///
    /// Efficiency features (Claude Code patterns):
    /// - Default limit of 500 lines (prevents context bloat)
    /// - Offset support for reading specific sections
    /// - Long lines truncated to 500 chars
    /// - Shows truncation notice with instructions for getting more
    async fn read_file(&self, args: Value) -> Result<Value> {
        const MAX_READ_LINES: usize = 500;
        const MAX_LINE_LENGTH: usize = 500;
        const PREVIEW_HEAD: usize = 100;
        const PREVIEW_TAIL: usize = 50;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        // Parse offset and limit (new params for efficiency)
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(MAX_READ_LINES);

        let full_path = self.resolve_path(path)?;
        info!("Reading file: {} (offset={}, limit={})", full_path.display(), offset, limit);

        let content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        let total_lines = content.lines().count();
        let total_chars = content.len();

        // Apply offset and limit
        let lines: Vec<&str> = content.lines().collect();
        let mut truncated = false;
        let mut truncation_message = String::new();

        let result_content = if total_lines <= limit && offset == 0 {
            // File fits within limit, return as-is (with long line truncation)
            lines.iter()
                .map(|line| {
                    if line.len() > MAX_LINE_LENGTH {
                        format!("{}... [line truncated, {} chars total]", &line[..MAX_LINE_LENGTH], line.len())
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else if offset > 0 || limit < total_lines {
            // User specified offset/limit - return exactly that range
            truncated = true;
            let start = offset.min(total_lines);
            let end = (offset + limit).min(total_lines);
            truncation_message = format!(
                "Showing lines {}-{} of {}. Use offset/limit to read other sections.",
                start + 1, end, total_lines
            );
            lines[start..end]
                .iter()
                .map(|line| {
                    if line.len() > MAX_LINE_LENGTH {
                        format!("{}... [truncated]", &line[..MAX_LINE_LENGTH])
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            // File exceeds default limit - show head + tail with truncation notice
            truncated = true;
            let head: Vec<String> = lines[..PREVIEW_HEAD.min(total_lines)]
                .iter()
                .map(|line| {
                    if line.len() > MAX_LINE_LENGTH {
                        format!("{}... [truncated]", &line[..MAX_LINE_LENGTH])
                    } else {
                        line.to_string()
                    }
                })
                .collect();

            let tail_start = total_lines.saturating_sub(PREVIEW_TAIL);
            let tail: Vec<String> = lines[tail_start..]
                .iter()
                .map(|line| {
                    if line.len() > MAX_LINE_LENGTH {
                        format!("{}... [truncated]", &line[..MAX_LINE_LENGTH])
                    } else {
                        line.to_string()
                    }
                })
                .collect();

            let omitted = total_lines - PREVIEW_HEAD - PREVIEW_TAIL;
            truncation_message = format!(
                "File has {} lines. Showing first {} and last {}. {} lines omitted. Use read_file with offset/limit for specific sections.",
                total_lines, PREVIEW_HEAD, PREVIEW_TAIL, omitted
            );

            format!(
                "{}\n\n[... {} lines omitted ...]\n\n{}",
                head.join("\n"),
                omitted,
                tail.join("\n")
            )
        };

        let mut result = json!({
            "success": true,
            "path": path,
            "content": result_content,
            "total_lines": total_lines,
            "total_chars": total_chars
        });

        if truncated {
            result["truncated"] = json!(true);
            result["truncation_message"] = json!(truncation_message);
        }

        Ok(result)
    }

    /// Write content to a file in the project directory (or anywhere if unrestricted)
    async fn write_file(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        let unrestricted = args
            .get("unrestricted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let full_path = if unrestricted {
            // Unrestricted mode - use absolute path as-is
            info!("Writing file (unrestricted): {}", path);
            PathBuf::from(path)
        } else {
            // Normal mode - validate path is within project
            self.resolve_path(path)?
        };

        info!("Writing file: {}", full_path.display());

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        fs::write(&full_path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", full_path.display()))?;

        let line_count = content.lines().count();

        Ok(json!({
            "success": true,
            "path": path,
            "bytes_written": content.len(),
            "lines_written": line_count
        }))
    }

    /// Edit a file using search and replace
    async fn edit_file(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let search = args
            .get("search")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'search' argument"))?;

        let replace = args
            .get("replace")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'replace' argument"))?;

        let full_path = self.resolve_path(path)?;
        info!("Editing file: {} (search/replace)", full_path.display());

        // Read the file
        let original_content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        // Check if search string exists
        if !original_content.contains(search) {
            return Err(anyhow::anyhow!(
                "Search string not found in file. Make sure the search text matches exactly, including whitespace."
            ));
        }

        // Perform replacement
        let new_content = original_content.replace(search, replace);

        // Count occurrences
        let occurrences = original_content.matches(search).count();

        // Write back the modified content
        fs::write(&full_path, &new_content)
            .await
            .with_context(|| format!("Failed to write file: {}", full_path.display()))?;

        Ok(json!({
            "success": true,
            "path": path,
            "replacements_made": occurrences,
            "original_size": original_content.len(),
            "new_size": new_content.len()
        }))
    }

    /// List files in a directory with optional pattern matching
    async fn list_files(&self, args: Value) -> Result<Value> {
        let directory = args
            .get("directory")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'directory' argument"))?;

        let pattern = args.get("pattern").and_then(|v| v.as_str());
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let dir_path = self.resolve_path(directory)?;
        info!("Listing files in: {}", dir_path.display());

        let mut files = Vec::new();

        if let Some(pattern) = pattern {
            // Use glob pattern
            let glob_pattern = if recursive {
                format!("{}/**/{}", dir_path.display(), pattern)
            } else {
                format!("{}/{}", dir_path.display(), pattern)
            };

            info!("Using glob pattern: {}", glob_pattern);

            for entry in glob(&glob_pattern)
                .with_context(|| format!("Invalid glob pattern: {}", glob_pattern))?
            {
                let base = self.effective_base_dir();
                match entry {
                    Ok(path) => {
                        if path.is_file() {
                            if let Some(rel_path) = path.strip_prefix(base).ok() {
                                files.push(rel_path.display().to_string());
                            }
                        }
                    }
                    Err(e) => warn!("Error reading glob entry: {}", e),
                }
            }
        } else {
            // List directory without pattern
            let mut entries = fs::read_dir(&dir_path)
                .await
                .with_context(|| format!("Failed to read directory: {}", dir_path.display()))?;

            let base = self.effective_base_dir();
            while let Some(entry) = entries
                .next_entry()
                .await
                .with_context(|| format!("Failed to read directory entry"))?
            {
                let path = entry.path();
                if path.is_file() {
                    if let Some(rel_path) = path.strip_prefix(&base).ok() {
                        files.push(rel_path.display().to_string());
                    }
                } else if recursive && path.is_dir() {
                    // Skip common ignored directories
                    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if dir_name == ".git"
                        || dir_name == "node_modules"
                        || dir_name == "target"
                        || dir_name == ".next"
                        || dir_name == "dist"
                        || dir_name == "build"
                    {
                        continue;
                    }

                    // Recursively list subdirectories
                    let sub_args = json!({
                        "directory": path.strip_prefix(&base)
                            .unwrap_or(&path)
                            .display()
                            .to_string(),
                        "recursive": true
                    });
                    // Box::pin for recursive async call
                    if let Ok(result) = Box::pin(self.list_files(sub_args)).await {
                        if let Some(sub_files) = result.get("files").and_then(|f| f.as_array()) {
                            for file in sub_files {
                                if let Some(file_str) = file.as_str() {
                                    files.push(file_str.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(json!({
            "success": true,
            "directory": directory,
            "file_count": files.len(),
            "files": files
        }))
    }

    /// Search for patterns in files using regex
    async fn grep_files(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' argument"))?;

        let search_path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let file_pattern = args
            .get("file_pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("*");

        let case_insensitive = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        info!(
            "Grepping for pattern '{}' in '{}' (files: {})",
            pattern, search_path, file_pattern
        );

        // Build regex
        let regex = if case_insensitive {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
        } else {
            regex::Regex::new(pattern)
        }
        .with_context(|| format!("Invalid regex pattern: {}", pattern))?;

        let base_path = self.resolve_path(search_path)?;

        // Build glob pattern for files to search
        let glob_pattern = format!("{}/**/{}", base_path.display(), file_pattern);

        let mut matches = Vec::new();
        let mut files_searched = 0;

        for entry in glob(&glob_pattern)
            .with_context(|| format!("Invalid glob pattern: {}", glob_pattern))?
        {
            match entry {
                Ok(path) if path.is_file() => {
                    files_searched += 1;

                    // Read file content
                    if let Ok(content) = fs::read_to_string(&path).await {
                        // Search for matches
                        for (line_num, line) in content.lines().enumerate() {
                            if regex.is_match(line) {
                                let rel_path = path
                                    .strip_prefix(self.effective_base_dir())
                                    .unwrap_or(&path)
                                    .display()
                                    .to_string();

                                matches.push(json!({
                                    "file": rel_path,
                                    "line": line_num + 1,
                                    "content": line
                                }));
                            }
                        }
                    }
                }
                Err(e) => warn!("Error reading glob entry: {}", e),
                _ => {}
            }
        }

        Ok(json!({
            "success": true,
            "pattern": pattern,
            "files_searched": files_searched,
            "match_count": matches.len(),
            "matches": matches
        }))
    }

    /// Resolve a relative path to an absolute path within the effective base directory
    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let path = Path::new(path);
        let base = self.effective_base_dir();

        // Prevent directory traversal attacks
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(anyhow::anyhow!(
                "Path traversal not allowed: {}",
                path.display()
            ));
        }

        let full_path = base.join(path);

        // Ensure the resolved path is still within base directory
        if !full_path.starts_with(&base) {
            return Err(anyhow::anyhow!(
                "Path outside project directory: {}",
                path.display()
            ));
        }

        Ok(full_path)
    }

    /// Summarize a file without reading full content (token optimization)
    async fn summarize_file(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let preview_lines = args
            .get("preview_lines")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(10);

        let full_path = self.resolve_path(path)?;
        info!("Summarizing file: {}", full_path.display());

        let content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let total_chars = content.len();

        // Get first N and last N lines
        let start_lines: Vec<String> = lines
            .iter()
            .take(preview_lines)
            .map(|s| s.to_string())
            .collect();

        let end_lines: Vec<String> = if total_lines > preview_lines * 2 {
            lines
                .iter()
                .skip(total_lines.saturating_sub(preview_lines))
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        // Detect file type and patterns
        let file_type = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");

        // Simple pattern detection
        let has_imports = content.contains("import ") || content.contains("use ");
        let has_exports = content.contains("export ") || content.contains("pub ");
        let has_classes = content.contains("class ") || content.contains("struct ");
        let has_functions = content.contains("function ") || content.contains("fn ") || content.contains("async ");

        Ok(json!({
            "success": true,
            "path": path,
            "file_type": file_type,
            "stats": {
                "total_lines": total_lines,
                "total_chars": total_chars,
                "preview_lines": preview_lines
            },
            "preview": {
                "start": start_lines,
                "end": end_lines,
                "omitted_lines": total_lines.saturating_sub(preview_lines * 2)
            },
            "patterns": {
                "has_imports": has_imports,
                "has_exports": has_exports,
                "has_classes": has_classes,
                "has_functions": has_functions
            }
        }))
    }

    /// Extract symbols (functions, classes, etc.) from a file
    async fn extract_symbols(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let full_path = self.resolve_path(path)?;
        info!("Extracting symbols from: {}", full_path.display());

        let content = fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;

        let file_type = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");

        // Simple regex-based symbol extraction (language-specific)
        let mut symbols = Vec::new();

        match file_type {
            "rs" => {
                // Rust: fn, struct, impl, trait, enum (using pre-compiled regexes)
                for cap in RE_RUST_FN.captures_iter(&content) {
                    symbols.push(json!({
                        "type": "function",
                        "name": &cap[2],
                        "visibility": if cap.get(1).is_some() { "public" } else { "private" }
                    }));
                }

                for cap in RE_RUST_STRUCT.captures_iter(&content) {
                    symbols.push(json!({
                        "type": "struct",
                        "name": &cap[2],
                        "visibility": if cap.get(1).is_some() { "public" } else { "private" }
                    }));
                }

                for cap in RE_RUST_ENUM.captures_iter(&content) {
                    symbols.push(json!({
                        "type": "enum",
                        "name": &cap[2],
                        "visibility": if cap.get(1).is_some() { "public" } else { "private" }
                    }));
                }

                for cap in RE_RUST_TRAIT.captures_iter(&content) {
                    symbols.push(json!({
                        "type": "trait",
                        "name": &cap[2],
                        "visibility": if cap.get(1).is_some() { "public" } else { "private" }
                    }));
                }
            }
            "ts" | "tsx" | "js" | "jsx" => {
                // TypeScript/JavaScript: function, class, interface, type (using pre-compiled regexes)
                for cap in RE_TS_FN.captures_iter(&content) {
                    symbols.push(json!({"type": "function", "name": &cap[1]}));
                }

                for cap in RE_TS_CLASS.captures_iter(&content) {
                    symbols.push(json!({"type": "class", "name": &cap[1]}));
                }

                for cap in RE_TS_INTERFACE.captures_iter(&content) {
                    symbols.push(json!({"type": "interface", "name": &cap[1]}));
                }

                for cap in RE_TS_TYPE.captures_iter(&content) {
                    symbols.push(json!({"type": "type", "name": &cap[1]}));
                }
            }
            _ => {
                // Generic: just find function-like patterns (using pre-compiled regex)
                for cap in RE_GENERIC_FN.captures_iter(&content) {
                    symbols.push(json!({"type": "function", "name": &cap[1]}));
                }
            }
        }

        Ok(json!({
            "success": true,
            "path": path,
            "file_type": file_type,
            "symbol_count": symbols.len(),
            "symbols": symbols
        }))
    }

    /// Count lines in multiple files (minimal token usage)
    async fn count_lines(&self, args: Value) -> Result<Value> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

        info!("Counting lines for {} file(s)", paths.len());

        let mut file_stats = Vec::new();

        for path_val in paths {
            if let Some(path) = path_val.as_str() {
                let full_path = match self.resolve_path(path) {
                    Ok(p) => p,
                    Err(e) => {
                        file_stats.push(json!({
                            "path": path,
                            "error": e.to_string()
                        }));
                        continue;
                    }
                };

                match fs::read_to_string(&full_path).await {
                    Ok(content) => {
                        let line_count = content.lines().count();
                        let char_count = content.len();
                        let word_count = content.split_whitespace().count();

                        file_stats.push(json!({
                            "path": path,
                            "lines": line_count,
                            "chars": char_count,
                            "words": word_count
                        }));
                    }
                    Err(e) => {
                        file_stats.push(json!({
                            "path": path,
                            "error": e.to_string()
                        }));
                    }
                }
            }
        }

        Ok(json!({
            "success": true,
            "file_count": file_stats.len(),
            "files": file_stats
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path_security() {
        let base = PathBuf::from("/project");
        let handler = FileHandlers::new(base.clone());

        // Valid paths
        assert!(handler.resolve_path("src/main.rs").is_ok());
        assert!(handler.resolve_path("./src/main.rs").is_ok());

        // Invalid paths (directory traversal)
        assert!(handler.resolve_path("../etc/passwd").is_err());
        assert!(handler.resolve_path("src/../../etc/passwd").is_err());
    }
}
