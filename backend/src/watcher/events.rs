// src/watcher/events.rs
// File change event types

use notify_debouncer_full::DebouncedEvent;
use std::path::PathBuf;

use super::config::{is_ignored_dir, should_process_extension};
use super::registry::WatchRegistry;

/// Type of file change
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
}

/// A processed file change event
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    /// Path to the changed file
    pub path: PathBuf,
    /// Type of change
    pub change_type: ChangeType,
    /// Attachment ID this file belongs to
    pub attachment_id: String,
    /// Project ID this file belongs to
    pub project_id: String,
    /// Relative path within the repository
    pub relative_path: String,
}

impl FileChangeEvent {
    /// Create a FileChangeEvent from a debounced notify event
    ///
    /// Returns None if:
    /// - The path is in an ignored directory
    /// - The file extension is not watched
    /// - The path doesn't belong to a watched repository
    pub fn from_debounced(event: &DebouncedEvent, registry: &WatchRegistry) -> Option<Self> {
        // Get the path from the event
        let path = event.paths.first()?.clone();

        // Check if path is in an ignored directory
        for component in path.components() {
            if let Some(name) = component.as_os_str().to_str() {
                if is_ignored_dir(name) {
                    return None;
                }
            }
        }

        // Check if file extension should be processed
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if !should_process_extension(ext) {
                return None;
            }
        } else {
            // No extension - skip
            return None;
        }

        // Find which repository this path belongs to
        let (attachment_id, project_id, base_path) = registry.find_repository_for_path(&path)?;

        // Calculate relative path
        let relative_path = path
            .strip_prefix(&base_path)
            .ok()?
            .to_string_lossy()
            .to_string();

        // Map notify event kind to our ChangeType
        let change_type = match event.kind {
            notify::EventKind::Create(_) => ChangeType::Created,
            notify::EventKind::Modify(_) => ChangeType::Modified,
            notify::EventKind::Remove(_) => ChangeType::Deleted,
            _ => return None, // Ignore other event types (access, etc.)
        };

        Some(Self {
            path,
            change_type,
            attachment_id,
            project_id,
            relative_path,
        })
    }
}
