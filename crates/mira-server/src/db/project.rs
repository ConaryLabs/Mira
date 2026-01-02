// db/project.rs
// Project management operations

use anyhow::Result;
use rusqlite::params;

use super::Database;

impl Database {
    /// Get or create project by path, returns (id, name)
    pub fn get_or_create_project(&self, path: &str, name: Option<&str>) -> Result<(i64, Option<String>)> {
        let conn = self.conn();

        // Try to find existing with its stored name
        let existing: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, name FROM projects WHERE path = ?",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((id, stored_name)) = existing {
            // Return stored name if we have one
            if stored_name.is_some() {
                return Ok((id, stored_name));
            }

            // No stored name - use caller's name or auto-detect
            let final_name = name.map(|s| s.to_string()).or_else(|| {
                Self::detect_project_name(path)
            });

            // Update the database with the detected name
            if final_name.is_some() {
                conn.execute(
                    "UPDATE projects SET name = ? WHERE id = ?",
                    params![&final_name, id],
                )?;
            }

            return Ok((id, final_name));
        }

        // Auto-detect name if not provided
        let detected_name = name.map(|s| s.to_string()).or_else(|| {
            Self::detect_project_name(path)
        });

        // Create new
        conn.execute(
            "INSERT INTO projects (path, name) VALUES (?, ?)",
            params![path, detected_name],
        )?;
        Ok((conn.last_insert_rowid(), detected_name))
    }

    /// Auto-detect project name from path
    fn detect_project_name(path: &str) -> Option<String> {
        use std::path::Path;

        let path = Path::new(path);
        let dir_name = || path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());

        // Try Cargo.toml for Rust projects
        let cargo_toml = path.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                // If it's a workspace, use directory name
                if content.contains("[workspace]") {
                    return dir_name();
                }

                // For single crate, find [package] section and get name
                let mut in_package = false;
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        in_package = line == "[package]";
                    } else if in_package && line.starts_with("name") {
                        if let Some(name) = line.split('=').nth(1) {
                            let name = name.trim().trim_matches('"').trim_matches('\'');
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Try package.json for Node projects
        let package_json = path.join("package.json");
        if package_json.exists() {
            if let Ok(content) = std::fs::read_to_string(&package_json) {
                // Simple JSON parsing for "name" field at top level
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with("\"name\"") {
                        if let Some(name) = line.split(':').nth(1) {
                            let name = name.trim().trim_matches(',').trim_matches('"').trim();
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Fall back to directory name
        dir_name()
    }

    /// Get database file path
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
}
