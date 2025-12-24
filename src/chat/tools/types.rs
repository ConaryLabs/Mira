//! Shared types for tool execution

use serde::{Deserialize, Serialize};

/// Diff information for file modifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub path: String,
    pub old_content: Option<String>,
    pub new_content: String,
    pub is_new_file: bool,
}

impl DiffInfo {
    /// Generate a unified diff string
    pub fn unified_diff(&self) -> String {
        if self.is_new_file {
            return self.format_new_file();
        }

        let old = self.old_content.as_deref().unwrap_or("");
        let new = &self.new_content;

        // Use similar crate for unified diff
        let diff = similar::TextDiff::from_lines(old, new);

        let mut output = String::new();
        output.push_str(&format!("--- a/{}\n", self.path));
        output.push_str(&format!("+++ b/{}\n", self.path));

        // Generate unified diff with context
        for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
            output.push_str(&hunk.to_string());
        }

        output
    }

    /// Format a new file diff
    fn format_new_file(&self) -> String {
        let mut output = String::new();
        output.push_str("--- /dev/null\n");
        output.push_str(&format!("+++ b/{}\n", self.path));
        output.push_str("@@ -0,0 +1 @@\n");

        for line in self.new_content.lines().take(20) {
            output.push_str(&format!("+{}\n", line));
        }

        let total_lines = self.new_content.lines().count();
        if total_lines > 20 {
            output.push_str(&format!("... ({} more lines)\n", total_lines - 20));
        }

        output
    }

    /// Check if there are actual changes
    pub fn has_changes(&self) -> bool {
        if self.is_new_file {
            return true;
        }
        match &self.old_content {
            Some(old) => old != &self.new_content,
            None => true,
        }
    }

    /// Get summary stats
    pub fn stats(&self) -> (usize, usize) {
        let old = self.old_content.as_deref().unwrap_or("");
        let new = &self.new_content;

        let diff = similar::TextDiff::from_lines(old, new);
        let mut added = 0;
        let mut removed = 0;

        for change in diff.iter_all_changes() {
            match change.tag() {
                similar::ChangeTag::Insert => added += 1,
                similar::ChangeTag::Delete => removed += 1,
                similar::ChangeTag::Equal => {}
            }
        }

        (added, removed)
    }
}

/// Rich tool result with diff information for file operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichToolResult {
    pub success: bool,
    pub output: String,
    pub diff: Option<DiffInfo>,
}
