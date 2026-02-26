// tools/core/project/detection.rs
// Project detection utilities: name, type, and system context gathering

use std::path::Path;
use std::process::Command;

/// Auto-detect project name from path (sync helper)
pub(super) fn detect_project_name(path: &str) -> Option<String> {
    let path = Path::new(path);
    let dir_name = || {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    };

    // Try Cargo.toml for Rust projects
    let cargo_toml = path.join("Cargo.toml");
    if cargo_toml.exists()
        && let Ok(content) = std::fs::read_to_string(&cargo_toml)
    {
        if content.contains("[workspace]") {
            return dir_name();
        }

        let mut in_package = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_package = line == "[package]";
            } else if in_package
                && line.starts_with("name")
                && let Some(name) = line.split('=').nth(1)
            {
                let name = name.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }

    // Try package.json for Node projects
    let package_json = path.join("package.json");
    if package_json.exists()
        && let Ok(contents) = std::fs::read_to_string(&package_json)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(name) = value["name"].as_str()
        && !name.is_empty()
    {
        return Some(name.to_string());
    }

    // Fall back to directory name
    dir_name()
}

/// Detect all project types from path (polyglot support).
///
/// Returns all detected languages based on manifest files present. A monorepo
/// with both `Cargo.toml` and `package.json` will return `["rust", "node"]`.
/// Returns `["unknown"]` if no known manifests are found.
pub fn detect_project_types(path: &str) -> Vec<&'static str> {
    let p = Path::new(path);
    let mut types = Vec::new();

    if p.join("Cargo.toml").exists() {
        types.push("rust");
    }
    if p.join("package.json").exists() {
        types.push("node");
    }
    if p.join("pyproject.toml").exists() || p.join("setup.py").exists() {
        types.push("python");
    }
    if p.join("go.mod").exists() {
        types.push("go");
    }
    // Java: detected but not yet supported for code intelligence.
    // pom.xml / build.gradle are recognized so we can warn rather than silently ignore.
    if p.join("pom.xml").exists() || p.join("build.gradle").exists() {
        types.push("java");
    }

    if types.is_empty() {
        types.push("unknown");
    }
    types
}

/// Detect primary project type from path.
///
/// For polyglot projects, returns the first (highest priority) language.
/// Use `detect_project_types` to get all detected languages.
pub fn detect_project_type(path: &str) -> &'static str {
    detect_project_types(path)
        .into_iter()
        .next()
        .unwrap_or("unknown")
}

/// Gather system context content for bash tool usage (returns content string, does not store)
pub(super) fn gather_system_context_content() -> Option<String> {
    let mut context_parts = Vec::new();

    // OS info
    if let Ok(output) = Command::new("uname").args(["-s", "-r"]).output()
        && output.status.success()
    {
        let os = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("OS: {}", os));
    }

    // Distro (Linux)
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if line.starts_with("PRETTY_NAME=") {
                let name = line.trim_start_matches("PRETTY_NAME=").trim_matches('"');
                context_parts.push(format!("Distro: {}", name));
                break;
            }
        }
    }

    // Shell
    if let Ok(shell) = std::env::var("SHELL") {
        context_parts.push(format!("Shell: {}", shell));
    }

    // User (try env, fallback to whoami)
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            context_parts.push(format!("User: {}", user));
        }
    } else if let Ok(output) = Command::new("whoami").output()
        && output.status.success()
    {
        let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("User: {}", user));
    }

    // Home directory (try env, fallback to ~)
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            context_parts.push(format!("Home: {}", home));
        }
    } else if let Ok(output) = Command::new("sh").args(["-c", "echo ~"]).output()
        && output.status.success()
    {
        let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("Home: {}", home));
    }

    // Timezone
    if let Ok(output) = Command::new("date").arg("+%Z (UTC%:z)").output()
        && output.status.success()
    {
        let tz = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("Timezone: {}", tz));
    }

    // Available tools (check common ones via PATH scan)
    let tools_to_check = [
        "git",
        "cargo",
        "rustc",
        "npm",
        "node",
        "python3",
        "docker",
        "systemctl",
        "curl",
        "jq",
    ];
    if let Ok(path_var) = std::env::var("PATH") {
        let path_dirs: Vec<std::path::PathBuf> = std::env::split_paths(&path_var).collect();
        let found: Vec<&str> = tools_to_check
            .iter()
            .filter(|tool| path_dirs.iter().any(|dir| dir.join(tool).is_file()))
            .copied()
            .collect();
        if !found.is_empty() {
            context_parts.push(format!("Available tools: {}", found.join(", ")));
        }
    }

    if context_parts.is_empty() {
        None
    } else {
        Some(context_parts.join("\n"))
    }
}
