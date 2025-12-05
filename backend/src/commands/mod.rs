// src/commands/mod.rs
// Custom slash commands system - inspired by Claude Code's .claude/commands/

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Scope of a slash command
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandScope {
    /// Project-specific command from .mira/commands/
    Project,
    /// User-global command from ~/.mira/commands/
    User,
}

/// A custom slash command loaded from a markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    /// Command name (e.g., "review", "git:pr")
    pub name: String,
    /// Path to the markdown file
    pub path: PathBuf,
    /// Raw markdown content
    pub content: String,
    /// Command scope
    pub scope: CommandScope,
    /// Optional description extracted from first line
    pub description: Option<String>,
}

impl SlashCommand {
    /// Execute the command by replacing $ARGUMENTS with the provided args
    pub fn execute(&self, arguments: &str) -> String {
        self.content.replace("$ARGUMENTS", arguments)
    }

    /// Check if this command has an $ARGUMENTS placeholder
    pub fn takes_arguments(&self) -> bool {
        self.content.contains("$ARGUMENTS")
    }
}

/// Registry for custom slash commands
#[derive(Debug, Clone, Default)]
pub struct CommandRegistry {
    commands: HashMap<String, SlashCommand>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Load commands from both user and project directories
    pub async fn load(&mut self, project_root: Option<&Path>) -> Result<()> {
        // Load user commands from ~/.mira/commands/
        if let Some(home) = dirs::home_dir() {
            let user_commands_dir = home.join(".mira").join("commands");
            if user_commands_dir.exists() {
                self.load_from_directory(&user_commands_dir, CommandScope::User, "")
                    .await?;
            }
        }

        // Load project commands from .mira/commands/ (these override user commands)
        if let Some(root) = project_root {
            let project_commands_dir = root.join(".mira").join("commands");
            if project_commands_dir.exists() {
                self.load_from_directory(&project_commands_dir, CommandScope::Project, "")
                    .await?;
            }
        }

        info!(
            "Loaded {} custom slash commands",
            self.commands.len()
        );

        Ok(())
    }

    /// Recursively load commands from a directory
    async fn load_from_directory(
        &mut self,
        dir: &Path,
        scope: CommandScope,
        prefix: &str,
    ) -> Result<()> {
        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if path.is_dir() {
                // Recurse into subdirectories for namespacing
                // e.g., commands/git/pr.md -> /git:pr
                let new_prefix = if prefix.is_empty() {
                    name_str.to_string()
                } else {
                    format!("{}:{}", prefix, name_str)
                };
                Box::pin(self.load_from_directory(&path, scope.clone(), &new_prefix)).await?;
            } else if path.extension().is_some_and(|ext| ext == "md") {
                // Load markdown file as command
                let command_name = if prefix.is_empty() {
                    name_str.trim_end_matches(".md").to_string()
                } else {
                    format!("{}:{}", prefix, name_str.trim_end_matches(".md"))
                };

                match self.load_command(&path, &command_name, scope.clone()).await {
                    Ok(cmd) => {
                        debug!("Loaded command /{} from {:?}", cmd.name, cmd.path);
                        self.commands.insert(cmd.name.clone(), cmd);
                    }
                    Err(e) => {
                        warn!("Failed to load command from {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single command from a markdown file
    async fn load_command(
        &self,
        path: &Path,
        name: &str,
        scope: CommandScope,
    ) -> Result<SlashCommand> {
        let content = fs::read_to_string(path).await?;

        // Extract description from first line if it starts with #
        let description = content
            .lines()
            .next()
            .filter(|line| line.starts_with('#'))
            .map(|line| line.trim_start_matches('#').trim().to_string());

        Ok(SlashCommand {
            name: name.to_string(),
            path: path.to_path_buf(),
            content,
            scope,
            description,
        })
    }

    /// Get a command by name
    pub fn get(&self, name: &str) -> Option<&SlashCommand> {
        self.commands.get(name)
    }

    /// List all available commands
    pub fn list(&self) -> Vec<&SlashCommand> {
        self.commands.values().collect()
    }

    /// Check if a message is a slash command and parse it
    pub fn parse_command(&self, message: &str) -> Option<(String, String)> {
        let trimmed = message.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        // Split into command and arguments
        let without_slash = &trimmed[1..];
        let parts: Vec<&str> = without_slash.splitn(2, char::is_whitespace).collect();

        let command_name = parts[0].to_string();
        let arguments = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();

        // Check if this command exists
        if self.commands.contains_key(&command_name) {
            Some((command_name, arguments))
        } else {
            None
        }
    }

    /// Execute a command by name with arguments
    pub fn execute(&self, name: &str, arguments: &str) -> Option<String> {
        self.commands.get(name).map(|cmd| cmd.execute(arguments))
    }

    /// Reload commands (useful for hot-reloading)
    pub async fn reload(&mut self, project_root: Option<&Path>) -> Result<()> {
        self.commands.clear();
        self.load(project_root).await
    }

    /// Register a command programmatically (for testing or built-in commands)
    pub fn register(&mut self, command: SlashCommand) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Get command count
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Response from executing a slash command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    /// The expanded prompt to send to the LLM
    pub prompt: String,
    /// The original command that was executed
    pub command: String,
    /// Arguments passed to the command
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_load_commands_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        let commands_dir = temp_dir.path().join(".mira").join("commands");
        fs::create_dir_all(&commands_dir).await.unwrap();

        // Create a test command
        let review_md = commands_dir.join("review.md");
        fs::write(&review_md, "# Review Code\n\nReview the following code:\n\n$ARGUMENTS")
            .await
            .unwrap();

        // Create a namespaced command
        let git_dir = commands_dir.join("git");
        fs::create_dir_all(&git_dir).await.unwrap();
        let pr_md = git_dir.join("pr.md");
        fs::write(&pr_md, "Create a PR with title: $ARGUMENTS")
            .await
            .unwrap();

        let mut registry = CommandRegistry::new();
        registry.load(Some(temp_dir.path())).await.unwrap();

        // Check that our commands were loaded (may also include user commands from ~/.mira)
        assert!(registry.len() >= 2);
        assert!(registry.get("review").is_some());
        assert!(registry.get("git:pr").is_some());

        // Test description extraction
        let review = registry.get("review").unwrap();
        assert_eq!(review.description, Some("Review Code".to_string()));
    }

    #[tokio::test]
    async fn test_execute_command() {
        let temp_dir = TempDir::new().unwrap();
        let commands_dir = temp_dir.path().join(".mira").join("commands");
        fs::create_dir_all(&commands_dir).await.unwrap();

        let test_md = commands_dir.join("test.md");
        fs::write(&test_md, "Run tests for: $ARGUMENTS")
            .await
            .unwrap();

        let mut registry = CommandRegistry::new();
        registry.load(Some(temp_dir.path())).await.unwrap();

        let result = registry.execute("test", "my_module");
        assert_eq!(result, Some("Run tests for: my_module".to_string()));
    }

    #[tokio::test]
    async fn test_parse_command() {
        let temp_dir = TempDir::new().unwrap();
        let commands_dir = temp_dir.path().join(".mira").join("commands");
        fs::create_dir_all(&commands_dir).await.unwrap();

        let help_md = commands_dir.join("help.md");
        fs::write(&help_md, "Show help for: $ARGUMENTS")
            .await
            .unwrap();

        let mut registry = CommandRegistry::new();
        registry.load(Some(temp_dir.path())).await.unwrap();

        // Valid command
        let parsed = registry.parse_command("/help authentication");
        assert_eq!(parsed, Some(("help".to_string(), "authentication".to_string())));

        // Command without args
        let parsed = registry.parse_command("/help");
        assert_eq!(parsed, Some(("help".to_string(), "".to_string())));

        // Non-existent command
        let parsed = registry.parse_command("/nonexistent foo");
        assert_eq!(parsed, None);

        // Not a command
        let parsed = registry.parse_command("hello world");
        assert_eq!(parsed, None);
    }
}
