//! Core file operations - shared by MCP and Chat
//!
//! Pure implementations of file read/write/edit/glob/grep.

use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader};
use std::fs::File;

use ignore::WalkBuilder;
use regex::RegexBuilder;

use super::super::{CoreError, CoreResult};

/// Maximum file size to read (1MB)
const MAX_READ_SIZE: usize = 1024 * 1024;

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct ReadFileInput {
    pub path: PathBuf,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub struct ReadFileOutput {
    pub content: String,
    pub truncated: bool,
    pub total_lines: Option<usize>,
}

pub struct WriteFileInput {
    pub path: PathBuf,
    pub content: String,
    pub create_dirs: bool,
}

pub struct WriteFileOutput {
    pub bytes_written: usize,
    pub path: String,
}

pub struct EditFileInput {
    pub path: PathBuf,
    pub old_string: String,
    pub new_string: String,
    pub replace_all: bool,
}

pub struct EditFileOutput {
    pub success: bool,
    pub old_lines: usize,
    pub new_lines: usize,
    pub occurrences_replaced: usize,
}

pub struct GlobInput {
    pub pattern: String,
    pub base_path: PathBuf,
}

pub struct GrepInput {
    pub pattern: String,
    pub search_path: PathBuf,
    pub max_matches: Option<usize>,
    pub max_line_len: Option<usize>,
    pub case_insensitive: bool,
}

pub struct GrepMatch {
    pub file: String,
    pub line_number: usize,
    pub content: String,
}

// ============================================================================
// Operations
// ============================================================================

/// Read a file with optional offset and limit
pub async fn read_file(input: ReadFileInput) -> CoreResult<ReadFileOutput> {
    let content = tokio::fs::read_to_string(&input.path)
        .await
        .map_err(|e| CoreError::FileRead(input.path.clone(), e.to_string()))?;

    let offset = input.offset.unwrap_or(0);
    let limit = input.limit;

    // Apply offset and limit if specified
    if offset > 0 || limit.is_some() {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let start = offset.min(total_lines);
        let end = limit.map(|l| (start + l).min(total_lines)).unwrap_or(total_lines);

        return Ok(ReadFileOutput {
            content: lines[start..end].join("\n"),
            truncated: end < total_lines,
            total_lines: Some(total_lines),
        });
    }

    // Check if file is too large
    if content.len() > MAX_READ_SIZE {
        let truncated: String = content.chars().take(MAX_READ_SIZE).collect();
        let total_lines = content.lines().count();
        let shown_lines = truncated.lines().count();

        return Ok(ReadFileOutput {
            content: format!(
                "{}\n\n... [truncated: showing {} of {} lines. Use offset/limit for full content]",
                truncated, shown_lines, total_lines
            ),
            truncated: true,
            total_lines: Some(total_lines),
        });
    }

    Ok(ReadFileOutput {
        content,
        truncated: false,
        total_lines: None,
    })
}

/// Write content to a file
pub async fn write_file(input: WriteFileInput) -> CoreResult<WriteFileOutput> {
    // Create parent directories if needed
    if input.create_dirs {
        if let Some(parent) = input.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CoreError::FileWrite(input.path.clone(), e.to_string()))?;
        }
    }

    let bytes = input.content.len();
    tokio::fs::write(&input.path, &input.content)
        .await
        .map_err(|e| CoreError::FileWrite(input.path.clone(), e.to_string()))?;

    Ok(WriteFileOutput {
        bytes_written: bytes,
        path: input.path.to_string_lossy().to_string(),
    })
}

/// Edit a file by replacing text
pub async fn edit_file(input: EditFileInput) -> CoreResult<EditFileOutput> {
    let content = tokio::fs::read_to_string(&input.path)
        .await
        .map_err(|e| CoreError::FileRead(input.path.clone(), e.to_string()))?;

    if !content.contains(&input.old_string) {
        return Err(CoreError::EditNotFound(
            input.path.to_string_lossy().to_string(),
            "old_string not found".to_string(),
        ));
    }

    let count = content.matches(&input.old_string).count();
    if !input.replace_all && count > 1 {
        return Err(CoreError::EditAmbiguous(
            input.path.to_string_lossy().to_string(),
            count,
        ));
    }

    let new_content = if input.replace_all {
        content.replace(&input.old_string, &input.new_string)
    } else {
        content.replacen(&input.old_string, &input.new_string, 1)
    };

    let occurrences = if input.replace_all { count } else { 1 };

    tokio::fs::write(&input.path, &new_content)
        .await
        .map_err(|e| CoreError::FileWrite(input.path.clone(), e.to_string()))?;

    Ok(EditFileOutput {
        success: true,
        old_lines: input.old_string.lines().count(),
        new_lines: input.new_string.lines().count(),
        occurrences_replaced: occurrences,
    })
}

/// Find files matching a glob pattern
pub fn glob_files(input: GlobInput) -> CoreResult<Vec<PathBuf>> {
    let glob_pattern = format!("{}/{}", input.base_path.display(), input.pattern);

    let matches: Vec<PathBuf> = glob::glob(&glob_pattern)
        .map_err(|e| CoreError::GlobPattern(e.to_string()))?
        .flatten()
        .collect();

    Ok(matches)
}

/// Search for pattern in files (respects .gitignore)
pub fn grep_files(input: GrepInput) -> CoreResult<Vec<GrepMatch>> {
    let max_matches = input.max_matches.unwrap_or(50);
    let max_line_len = input.max_line_len.unwrap_or(200);

    let regex = RegexBuilder::new(&input.pattern)
        .case_insensitive(input.case_insensitive)
        .build()
        .map_err(|e| CoreError::RegexInvalid(e.to_string()))?;

    let mut matches = Vec::new();

    // Walk directory respecting .gitignore
    let walker = WalkBuilder::new(&input.search_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        if matches.len() >= max_matches {
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

        // Skip binary files
        if is_binary_extension(path) {
            continue;
        }

        // Open and search file
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);
        let rel_path = path.strip_prefix(&input.search_path).unwrap_or(path);

        for (line_num, line) in reader.lines().enumerate() {
            if matches.len() >= max_matches {
                break;
            }

            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if regex.is_match(&line) {
                let display_line = if line.len() > max_line_len {
                    format!("{}...", &line[..max_line_len])
                } else {
                    line
                };

                matches.push(GrepMatch {
                    file: rel_path.display().to_string(),
                    line_number: line_num + 1,
                    content: display_line,
                });
            }
        }
    }

    Ok(matches)
}

/// Check if a file has a binary extension
fn is_binary_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "bmp" |
            "woff" | "woff2" | "ttf" | "eot" | "otf" |
            "pdf" | "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" |
            "exe" | "dll" | "so" | "dylib" | "o" | "a" | "lib" |
            "mp3" | "mp4" | "wav" | "ogg" | "webm" | "avi" | "mov" |
            "sqlite" | "db" | "bin" | "dat"
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_write_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        // Write
        let write_result = write_file(WriteFileInput {
            path: path.clone(),
            content: "Hello, World!".to_string(),
            create_dirs: true,
        }).await.unwrap();

        assert_eq!(write_result.bytes_written, 13);

        // Read
        let read_result = read_file(ReadFileInput {
            path: path.clone(),
            offset: None,
            limit: None,
        }).await.unwrap();

        assert_eq!(read_result.content, "Hello, World!");
        assert!(!read_result.truncated);
    }

    #[tokio::test]
    async fn test_edit_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        // Write initial
        write_file(WriteFileInput {
            path: path.clone(),
            content: "Hello, World!".to_string(),
            create_dirs: true,
        }).await.unwrap();

        // Edit
        let edit_result = edit_file(EditFileInput {
            path: path.clone(),
            old_string: "World".to_string(),
            new_string: "Rust".to_string(),
            replace_all: false,
        }).await.unwrap();

        assert!(edit_result.success);
        assert_eq!(edit_result.occurrences_replaced, 1);

        // Verify
        let read_result = read_file(ReadFileInput {
            path,
            offset: None,
            limit: None,
        }).await.unwrap();

        assert_eq!(read_result.content, "Hello, Rust!");
    }
}
