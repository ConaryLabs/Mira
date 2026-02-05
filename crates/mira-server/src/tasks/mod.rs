// crates/mira-server/src/tasks/mod.rs
// Native Claude Code task file reader
//
// Reads task JSON files from ~/.claude/tasks/{list-id}/ directories.
// This is a pure filesystem reader — no database dependency.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A task as stored by Claude Code in ~/.claude/tasks/{list-id}/{id}.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTask {
    pub id: String,
    pub subject: String,
    pub description: Option<String>,
    pub active_form: Option<String>,
    pub status: String,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

/// Find the current task list directory.
///
/// Strategy:
/// 1. Check `CLAUDE_CODE_TASK_LIST_ID` env var → `~/.claude/tasks/{id}/`
/// 2. Check captured task list ID from SessionStart hook → `~/.mira/claude-task-list-id`
/// 3. Fallback: most recently modified directory in `~/.claude/tasks/`
/// 4. Return `None` if no task lists exist
pub fn find_current_task_list() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let tasks_dir = home.join(".claude/tasks");

    if !tasks_dir.is_dir() {
        return None;
    }

    // Strategy 1: env var
    if let Ok(list_id) = std::env::var("CLAUDE_CODE_TASK_LIST_ID") {
        let dir = tasks_dir.join(&list_id);
        if dir.is_dir() {
            return Some(dir);
        }
    }

    // Strategy 2: captured task list ID from SessionStart hook
    if let Some(list_id) = crate::hooks::session::read_claude_task_list_id() {
        let dir = tasks_dir.join(&list_id);
        if dir.is_dir() {
            return Some(dir);
        }
    }

    // Strategy 3: most recently modified directory
    let mut dirs: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(&tasks_dir)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();

    dirs.sort_by(|a, b| b.1.cmp(&a.1));
    dirs.into_iter().next().map(|(path, _)| path)
}

/// Read all tasks from a task list directory.
pub fn read_task_list(dir: &Path) -> Result<Vec<NativeTask>> {
    let mut tasks = Vec::new();

    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only read .json files (skip .lock etc.)
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<NativeTask>(&content) {
                Ok(task) => tasks.push(task),
                Err(e) => {
                    eprintln!(
                        "[mira] Failed to parse task file {}: {}",
                        path.display(),
                        e
                    );
                }
            },
            Err(e) => {
                eprintln!(
                    "[mira] Failed to read task file {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }

    // Sort by numeric ID for consistent ordering
    tasks.sort_by(|a, b| {
        let a_num: i64 = a.id.parse().unwrap_or(i64::MAX);
        let b_num: i64 = b.id.parse().unwrap_or(i64::MAX);
        a_num.cmp(&b_num)
    });

    Ok(tasks)
}

/// Read tasks and return only pending/in_progress ones.
pub fn get_pending_tasks(dir: &Path) -> Result<Vec<NativeTask>> {
    let tasks = read_task_list(dir)?;
    Ok(tasks
        .into_iter()
        .filter(|t| t.status == "pending" || t.status == "in_progress")
        .collect())
}

/// Count completed vs remaining tasks.
/// Returns (completed, remaining).
pub fn count_tasks(dir: &Path) -> Result<(usize, usize)> {
    let tasks = read_task_list(dir)?;
    let completed = tasks.iter().filter(|t| t.status == "completed").count();
    let remaining = tasks.len() - completed;
    Ok((completed, remaining))
}

/// Extract the task list ID (directory name) from a task list path.
pub fn task_list_id(dir: &Path) -> Option<String> {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_task(dir: &Path, id: &str, subject: &str, status: &str) {
        let task = serde_json::json!({
            "id": id,
            "subject": subject,
            "status": status,
            "blocks": [],
            "blockedBy": []
        });
        fs::write(dir.join(format!("{}.json", id)), task.to_string()).unwrap();
    }

    #[test]
    fn test_read_task_list_basic() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "1", "First task", "pending");
        write_task(dir.path(), "2", "Second task", "in_progress");
        write_task(dir.path(), "3", "Third task", "completed");

        let tasks = read_task_list(dir.path()).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "1");
        assert_eq!(tasks[0].subject, "First task");
        assert_eq!(tasks[1].status, "in_progress");
        assert_eq!(tasks[2].status, "completed");
    }

    #[test]
    fn test_read_task_list_skips_non_json() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "1", "Real task", "pending");
        fs::write(dir.path().join(".lock"), "").unwrap();
        fs::write(dir.path().join("notes.txt"), "not a task").unwrap();

        let tasks = read_task_list(dir.path()).unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn test_read_task_list_handles_malformed() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "1", "Good task", "pending");
        fs::write(dir.path().join("2.json"), "not valid json").unwrap();

        let tasks = read_task_list(dir.path()).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].subject, "Good task");
    }

    #[test]
    fn test_read_task_list_sorted_by_id() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "3", "Third", "pending");
        write_task(dir.path(), "1", "First", "pending");
        write_task(dir.path(), "2", "Second", "pending");

        let tasks = read_task_list(dir.path()).unwrap();
        assert_eq!(tasks[0].id, "1");
        assert_eq!(tasks[1].id, "2");
        assert_eq!(tasks[2].id, "3");
    }

    #[test]
    fn test_get_pending_tasks() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "1", "Pending", "pending");
        write_task(dir.path(), "2", "In progress", "in_progress");
        write_task(dir.path(), "3", "Done", "completed");

        let pending = get_pending_tasks(dir.path()).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|t| t.status != "completed"));
    }

    #[test]
    fn test_count_tasks() {
        let dir = TempDir::new().unwrap();
        write_task(dir.path(), "1", "Done 1", "completed");
        write_task(dir.path(), "2", "Done 2", "completed");
        write_task(dir.path(), "3", "Pending", "pending");
        write_task(dir.path(), "4", "In progress", "in_progress");

        let (completed, remaining) = count_tasks(dir.path()).unwrap();
        assert_eq!(completed, 2);
        assert_eq!(remaining, 2);
    }

    #[test]
    fn test_count_tasks_empty() {
        let dir = TempDir::new().unwrap();
        let (completed, remaining) = count_tasks(dir.path()).unwrap();
        assert_eq!(completed, 0);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_task_list_id() {
        let path = PathBuf::from("/home/user/.claude/tasks/abc-123");
        assert_eq!(task_list_id(&path), Some("abc-123".to_string()));
    }

    #[test]
    fn test_find_current_task_list_env_var() {
        // This test just verifies the function doesn't panic with no tasks dir
        // Full env var testing would require modifying env state
        let result = find_current_task_list();
        // Result depends on whether ~/.claude/tasks/ exists
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_deserialize_with_description() {
        let json = r#"{
            "id": "1",
            "subject": "Test task",
            "description": "A detailed description",
            "activeForm": "Testing things",
            "status": "pending",
            "blocks": ["2"],
            "blockedBy": []
        }"#;

        let task: NativeTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "1");
        assert_eq!(task.subject, "Test task");
        assert_eq!(task.description, Some("A detailed description".to_string()));
        assert_eq!(task.active_form, Some("Testing things".to_string()));
        assert_eq!(task.blocks, vec!["2"]);
        assert!(task.blocked_by.is_empty());
    }

    #[test]
    fn test_deserialize_minimal() {
        let json = r#"{
            "id": "1",
            "subject": "Minimal",
            "status": "pending"
        }"#;

        let task: NativeTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "1");
        assert!(task.description.is_none());
        assert!(task.active_form.is_none());
        assert!(task.blocks.is_empty());
        assert!(task.blocked_by.is_empty());
    }
}
