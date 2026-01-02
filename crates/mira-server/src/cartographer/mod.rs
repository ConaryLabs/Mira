// crates/mira-server/src/cartographer/mod.rs
// Codebase mapping and structure analysis

use crate::db::Database;
use anyhow::Result;
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

/// A logical module in the codebase
#[derive(Debug, Clone)]
pub struct Module {
    /// Unique identifier (e.g., "mcp/tools")
    pub id: String,
    /// Human-readable name (e.g., "tools")
    pub name: String,
    /// Directory path relative to project root
    pub path: String,
    /// Purpose summary (LLM-generated or heuristic)
    pub purpose: Option<String>,
    /// Key public exports
    pub exports: Vec<String>,
    /// Module IDs this depends on
    pub depends_on: Vec<String>,
    /// Symbol count
    pub symbol_count: u32,
    /// Line count
    pub line_count: u32,
}

/// Complete codebase map
#[derive(Debug, Clone)]
pub struct CodebaseMap {
    /// Project name
    pub name: String,
    /// Project type (rust/node/python/go)
    pub project_type: String,
    /// Detected modules
    pub modules: Vec<Module>,
    /// Entry points (main.rs, lib.rs, etc.)
    pub entry_points: Vec<String>,
    /// External dependencies
    pub external_deps: Vec<String>,
    /// When the map was last updated
    pub updated_at: String,
}

/// Detect Rust modules from project structure
pub fn detect_rust_modules(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_rust_modules: scanning {:?}", project_path);

    // Find all Cargo.toml files (workspace members)
    let cargo_tomls: Vec<_> = WalkDir::new(project_path)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "Cargo.toml")
        .filter(|e| e.path() != project_path.join("Cargo.toml") || !is_workspace(e.path()))
        .collect();

    tracing::info!("Found {} Cargo.toml files", cargo_tomls.len());

    for entry in cargo_tomls {
        let crate_root = entry.path().parent().unwrap_or(project_path);
        let crate_name = parse_crate_name(entry.path()).unwrap_or_else(|| {
            crate_root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        let src_dir = crate_root.join("src");
        if !src_dir.exists() {
            continue;
        }

        // Walk src directory looking for modules
        detect_modules_in_src(&src_dir, &crate_name, project_path, &mut modules);
    }

    // If no crates found, try the project root directly
    if modules.is_empty() {
        let src_dir = project_path.join("src");
        if src_dir.exists() {
            let crate_name = project_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string();
            detect_modules_in_src(&src_dir, &crate_name, project_path, &mut modules);
        }
    }

    tracing::info!("detect_rust_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

fn is_workspace(cargo_toml: &Path) -> bool {
    std::fs::read_to_string(cargo_toml)
        .map(|c| c.contains("[workspace]"))
        .unwrap_or(false)
}

fn parse_crate_name(cargo_toml: &Path) -> Option<String> {
    let content = std::fs::read_to_string(cargo_toml).ok()?;
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
    None
}

fn detect_modules_in_src(
    src_dir: &Path,
    crate_name: &str,
    project_path: &Path,
    modules: &mut Vec<Module>,
) {
    let mut seen_dirs: HashSet<String> = HashSet::new();

    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "target"
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative = path.strip_prefix(project_path).unwrap_or(path);

        if path.is_file() && path.extension().map_or(false, |e| e == "rs") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Entry points become top-level modules
            if file_name == "lib.rs" || file_name == "main.rs" {
                let module_path = relative.parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());
                    modules.push(Module {
                        id: crate_name.to_string(),
                        name: crate_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                    });
                }
            }
            // mod.rs indicates a module directory
            else if file_name == "mod.rs" {
                if let Some(parent) = path.parent() {
                    let module_name = parent.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    let module_path = parent.strip_prefix(project_path)
                        .unwrap_or(parent)
                        .to_string_lossy()
                        .to_string();

                    // Create module ID: crate_name/relative_module_path
                    let src_relative = parent.strip_prefix(src_dir).unwrap_or(parent);
                    let module_id = if src_relative.as_os_str().is_empty() {
                        crate_name.to_string()
                    } else {
                        format!("{}/{}", crate_name, src_relative.to_string_lossy())
                    };

                    if !seen_dirs.contains(&module_path) {
                        seen_dirs.insert(module_path.clone());
                        modules.push(Module {
                            id: module_id,
                            name: module_name.to_string(),
                            path: module_path,
                            purpose: None,
                            exports: vec![],
                            depends_on: vec![],
                            symbol_count: 0,
                            line_count: 0,
                        });
                    }
                }
            }
            // Regular .rs files in src/ are also modules
            else if path.parent() == Some(src_dir) && file_name != "lib.rs" && file_name != "main.rs" {
                let module_name = file_name.trim_end_matches(".rs");
                let module_id = format!("{}/{}", crate_name, module_name);
                let module_path = relative.to_string_lossy().to_string();

                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());
                    modules.push(Module {
                        id: module_id,
                        name: module_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                    });
                }
            }
        }
    }
}

/// Get or generate codebase map
pub fn get_or_generate_map(
    db: &Database,
    project_id: i64,
    project_path: &str,
    project_name: &str,
    project_type: &str,
) -> Result<CodebaseMap> {
    tracing::info!("get_or_generate_map: project_id={}, path={}", project_id, project_path);

    // Check if we have cached modules
    let cached_count: i64 = {
        let conn = db.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM codebase_modules WHERE project_id = ?",
            params![project_id],
            |row| row.get(0),
        )?
    }; // conn dropped here

    tracing::info!("Cached modules: {}", cached_count);

    if cached_count == 0 {
        // Generate fresh
        let path = Path::new(project_path);
        let modules = detect_rust_modules(path);

        // Enrich with database data and store
        let enriched = enrich_and_store_modules(db, project_id, modules, path)?;

        return Ok(CodebaseMap {
            name: project_name.to_string(),
            project_type: project_type.to_string(),
            modules: enriched,
            entry_points: find_entry_points(path),
            external_deps: get_external_deps(db, project_id)?,
            updated_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    // Load from cache
    let modules: Vec<Module> = {
        let conn = db.conn();
        let mut stmt = conn.prepare(
            "SELECT module_id, name, path, purpose, exports, depends_on, symbol_count, line_count
             FROM codebase_modules WHERE project_id = ? ORDER BY module_id"
        )?;

        stmt.query_map(params![project_id], |row| {
            let exports_json: Option<String> = row.get(4)?;
            let depends_json: Option<String> = row.get(5)?;

            Ok(Module {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                purpose: row.get(3)?,
                exports: exports_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                depends_on: depends_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                symbol_count: row.get(6)?,
                line_count: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    }; // conn dropped here

    Ok(CodebaseMap {
        name: project_name.to_string(),
        project_type: project_type.to_string(),
        modules,
        entry_points: find_entry_points(Path::new(project_path)),
        external_deps: get_external_deps(db, project_id)?,
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn enrich_and_store_modules(
    db: &Database,
    project_id: i64,
    mut modules: Vec<Module>,
    project_path: &Path,
) -> Result<Vec<Module>> {
    tracing::info!("enrich_and_store_modules: starting with {} modules", modules.len());
    let conn = db.conn();

    // First pass: collect exports, symbol counts, line counts, raw deps
    let mut raw_deps_per_module: Vec<Vec<String>> = Vec::with_capacity(modules.len());
    let total_modules = modules.len();

    for (i, module) in modules.iter_mut().enumerate() {
        tracing::debug!("Module {}/{}: {} (path={})", i + 1, total_modules, module.id, module.path);

        // Get exports (pub symbols in this module's path)
        let pattern = format!("{}%", module.path);
        let mut stmt = conn.prepare(
            "SELECT DISTINCT name FROM code_symbols
             WHERE project_id = ? AND file_path LIKE ?
             ORDER BY name LIMIT 20"
        )?;

        module.exports = stmt.query_map(params![project_id, pattern], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        tracing::debug!("  found {} exports", module.exports.len());

        // Get symbol count
        module.symbol_count = conn.query_row(
            "SELECT COUNT(*) FROM code_symbols WHERE project_id = ? AND file_path LIKE ?",
            params![project_id, pattern],
            |row| row.get(0),
        )?;
        tracing::debug!("  symbol_count: {}", module.symbol_count);

        // Get dependencies from imports
        let mut deps_stmt = conn.prepare(
            "SELECT DISTINCT import_path FROM imports
             WHERE project_id = ? AND file_path LIKE ? AND is_external = 0"
        )?;

        let raw_deps: Vec<String> = deps_stmt
            .query_map(params![project_id, pattern], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        tracing::debug!("  found {} deps", raw_deps.len());
        raw_deps_per_module.push(raw_deps);

        // Get line count from files
        tracing::debug!("  counting lines...");
        module.line_count = count_lines_in_module(project_path, &module.path);
        tracing::debug!("  line_count: {}", module.line_count);

        // Generate purpose heuristic
        if module.purpose.is_none() {
            module.purpose = generate_purpose_heuristic(&module.name, &module.exports);
        }
        tracing::debug!("  done with module");
    }

    // Second pass: resolve dependencies (needs immutable access to modules)
    // Create a snapshot of module IDs for dependency resolution
    let module_ids: Vec<(String, String)> = modules.iter()
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();

    for (i, module) in modules.iter_mut().enumerate() {
        module.depends_on = raw_deps_per_module[i].iter()
            .filter_map(|import| resolve_import_to_module(import, &module_ids))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Store in database
        let exports_json = serde_json::to_string(&module.exports)?;
        let depends_json = serde_json::to_string(&module.depends_on)?;

        conn.execute(
            "INSERT OR REPLACE INTO codebase_modules
             (project_id, module_id, name, path, purpose, exports, depends_on, symbol_count, line_count, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
            params![
                project_id,
                module.id,
                module.name,
                module.path,
                module.purpose,
                exports_json,
                depends_json,
                module.symbol_count,
                module.line_count
            ],
        )?;
    }

    Ok(modules)
}

fn resolve_import_to_module(import: &str, module_ids: &[(String, String)]) -> Option<String> {
    // Convert "crate::foo::bar" to check against module IDs
    let import = import
        .replace("crate::", "")
        .replace("super::", "")
        .replace("::", "/");

    // Find matching module
    for (id, name) in module_ids {
        if id.ends_with(&import) || import.starts_with(name) {
            return Some(id.clone());
        }
    }
    None
}

fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    let full_path = project_path.join(module_path);

    let mut count = 0u32;
    for entry in WalkDir::new(&full_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
    {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            count += content.lines().count() as u32;
        }
    }
    count
}

fn generate_purpose_heuristic(name: &str, exports: &[String]) -> Option<String> {
    // Check common module names
    let lower = name.to_lowercase();
    let purpose = match lower.as_str() {
        "db" | "database" => "Database operations and queries",
        "api" => "API endpoint handlers",
        "web" => "HTTP server and routes",
        "mcp" => "MCP protocol implementation",
        "models" | "types" => "Data type definitions",
        "utils" | "helpers" => "Utility functions",
        "auth" | "authentication" => "Authentication and authorization",
        "config" => "Configuration management",
        "handlers" => "Request/event handlers",
        "middleware" => "Middleware components",
        "routes" | "routing" => "Route definitions",
        "indexer" => "Code indexing and analysis",
        "embeddings" => "Vector embeddings",
        "cartographer" => "Codebase structure mapping",
        "hooks" => "Event hooks and callbacks",
        "tools" => "Tool implementations",
        "parsers" => "Code parsing",
        "tests" | "test" => "Test suites",
        _ => {
            // Try to infer from exports
            if exports.iter().any(|e| e.contains("Test") || e.contains("test")) {
                "Test utilities"
            } else if exports.iter().any(|e| e.contains("Error")) {
                "Error types and handling"
            } else if exports.iter().any(|e| e.contains("Config")) {
                "Configuration"
            } else {
                return None;
            }
        }
    };
    Some(purpose.to_string())
}

fn find_entry_points(project_path: &Path) -> Vec<String> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(project_path)
        .max_depth(5)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "target" && name != "node_modules"
        })
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy();
        if name == "main.rs" || name == "lib.rs" {
            if let Ok(rel) = entry.path().strip_prefix(project_path) {
                entries.push(rel.to_string_lossy().to_string());
            }
        }
    }

    entries.sort();
    entries
}

fn get_external_deps(db: &Database, project_id: i64) -> Result<Vec<String>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT import_path FROM imports
         WHERE project_id = ? AND is_external = 1
         ORDER BY import_path LIMIT 30"
    )?;

    let deps: Vec<String> = stmt
        .query_map(params![project_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(deps)
}

/// Format codebase map in compact text format
pub fn format_compact(map: &CodebaseMap) -> String {
    let mut output = String::new();

    // Group modules by top-level (crate name)
    let mut grouped: HashMap<String, Vec<&Module>> = HashMap::new();
    for module in &map.modules {
        let top = module.id.split('/').next().unwrap_or(&module.id);
        grouped.entry(top.to_string()).or_default().push(module);
    }

    for (crate_name, modules) in grouped.iter() {
        output.push_str(&format!("\n{}:\n", crate_name));

        for module in modules {
            // Skip if this is just the crate root
            if module.id == *crate_name {
                continue;
            }

            let purpose = module.purpose.as_deref().unwrap_or("");
            let deps = if module.depends_on.is_empty() {
                String::new()
            } else {
                let dep_names: Vec<_> = module.depends_on.iter()
                    .map(|d| d.split('/').last().unwrap_or(d))
                    .take(3)
                    .collect();
                format!(" -> {}", dep_names.join(", "))
            };

            output.push_str(&format!("  {} - {}{}\n", module.name, purpose, deps));
        }
    }

    if !map.entry_points.is_empty() {
        output.push_str(&format!("\nEntry: {}\n", map.entry_points.join(", ")));
    }

    output
}

// ═══════════════════════════════════════
// LLM-POWERED SUMMARIES
// ═══════════════════════════════════════

/// Context for a module to be summarized by LLM
#[derive(Debug, Clone)]
pub struct ModuleSummaryContext {
    pub module_id: String,
    pub name: String,
    pub path: String,
    pub exports: Vec<String>,
    pub code_preview: String,
    pub line_count: u32,
}

/// Get modules that need LLM summaries (no purpose or heuristic-only)
pub fn get_modules_needing_summaries(db: &Database, project_id: i64) -> Result<Vec<ModuleSummaryContext>> {
    let conn = db.conn();

    // Get modules without purposes or with generic heuristic purposes
    let mut stmt = conn.prepare(
        "SELECT module_id, name, path, exports, line_count
         FROM codebase_modules
         WHERE project_id = ? AND (purpose IS NULL OR purpose = '')"
    )?;

    let modules: Vec<ModuleSummaryContext> = stmt
        .query_map(params![project_id], |row| {
            let exports_json: Option<String> = row.get(3)?;
            Ok(ModuleSummaryContext {
                module_id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                exports: exports_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                code_preview: String::new(), // Filled in below
                line_count: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Read code preview for a module (first ~50 lines of key files)
pub fn get_module_code_preview(project_path: &Path, module_path: &str) -> String {
    let full_path = project_path.join(module_path);
    let mut preview = String::new();

    // Try mod.rs first, then lib.rs, then main.rs
    let candidates = ["mod.rs", "lib.rs", "main.rs"];

    for candidate in candidates {
        let file_path = if full_path.is_dir() {
            full_path.join(candidate)
        } else if full_path.extension().map_or(false, |e| e == "rs") {
            full_path.clone()
        } else {
            continue;
        };

        if file_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                // Take first 50 lines
                let lines: Vec<&str> = content.lines().take(50).collect();
                preview = lines.join("\n");
                break;
            }
        }
    }

    // If still empty, try to find any .rs file in the directory
    if preview.is_empty() && full_path.is_dir() {
        for entry in WalkDir::new(&full_path)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let lines: Vec<&str> = content.lines().take(50).collect();
                preview = lines.join("\n");
                break;
            }
        }
    }

    preview
}

/// Build prompt for summarizing multiple modules
pub fn build_summary_prompt(modules: &[ModuleSummaryContext]) -> String {
    let mut prompt = String::from(
        "Summarize each module's purpose in 1-2 sentences. Be specific about what it does and how it fits into the system.\n\n\
         Respond in this exact format (one line per module):\n\
         module_id: summary\n\n\
         Modules to summarize:\n\n"
    );

    for module in modules {
        prompt.push_str(&format!("--- {} ---\n", module.module_id));
        prompt.push_str(&format!("Name: {}\n", module.name));
        prompt.push_str(&format!("Lines: {}\n", module.line_count));

        if !module.exports.is_empty() {
            let exports_preview: Vec<_> = module.exports.iter().take(10).collect();
            prompt.push_str(&format!("Exports: {}\n", exports_preview.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }

        if !module.code_preview.is_empty() {
            prompt.push_str("Code preview:\n```rust\n");
            prompt.push_str(&module.code_preview);
            prompt.push_str("\n```\n");
        }

        prompt.push('\n');
    }

    prompt
}

/// Parse LLM response into module summaries
pub fn parse_summary_response(response: &str) -> HashMap<String, String> {
    let mut summaries = HashMap::new();

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }

        // Parse "module_id: summary" format
        if let Some((module_id, summary)) = line.split_once(':') {
            let module_id = module_id.trim().to_string();
            let summary = summary.trim().to_string();

            if !module_id.is_empty() && !summary.is_empty() {
                summaries.insert(module_id, summary);
            }
        }
    }

    summaries
}

/// Update module purposes in database
pub fn update_module_purposes(
    db: &Database,
    project_id: i64,
    summaries: &HashMap<String, String>,
) -> Result<usize> {
    let conn = db.conn();
    let mut updated = 0;

    for (module_id, purpose) in summaries {
        let rows = conn.execute(
            "UPDATE codebase_modules SET purpose = ?, updated_at = datetime('now')
             WHERE project_id = ? AND module_id = ?",
            params![purpose, project_id, module_id],
        )?;
        updated += rows;
    }

    Ok(updated)
}
