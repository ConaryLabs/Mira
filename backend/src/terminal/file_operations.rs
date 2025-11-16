// backend/src/terminal/file_operations.rs

use super::types::*;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info};

/// Handles file operations on the local machine
pub struct FileOperations {
    /// Base directory for file operations (for security)
    base_dir: Option<PathBuf>,
}

impl FileOperations {
    /// Create a new file operations handler
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        Self { base_dir }
    }

    /// Read the contents of a file
    pub async fn read_file(&self, path: impl AsRef<Path>) -> TerminalResult<String> {
        let path = self.resolve_path(path.as_ref())?;
        info!("Reading file: {}", path.display());

        let mut file = fs::File::open(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to open file: {}", e))
        })?;

        let mut contents = String::new();
        file.read_to_string(&mut contents).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to read file: {}", e))
        })?;

        Ok(contents)
    }

    /// Read file as bytes
    pub async fn read_file_bytes(&self, path: impl AsRef<Path>) -> TerminalResult<Vec<u8>> {
        let path = self.resolve_path(path.as_ref())?;
        debug!("Reading file bytes: {}", path.display());

        fs::read(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to read file: {}", e))
        })
    }

    /// Write contents to a file
    pub async fn write_file(&self, path: impl AsRef<Path>, content: &str) -> TerminalResult<()> {
        let path = self.resolve_path(path.as_ref())?;
        info!("Writing file: {}", path.display());

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                TerminalError::FileOperationFailed(format!("Failed to create directories: {}", e))
            })?;
        }

        let mut file = fs::File::create(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to create file: {}", e))
        })?;

        file.write_all(content.as_bytes()).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to write file: {}", e))
        })?;

        Ok(())
    }

    /// Write bytes to a file
    pub async fn write_file_bytes(&self, path: impl AsRef<Path>, content: &[u8]) -> TerminalResult<()> {
        let path = self.resolve_path(path.as_ref())?;
        debug!("Writing file bytes: {}", path.display());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&path, content).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to write file: {}", e))
        })
    }

    /// List files in a directory
    pub async fn list_directory(&self, path: impl AsRef<Path>) -> TerminalResult<Vec<FileInfo>> {
        let path = self.resolve_path(path.as_ref())?;
        info!("Listing directory: {}", path.display());

        let mut entries = fs::read_dir(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to read directory: {}", e))
        })?;

        let mut files = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to read directory entry: {}", e))
        })? {
            let metadata = entry.metadata().await.map_err(|e| {
                TerminalError::FileOperationFailed(format!("Failed to read metadata: {}", e))
            })?;

            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();
            let path_str = entry.path().to_string_lossy().to_string();

            let is_hidden = name.starts_with('.');

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                })
                .flatten();

            let permissions = Self::get_permissions_string(&metadata);

            files.push(FileInfo {
                path: path_str,
                name,
                is_directory: metadata.is_dir(),
                size: metadata.len(),
                modified,
                permissions,
                is_hidden,
            });
        }

        // Sort: directories first, then by name
        files.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        Ok(files)
    }

    /// Check if a path exists
    pub async fn path_exists(&self, path: impl AsRef<Path>) -> TerminalResult<bool> {
        let path = self.resolve_path(path.as_ref())?;
        Ok(path.exists())
    }

    /// Check if a path is a directory
    pub async fn is_directory(&self, path: impl AsRef<Path>) -> TerminalResult<bool> {
        let path = self.resolve_path(path.as_ref())?;
        let metadata = fs::metadata(&path).await?;
        Ok(metadata.is_dir())
    }

    /// Create a directory
    pub async fn create_directory(&self, path: impl AsRef<Path>) -> TerminalResult<()> {
        let path = self.resolve_path(path.as_ref())?;
        info!("Creating directory: {}", path.display());

        fs::create_dir_all(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to create directory: {}", e))
        })
    }

    /// Delete a file or directory
    pub async fn delete(&self, path: impl AsRef<Path>, recursive: bool) -> TerminalResult<()> {
        let path = self.resolve_path(path.as_ref())?;
        info!("Deleting: {} (recursive: {})", path.display(), recursive);

        let metadata = fs::metadata(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Path not found: {}", e))
        })?;

        if metadata.is_dir() {
            if recursive {
                fs::remove_dir_all(&path).await
            } else {
                fs::remove_dir(&path).await
            }
        } else {
            fs::remove_file(&path).await
        }
        .map_err(|e| TerminalError::FileOperationFailed(format!("Failed to delete: {}", e)))
    }

    /// Get file metadata
    pub async fn get_metadata(&self, path: impl AsRef<Path>) -> TerminalResult<FileInfo> {
        let path = self.resolve_path(path.as_ref())?;
        debug!("Getting metadata for: {}", path.display());

        let metadata = fs::metadata(&path).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to get metadata: {}", e))
        })?;

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let path_str = path.to_string_lossy().to_string();
        let is_hidden = name.starts_with('.');

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            })
            .flatten();

        let permissions = Self::get_permissions_string(&metadata);

        Ok(FileInfo {
            path: path_str,
            name,
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            modified,
            permissions,
            is_hidden,
        })
    }

    /// Copy a file
    pub async fn copy_file(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> TerminalResult<()> {
        let from = self.resolve_path(from.as_ref())?;
        let to = self.resolve_path(to.as_ref())?;
        info!("Copying file: {} -> {}", from.display(), to.display());

        fs::copy(&from, &to).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to copy file: {}", e))
        })?;

        Ok(())
    }

    /// Move/rename a file
    pub async fn move_file(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> TerminalResult<()> {
        let from = self.resolve_path(from.as_ref())?;
        let to = self.resolve_path(to.as_ref())?;
        info!("Moving file: {} -> {}", from.display(), to.display());

        fs::rename(&from, &to).await.map_err(|e| {
            TerminalError::FileOperationFailed(format!("Failed to move file: {}", e))
        })
    }

    /// Resolve a path relative to base directory
    fn resolve_path(&self, path: &Path) -> TerminalResult<PathBuf> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(base) = &self.base_dir {
            base.join(path)
        } else {
            std::env::current_dir()
                .map_err(|e| TerminalError::IoError(e))?
                .join(path)
        };

        // Security check: ensure path doesn't escape base_dir
        if let Some(base) = &self.base_dir {
            let canonical_base = base.canonicalize().ok();
            let canonical_resolved = resolved.canonicalize().ok();

            if let (Some(base), Some(resolved)) = (canonical_base, canonical_resolved) {
                if !resolved.starts_with(&base) {
                    return Err(TerminalError::PermissionDenied(
                        "Path escapes base directory".to_string(),
                    ));
                }
            }
        }

        Ok(resolved)
    }

    /// Get permissions string from metadata
    #[cfg(unix)]
    fn get_permissions_string(metadata: &std::fs::Metadata) -> Option<String> {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        Some(format!("{:o}", mode & 0o777))
    }

    #[cfg(not(unix))]
    fn get_permissions_string(_metadata: &std::fs::Metadata) -> Option<String> {
        None
    }
}

impl Default for FileOperations {
    fn default() -> Self {
        Self::new(None)
    }
}
