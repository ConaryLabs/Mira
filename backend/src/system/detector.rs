// backend/src/system/detector.rs
// System environment detection logic

use super::types::*;
use std::collections::HashSet;
use std::process::Command;
use tracing::{debug, info};

/// System environment detector
pub struct SystemDetector;

impl SystemDetector {
    /// Detect system context (runs once at startup)
    pub fn detect() -> SystemContext {
        info!("Detecting system environment...");

        let os = Self::detect_os();
        let package_managers = Self::detect_package_managers();
        let shell = Self::detect_shell();
        let tools = Self::detect_tools();

        info!(
            "System detected: {} {} ({} shell, {} package managers, {} tools)",
            os.os_type,
            os.version,
            shell.name,
            package_managers.iter().filter(|pm| pm.available).count(),
            tools.len()
        );

        SystemContext {
            os,
            package_managers,
            shell,
            tools,
            detected_at: chrono::Utc::now(),
        }
    }

    /// Detect operating system information
    fn detect_os() -> OsInfo {
        let os_type = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();

        let version = match os_type.as_str() {
            "linux" => Self::detect_linux_distro(),
            "macos" => Self::detect_macos_version(),
            "windows" => Self::detect_windows_version(),
            _ => "Unknown".to_string(),
        };

        debug!("OS detected: {} {} ({})", os_type, version, arch);

        OsInfo {
            os_type,
            version,
            arch,
        }
    }

    /// Detect Linux distribution from /etc/os-release
    fn detect_linux_distro() -> String {
        // Try /etc/os-release first (most modern distros)
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    return line
                        .trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string();
                }
            }
        }

        // Fallback to lsb_release
        if let Ok(output) = Command::new("lsb_release").arg("-ds").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !version.is_empty() {
                    return version;
                }
            }
        }

        "Linux".to_string()
    }

    /// Detect macOS version
    fn detect_macos_version() -> String {
        if let Ok(output) = Command::new("sw_vers").arg("-productVersion").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return format!("macOS {}", version);
            }
        }
        "macOS".to_string()
    }

    /// Detect Windows version
    fn detect_windows_version() -> String {
        // Try to get Windows version from systeminfo
        if let Ok(output) = Command::new("cmd")
            .args(["/c", "ver"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !version.is_empty() {
                    return version;
                }
            }
        }
        "Windows".to_string()
    }

    /// Detect available package managers
    fn detect_package_managers() -> Vec<PackageManager> {
        let mut managers = vec![];

        // Package manager checks in priority order
        let checks: &[(&str, &[&str])] = &[
            ("apt", &["apt", "--version"]),
            ("dnf", &["dnf", "--version"]),
            ("yum", &["yum", "--version"]),
            ("pacman", &["pacman", "--version"]),
            ("zypper", &["zypper", "--version"]),
            ("brew", &["brew", "--version"]),
            ("nix", &["nix", "--version"]),
            ("chocolatey", &["choco", "--version"]),
            ("winget", &["winget", "--version"]),
            ("scoop", &["scoop", "--version"]),
        ];

        let mut found_primary = false;

        for (name, cmd) in checks {
            let available = Command::new(cmd[0])
                .args(&cmd[1..])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if available {
                let is_primary = !found_primary;
                if is_primary {
                    found_primary = true;
                    debug!("Primary package manager: {}", name);
                }

                managers.push(PackageManager {
                    name: name.to_string(),
                    available: true,
                    primary: is_primary,
                });
            }
        }

        managers
    }

    /// Detect default shell
    fn detect_shell() -> ShellInfo {
        // Check SHELL env var (Unix)
        if let Ok(shell_path) = std::env::var("SHELL") {
            let name = std::path::Path::new(&shell_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sh")
                .to_string();

            debug!("Shell detected: {} ({})", name, shell_path);

            return ShellInfo {
                name,
                path: Some(shell_path),
            };
        }

        // Windows fallback
        if cfg!(windows) {
            // Check if PowerShell is available
            if std::env::var("PSModulePath").is_ok() {
                return ShellInfo {
                    name: "powershell".to_string(),
                    path: None,
                };
            }
            return ShellInfo {
                name: "cmd".to_string(),
                path: None,
            };
        }

        ShellInfo {
            name: "sh".to_string(),
            path: None,
        }
    }

    /// Detect available CLI tools
    fn detect_tools() -> Vec<AvailableTool> {
        let tool_checks: &[(&str, &str)] = &[
            ("git", "git --version"),
            ("docker", "docker --version"),
            ("node", "node --version"),
            ("npm", "npm --version"),
            ("pnpm", "pnpm --version"),
            ("yarn", "yarn --version"),
            ("python", "python3 --version"),
            ("pip", "pip3 --version"),
            ("cargo", "cargo --version"),
            ("rustc", "rustc --version"),
            ("go", "go version"),
            ("java", "java --version"),
            ("make", "make --version"),
            ("cmake", "cmake --version"),
            ("gcc", "gcc --version"),
            ("clang", "clang --version"),
            ("kubectl", "kubectl version --client"),
            ("terraform", "terraform --version"),
        ];

        let mut tools = vec![];
        let mut seen: HashSet<&str> = HashSet::new();

        for (name, cmd) in tool_checks {
            if seen.contains(name) {
                continue;
            }

            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match Command::new(parts[0]).args(&parts[1..]).output() {
                Ok(output) if output.status.success() => {
                    // Extract first line as version
                    let version = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .map(|l| l.trim().to_string())
                        .filter(|v| !v.is_empty());

                    debug!("Tool found: {} ({:?})", name, version);

                    tools.push(AvailableTool {
                        name: name.to_string(),
                        version,
                    });
                    seen.insert(name);
                }
                Ok(_) => {
                    // Command exists but failed (not installed properly)
                }
                Err(_) => {
                    // Command not found
                }
            }
        }

        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_os() {
        let os = SystemDetector::detect_os();
        assert!(!os.os_type.is_empty());
        assert!(!os.arch.is_empty());
    }

    #[test]
    fn test_detect_shell() {
        let shell = SystemDetector::detect_shell();
        assert!(!shell.name.is_empty());
    }

    #[test]
    fn test_full_detection() {
        let context = SystemDetector::detect();
        assert!(!context.os.os_type.is_empty());
        assert!(!context.shell.name.is_empty());
        // Should have at least git on most dev machines
        // but don't assert this as it might fail in CI
    }
}
