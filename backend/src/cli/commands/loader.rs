// backend/src/cli/commands/loader.rs
// Load custom commands from ~/.mira/commands/ and .mira/commands/

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A loaded custom command
#[derive(Debug, Clone)]
pub struct CustomCommand {
    /// Command name (without the leading /)
    pub name: String,
    /// Description (first line of the markdown file)
    pub description: String,
    /// Full prompt template
    pub template: String,
    /// Source path
    pub source: PathBuf,
    /// Whether this accepts arguments (contains $ARGUMENTS placeholder)
    pub accepts_args: bool,
}

/// Loads custom commands from markdown files
pub struct CommandLoader {
    /// Loaded commands by name
    commands: HashMap<String, CustomCommand>,
}

impl CommandLoader {
    /// Create a new command loader and scan for commands
    pub fn new() -> Result<Self> {
        let mut loader = Self {
            commands: HashMap::new(),
        };
        loader.scan_all_directories()?;
        Ok(loader)
    }

    /// Scan all command directories
    fn scan_all_directories(&mut self) -> Result<()> {
        // Global commands from ~/.mira/commands/
        if let Some(home) = dirs::home_dir() {
            let global_dir = home.join(".mira").join("commands");
            if global_dir.exists() {
                self.scan_directory(&global_dir)?;
            }
        }

        // Project-local commands from .mira/commands/
        let local_dir = PathBuf::from(".mira").join("commands");
        if local_dir.exists() {
            self.scan_directory(&local_dir)?;
        }

        Ok(())
    }

    /// Scan a directory for command files
    fn scan_directory(&mut self, dir: &Path) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                if let Some(cmd) = self.load_command(&path)? {
                    self.commands.insert(cmd.name.clone(), cmd);
                }
            }
        }

        Ok(())
    }

    /// Load a command from a markdown file
    fn load_command(&self, path: &Path) -> Result<Option<CustomCommand>> {
        let content = std::fs::read_to_string(path)?;

        // Get command name from filename (without .md extension)
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if name.is_empty() {
            return Ok(None);
        }

        // First non-empty line is the description
        let description = content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .trim_start_matches('#')
            .trim()
            .to_string();

        // Check if template accepts arguments
        let accepts_args = content.contains("$ARGUMENTS")
            || content.contains("{{arguments}}")
            || content.contains("{arguments}");

        Ok(Some(CustomCommand {
            name,
            description,
            template: content,
            source: path.to_path_buf(),
            accepts_args,
        }))
    }

    /// Get a command by name
    pub fn get(&self, name: &str) -> Option<&CustomCommand> {
        self.commands.get(name)
    }

    /// List all available commands
    pub fn list(&self) -> Vec<&CustomCommand> {
        let mut cmds: Vec<_> = self.commands.values().collect();
        cmds.sort_by(|a, b| a.name.cmp(&b.name));
        cmds
    }

    /// Expand a command template with arguments
    pub fn expand(&self, name: &str, args: Option<&str>) -> Option<String> {
        let cmd = self.commands.get(name)?;
        let mut expanded = cmd.template.clone();

        // Replace argument placeholders
        let args_value = args.unwrap_or("");
        expanded = expanded.replace("$ARGUMENTS", args_value);
        expanded = expanded.replace("{{arguments}}", args_value);
        expanded = expanded.replace("{arguments}", args_value);

        Some(expanded)
    }

    /// Check if a command exists
    pub fn exists(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// Reload commands (useful after project change)
    pub fn reload(&mut self) -> Result<()> {
        self.commands.clear();
        self.scan_all_directories()
    }
}

impl Default for CommandLoader {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            commands: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_command() {
        let temp_dir = TempDir::new().unwrap();
        let cmd_file = temp_dir.path().join("test-cmd.md");
        std::fs::write(
            &cmd_file,
            "# Test Command\n\nThis is a test command that accepts $ARGUMENTS.",
        )
        .unwrap();

        let loader = CommandLoader {
            commands: HashMap::new(),
        };
        let cmd = loader.load_command(&cmd_file).unwrap().unwrap();

        assert_eq!(cmd.name, "test-cmd");
        assert_eq!(cmd.description, "Test Command");
        assert!(cmd.accepts_args);
    }

    #[test]
    fn test_expand_command() {
        let mut loader = CommandLoader {
            commands: HashMap::new(),
        };

        let cmd = CustomCommand {
            name: "greet".to_string(),
            description: "Greet someone".to_string(),
            template: "Say hello to $ARGUMENTS".to_string(),
            source: PathBuf::new(),
            accepts_args: true,
        };

        loader.commands.insert("greet".to_string(), cmd);

        let expanded = loader.expand("greet", Some("World")).unwrap();
        assert_eq!(expanded, "Say hello to World");
    }
}
