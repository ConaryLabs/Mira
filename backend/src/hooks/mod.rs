// src/hooks/mod.rs
// Hook system for pre/post tool execution - inspired by Claude Code's hooks

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// When the hook should run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTrigger {
    /// Before a tool is executed
    PreToolUse,
    /// After a tool executes successfully
    PostToolUse,
    /// Before a slash command is executed
    PreCommand,
    /// After a slash command executes
    PostCommand,
}

/// What to do when a hook fails
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    /// Block the tool/command from executing
    Block,
    /// Warn but allow execution to continue
    #[default]
    Warn,
    /// Silently ignore the failure
    Ignore,
}

/// A single hook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// Unique name for the hook
    pub name: String,
    /// When to trigger this hook
    pub trigger: HookTrigger,
    /// Tool name pattern to match (supports * wildcard)
    #[serde(default)]
    pub tool_pattern: Option<String>,
    /// Shell command to execute
    pub command: String,
    /// Working directory for the command
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Timeout in milliseconds (default: 60000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// What to do on failure
    #[serde(default)]
    pub on_failure: OnFailure,
    /// Whether this hook is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
}

fn default_timeout() -> u64 {
    60000
}

fn default_enabled() -> bool {
    true
}

impl Hook {
    /// Check if this hook matches the given tool name
    pub fn matches_tool(&self, tool_name: &str) -> bool {
        match &self.tool_pattern {
            None => true, // No pattern means match all
            Some(pattern) => {
                if pattern == "*" {
                    true
                } else if pattern.ends_with('*') {
                    let prefix = &pattern[..pattern.len() - 1];
                    tool_name.starts_with(prefix)
                } else if pattern.starts_with('*') {
                    let suffix = &pattern[1..];
                    tool_name.ends_with(suffix)
                } else {
                    tool_name == pattern
                }
            }
        }
    }
}

/// Result of executing a hook
#[derive(Debug, Clone)]
pub struct HookResult {
    pub hook_name: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
}

impl HookResult {
    /// Check if this result should block execution
    pub fn should_block(&self, on_failure: &OnFailure) -> bool {
        !self.success && matches!(on_failure, OnFailure::Block)
    }
}

/// Hook configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

/// Hook manager that loads and executes hooks
#[derive(Debug, Clone, Default)]
pub struct HookManager {
    hooks: Vec<Hook>,
    project_root: Option<PathBuf>,
}

impl HookManager {
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            project_root: None,
        }
    }

    /// Set the project root for relative paths
    pub fn with_project_root(mut self, root: PathBuf) -> Self {
        self.project_root = Some(root);
        self
    }

    /// Load hooks from configuration files
    pub async fn load(&mut self, project_root: Option<&Path>) -> Result<()> {
        self.hooks.clear();

        if let Some(root) = project_root {
            self.project_root = Some(root.to_path_buf());
        }

        // Load user-level hooks from ~/.mira/hooks.json
        if let Some(home) = dirs::home_dir() {
            let user_hooks = home.join(".mira").join("hooks.json");
            if user_hooks.exists() {
                self.load_from_file(&user_hooks).await?;
            }
        }

        // Load project-level hooks from .mira/hooks.json (override user hooks)
        if let Some(root) = &self.project_root {
            let project_hooks = root.join(".mira").join("hooks.json");
            if project_hooks.exists() {
                self.load_from_file(&project_hooks).await?;
            }
        }

        info!("Loaded {} hooks", self.hooks.len());
        Ok(())
    }

    /// Load hooks from a specific file
    async fn load_from_file(&mut self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path).await?;
        let config: HooksConfig = serde_json::from_str(&content)?;

        for hook in config.hooks {
            if hook.enabled {
                debug!("Loaded hook: {} ({:?})", hook.name, hook.trigger);
                self.hooks.push(hook);
            }
        }

        Ok(())
    }

    /// Get all hooks that match the given trigger and tool
    pub fn get_matching_hooks(&self, trigger: HookTrigger, tool_name: Option<&str>) -> Vec<&Hook> {
        self.hooks
            .iter()
            .filter(|h| h.trigger == trigger)
            .filter(|h| match tool_name {
                Some(name) => h.matches_tool(name),
                None => h.tool_pattern.is_none(),
            })
            .collect()
    }

    /// Execute a single hook
    pub async fn execute_hook(
        &self,
        hook: &Hook,
        env_vars: &HashMap<String, String>,
    ) -> HookResult {
        let start = std::time::Instant::now();

        // Build the command
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&hook.command);

        // Set working directory
        if let Some(cwd) = &hook.cwd {
            cmd.current_dir(cwd);
        } else if let Some(root) = &self.project_root {
            cmd.current_dir(root);
        }

        // Add environment variables
        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Execute with timeout
        let timeout_duration = Duration::from_millis(hook.timeout_ms);
        let result = timeout(timeout_duration, cmd.output()).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let exit_code = output.status.code();
                let success = output.status.success();
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if !success {
                    warn!(
                        "Hook '{}' failed with exit code {:?}: {}",
                        hook.name, exit_code, stderr
                    );
                } else {
                    debug!("Hook '{}' completed in {}ms", hook.name, duration_ms);
                }

                HookResult {
                    hook_name: hook.name.clone(),
                    success,
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    timed_out: false,
                }
            }
            Ok(Err(e)) => {
                error!("Hook '{}' execution error: {}", hook.name, e);
                HookResult {
                    hook_name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: e.to_string(),
                    duration_ms,
                    timed_out: false,
                }
            }
            Err(_) => {
                warn!("Hook '{}' timed out after {}ms", hook.name, hook.timeout_ms);
                HookResult {
                    hook_name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Hook timed out after {}ms", hook.timeout_ms),
                    duration_ms,
                    timed_out: true,
                }
            }
        }
    }

    /// Execute all matching hooks for a trigger
    /// Returns (should_continue, results)
    pub async fn execute_hooks(
        &self,
        trigger: HookTrigger,
        tool_name: Option<&str>,
        env_vars: &HashMap<String, String>,
    ) -> (bool, Vec<HookResult>) {
        let hooks = self.get_matching_hooks(trigger, tool_name);
        let mut results = Vec::new();
        let mut should_continue = true;

        for hook in hooks {
            let result = self.execute_hook(hook, env_vars).await;

            if result.should_block(&hook.on_failure) {
                should_continue = false;
                info!(
                    "Hook '{}' blocked execution: {}",
                    hook.name, result.stderr
                );
            }

            results.push(result);
        }

        (should_continue, results)
    }

    /// Reload hooks configuration
    pub async fn reload(&mut self) -> Result<()> {
        let project_root = self.project_root.clone();
        self.load(project_root.as_deref()).await
    }

    /// Get the number of loaded hooks
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Check if no hooks are loaded
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// List all hooks
    pub fn list(&self) -> &[Hook] {
        &self.hooks
    }
}

/// Environment variables set for hook execution
pub struct HookEnv;

impl HookEnv {
    /// Create environment variables for a tool execution hook
    pub fn for_tool(tool_name: &str, tool_args: &str) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("MIRA_TOOL_NAME".to_string(), tool_name.to_string());
        env.insert("MIRA_TOOL_ARGS".to_string(), tool_args.to_string());
        env
    }

    /// Create environment variables for a command execution hook
    pub fn for_command(command_name: &str, arguments: &str) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("MIRA_COMMAND_NAME".to_string(), command_name.to_string());
        env.insert("MIRA_COMMAND_ARGS".to_string(), arguments.to_string());
        env
    }

    /// Add tool result to environment (for PostToolUse)
    pub fn with_result(
        mut env: HashMap<String, String>,
        success: bool,
        output: &str,
    ) -> HashMap<String, String> {
        env.insert("MIRA_TOOL_SUCCESS".to_string(), success.to_string());
        env.insert("MIRA_TOOL_OUTPUT".to_string(), output.to_string());
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[test]
    fn test_hook_matches_tool() {
        let hook = Hook {
            name: "test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: Some("write_*".to_string()),
            command: "echo test".to_string(),
            cwd: None,
            timeout_ms: 1000,
            on_failure: OnFailure::Warn,
            enabled: true,
            description: None,
        };

        assert!(hook.matches_tool("write_file"));
        assert!(hook.matches_tool("write_project"));
        assert!(!hook.matches_tool("read_file"));
    }

    #[test]
    fn test_hook_matches_all() {
        let hook = Hook {
            name: "test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: None,
            command: "echo test".to_string(),
            cwd: None,
            timeout_ms: 1000,
            on_failure: OnFailure::Warn,
            enabled: true,
            description: None,
        };

        assert!(hook.matches_tool("any_tool"));
        assert!(hook.matches_tool("another"));
    }

    #[tokio::test]
    async fn test_load_hooks_config() {
        let temp_dir = TempDir::new().unwrap();
        let mira_dir = temp_dir.path().join(".mira");
        fs::create_dir_all(&mira_dir).await.unwrap();

        let hooks_json = mira_dir.join("hooks.json");
        fs::write(
            &hooks_json,
            r#"{
                "hooks": [
                    {
                        "name": "pre-write-test",
                        "trigger": "pre_tool_use",
                        "tool_pattern": "write_*",
                        "command": "echo 'Before write'",
                        "timeout_ms": 5000,
                        "on_failure": "block"
                    }
                ]
            }"#,
        )
        .await
        .unwrap();

        let mut manager = HookManager::new();
        manager.load(Some(temp_dir.path())).await.unwrap();

        assert_eq!(manager.len(), 1);
        let hooks = manager.get_matching_hooks(HookTrigger::PreToolUse, Some("write_file"));
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "pre-write-test");
    }

    #[tokio::test]
    async fn test_execute_simple_hook() {
        let hook = Hook {
            name: "echo-test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: None,
            command: "echo 'hello world'".to_string(),
            cwd: None,
            timeout_ms: 5000,
            on_failure: OnFailure::Warn,
            enabled: true,
            description: None,
        };

        let manager = HookManager::new();
        let env = HashMap::new();
        let result = manager.execute_hook(&hook, &env).await;

        assert!(result.success);
        assert!(result.stdout.contains("hello world"));
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_execute_failing_hook() {
        let hook = Hook {
            name: "fail-test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: None,
            command: "exit 1".to_string(),
            cwd: None,
            timeout_ms: 5000,
            on_failure: OnFailure::Block,
            enabled: true,
            description: None,
        };

        let manager = HookManager::new();
        let env = HashMap::new();
        let result = manager.execute_hook(&hook, &env).await;

        assert!(!result.success);
        assert!(result.should_block(&OnFailure::Block));
    }

    #[tokio::test]
    async fn test_hook_timeout() {
        let hook = Hook {
            name: "timeout-test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: None,
            command: "sleep 10".to_string(),
            cwd: None,
            timeout_ms: 100, // Very short timeout
            on_failure: OnFailure::Warn,
            enabled: true,
            description: None,
        };

        let manager = HookManager::new();
        let env = HashMap::new();
        let result = manager.execute_hook(&hook, &env).await;

        assert!(!result.success);
        assert!(result.timed_out);
    }

    #[tokio::test]
    async fn test_hook_with_env_vars() {
        let hook = Hook {
            name: "env-test".to_string(),
            trigger: HookTrigger::PreToolUse,
            tool_pattern: None,
            command: "echo $MIRA_TOOL_NAME".to_string(),
            cwd: None,
            timeout_ms: 5000,
            on_failure: OnFailure::Warn,
            enabled: true,
            description: None,
        };

        let manager = HookManager::new();
        let env = HookEnv::for_tool("write_file", r#"{"path": "test.txt"}"#);
        let result = manager.execute_hook(&hook, &env).await;

        assert!(result.success);
        assert!(result.stdout.contains("write_file"));
    }
}
