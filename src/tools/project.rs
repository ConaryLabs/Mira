// src/tools/project.rs
// Project management and guidelines tools

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::path::Path;

use super::types::*;

/// Detect project type from marker files
fn detect_project_type(path: &Path) -> Option<String> {
    let markers = [
        ("Cargo.toml", "rust"),
        ("package.json", "node"),
        ("pyproject.toml", "python"),
        ("setup.py", "python"),
        ("go.mod", "go"),
        ("Gemfile", "ruby"),
        ("pom.xml", "java"),
        ("build.gradle", "java"),
        ("CMakeLists.txt", "cpp"),
        ("Makefile", "make"),
    ];

    for (file, project_type) in markers {
        if path.join(file).exists() {
            return Some(project_type.to_string());
        }
    }
    None
}

/// Set the active project for this session
pub async fn set_project(db: &SqlitePool, req: SetProjectRequest) -> anyhow::Result<serde_json::Value> {
    // Canonicalize and validate the path
    let path = Path::new(&req.project_path);
    let canonical_path = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        std::env::current_dir()?.join(path).canonicalize()?
    };

    if !canonical_path.exists() {
        anyhow::bail!("Project path does not exist: {}", canonical_path.display());
    }

    let path_str = canonical_path.to_string_lossy().to_string();

    // Get name from request or directory name
    let name = req.name.unwrap_or_else(|| {
        canonical_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });

    // Detect project type
    let project_type = detect_project_type(&canonical_path);

    let now = Utc::now().timestamp();

    // Upsert the project
    sqlx::query(r#"
        INSERT INTO projects (path, name, project_type, first_seen, last_accessed)
        VALUES ($1, $2, $3, $4, $4)
        ON CONFLICT(path) DO UPDATE SET
            name = COALESCE(excluded.name, projects.name),
            project_type = COALESCE(excluded.project_type, projects.project_type),
            last_accessed = excluded.last_accessed
    "#)
    .bind(&path_str)
    .bind(&name)
    .bind(&project_type)
    .bind(now)
    .execute(db)
    .await?;

    // Get the project ID
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM projects WHERE path = $1")
        .bind(&path_str)
        .fetch_one(db)
        .await?;

    Ok(serde_json::json!({
        "status": "active",
        "id": id,
        "path": path_str,
        "name": name,
        "project_type": project_type,
        "message": format!("Project '{}' is now active. Memories and context will be scoped to this project.", name),
    }))
}

/// Get coding guidelines
pub async fn get_guidelines(db: &SqlitePool, req: GetGuidelinesRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let query = r#"
        SELECT id, content, category, project_path, priority,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM coding_guidelines
        WHERE ($1 IS NULL OR project_path = $1 OR project_path IS NULL)
          AND ($2 IS NULL OR category = $2)
        ORDER BY priority DESC, category, created_at
    "#;

    let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, String)>(query)
        .bind(&req.project_path)
        .bind(&req.category)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, content, category, project_path, priority, created_at)| {
            serde_json::json!({
                "id": id,
                "content": content,
                "category": category,
                "project_path": project_path,
                "priority": priority,
                "created_at": created_at,
            })
        })
        .collect())
}

/// Add a coding guideline
pub async fn add_guideline(db: &SqlitePool, req: AddGuidelineRequest) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let priority = req.priority.unwrap_or(0);

    let result = sqlx::query(r#"
        INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $5)
    "#)
    .bind(&req.content)
    .bind(&req.category)
    .bind(&req.project_path)
    .bind(priority)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "added",
        "id": result.last_insert_rowid(),
        "category": req.category,
        "content": req.content,
        "project_path": req.project_path,
        "priority": priority,
    }))
}
