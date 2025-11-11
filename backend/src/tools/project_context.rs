// src/tools/project_context.rs
// PHASE 3.1: Get complete project overview in one call
// Reduces 5-10 tool calls to 1 efficient operation
// FIXED: Proper JOINs through repository_files → git_repo_attachments

use anyhow::{Result, Context};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use sqlx::SqlitePool;
use tracing::info;
use walkdir::WalkDir;
use std::time::SystemTime;

/// Get complete project overview in one call
pub async fn get_project_context(project_id: &str, pool: &SqlitePool) -> Result<Value> {
    info!("Building complete project context for project_id: {}", project_id);
    
    // Get git attachment for local path
    let attachment = sqlx::query!(
        r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
        project_id
    )
    .fetch_one(pool)
    .await
    .context("Failed to find project git attachment")?;
    
    let local_path = Path::new(&attachment.local_path);
    
    // Build comprehensive context in parallel
    let (tree, recent_files, languages, code_elements) = tokio::try_join!(
        build_full_tree(local_path),
        get_recently_modified(local_path, 20),
        async { Ok::<_, anyhow::Error>(detect_languages(local_path).await) },
        count_code_elements(pool, project_id)
    )?;
    
    // Count total files
    let total_files = if let Some(tree_array) = tree.as_array() {
        tree_array.iter().filter(|item| {
            item.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false) == false
        }).count()
    } else {
        0
    };
    
    // Add total_files to code_stats
    let mut code_stats = code_elements.as_object().unwrap().clone();
    code_stats.insert("total_files".to_string(), json!(total_files));
    
    Ok(json!({
        "project_id": project_id,
        "local_path": attachment.local_path,
        "file_tree": tree,
        "recent_files": recent_files,
        "languages": languages,
        "code_stats": code_stats,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Build complete file tree structure (no depth limit)
async fn build_full_tree(path: &Path) -> Result<Value> {
    let mut tree = Vec::new();
    
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !is_ignored(e.path()))
    {
        let entry = entry?;
        let entry_path = entry.path();
        
        // Skip hidden files and common ignore patterns
        if entry_path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        
        let relative = entry_path.strip_prefix(path)?;
        tree.push(json!({
            "path": relative.display().to_string(),
            "is_dir": entry_path.is_dir(),
            "name": entry_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        }));
    }
    
    Ok(json!(tree))
}

/// Get recently modified files (last N files by mtime)
async fn get_recently_modified(path: &Path, limit: usize) -> Result<Vec<Value>> {
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
    
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !is_ignored(e.path()))
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Ok(metadata) = std::fs::metadata(entry_path) {
                if let Ok(modified) = metadata.modified() {
                    files.push((entry_path.to_path_buf(), modified));
                }
            }
        }
    }
    
    // Sort by modification time (newest first)
    files.sort_by(|a, b| b.1.cmp(&a.1));
    
    // Take top N
    let recent: Vec<Value> = files.into_iter()
        .take(limit)
        .map(|(path, _)| {
            json!({
                "path": path.display().to_string(),
                "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            })
        })
        .collect();
    
    Ok(recent)
}

/// Detect languages used in project
async fn detect_languages(path: &Path) -> HashMap<String, usize> {
    let mut languages: HashMap<String, usize> = HashMap::new();
    
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !is_ignored(e.path()))
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                let language = match ext {
                    "rs" => "Rust",
                    "ts" | "tsx" => "TypeScript",
                    "js" | "jsx" => "JavaScript",
                    "py" => "Python",
                    "go" => "Go",
                    "cpp" | "cc" | "cxx" => "C++",
                    "c" | "h" => "C",
                    "java" => "Java",
                    "rb" => "Ruby",
                    "php" => "PHP",
                    "cs" => "C#",
                    "swift" => "Swift",
                    "kt" | "kts" => "Kotlin",
                    "scala" => "Scala",
                    "sh" | "bash" => "Shell",
                    "sql" => "SQL",
                    "md" => "Markdown",
                    "json" => "JSON",
                    "yaml" | "yml" => "YAML",
                    "toml" => "TOML",
                    "xml" => "XML",
                    "html" => "HTML",
                    "css" | "scss" | "sass" => "CSS",
                    _ => continue,
                };
                
                *languages.entry(language.to_string()).or_insert(0) += 1;
            }
        }
    }
    
    languages
}

/// Count code elements by type with proper JOINs through repository_files
async fn count_code_elements(pool: &SqlitePool, project_id: &str) -> Result<Value> {
    // FIXED: Use proper JOINs to get to project_id
    let counts = sqlx::query!(
        r#"
        SELECT 
            ce.element_type,
            COUNT(*) as count
        FROM code_elements ce
        JOIN repository_files rf ON ce.file_id = rf.id
        JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
        WHERE gra.project_id = ?
        GROUP BY ce.element_type
        "#,
        project_id
    )
    .fetch_all(pool)
    .await?;
    
    let mut stats = HashMap::new();
    for row in counts {
        stats.insert(row.element_type, row.count);
    }
    
    // FIXED: Use complexity_score (not cyclomatic_complexity) and proper JOINs
    let complexity: Option<f64> = sqlx::query_scalar!(
        r#"
        SELECT AVG(ce.complexity_score) as "avg: f64"
        FROM code_elements ce
        JOIN repository_files rf ON ce.file_id = rf.id
        JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
        WHERE gra.project_id = ?
        "#,
        project_id
    )
    .fetch_optional(pool)
    .await?
    .flatten();
    
    // FIXED: Use proper JOINs for quality issues
    let issues_count: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) as "count: i64"
        FROM code_quality_issues cqi
        JOIN code_elements ce ON cqi.element_id = ce.id
        JOIN repository_files rf ON ce.file_id = rf.id
        JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
        WHERE gra.project_id = ?
        "#,
        project_id
    )
    .fetch_one(pool)
    .await?;
    
    Ok(json!({
        "elements": stats,
        "avg_complexity": complexity.unwrap_or(0.0),
        "quality_issues": issues_count,
    }))
}

/// Check if path should be ignored
fn is_ignored(path: &Path) -> bool {
    let ignored_patterns = [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "vendor",
        "__pycache__",
        ".cache",
        "coverage",
    ];
    
    path.components().any(|c| {
        if let Some(s) = c.as_os_str().to_str() {
            ignored_patterns.iter().any(|&pattern| s == pattern)
        } else {
            false
        }
    })
}
