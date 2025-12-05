// backend/src/system/types.rs
// System context types for platform-aware LLM responses

use serde::{Deserialize, Serialize};

/// Complete system environment context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemContext {
    /// Operating system details
    pub os: OsInfo,
    /// Available package managers
    pub package_managers: Vec<PackageManager>,
    /// Default shell
    pub shell: ShellInfo,
    /// Available CLI tools
    pub tools: Vec<AvailableTool>,
    /// Detection timestamp
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

/// Operating system information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    /// OS type: linux, macos, windows
    pub os_type: String,
    /// Distribution/version: "Ubuntu 22.04", "macOS Sonoma 14.2", "Windows 11"
    pub version: String,
    /// Architecture: x86_64, aarch64
    pub arch: String,
}

/// Package manager information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManager {
    /// Name: apt, brew, dnf, pacman, chocolatey, winget
    pub name: String,
    /// Whether it's available on this system
    pub available: bool,
    /// Whether it's the primary/recommended package manager
    pub primary: bool,
}

/// Shell information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellInfo {
    /// Shell name: bash, zsh, fish, powershell, cmd
    pub name: String,
    /// Shell path (if available)
    pub path: Option<String>,
}

/// Available CLI tool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableTool {
    /// Tool name: git, docker, node, python, cargo, etc.
    pub name: String,
    /// Version string if detectable
    pub version: Option<String>,
}

impl SystemContext {
    /// Get the primary package manager name, if any
    pub fn primary_package_manager(&self) -> Option<&str> {
        self.package_managers
            .iter()
            .find(|pm| pm.primary)
            .map(|pm| pm.name.as_str())
    }

    /// Check if a specific tool is available
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name)
    }

    /// Get tool version if available
    pub fn tool_version(&self, name: &str) -> Option<&str> {
        self.tools
            .iter()
            .find(|t| t.name == name)
            .and_then(|t| t.version.as_deref())
    }
}
