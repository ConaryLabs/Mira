//! Core session operations - shared by MCP and Chat
//!
//! Session storage, search, and initialization.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use crate::core::primitives::semantic::COLLECTION_CONVERSATION;

use super::super::{CoreResult, OpContext};
use super::work_state::{get_active_plan, get_active_todos, get_working_docs, ActivePlan, WorkingDoc, WorkStateTodo};

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct StoreSessionInput {
    pub session_id: Option<String>,
    pub summary: String,
    pub project_path: Option<String>,
    pub topics: Option<Vec<String>>,
    pub project_id: Option<i64>,
}

pub struct StoreSessionOutput {
    pub session_id: String,
    pub project_id: Option<i64>,
    pub semantic_search: bool,
}

pub struct SearchSessionsInput {
    pub query: String,
    pub limit: usize,
    pub project_id: Option<i64>,
}

pub struct SessionSearchResult {
    pub content: String,
    pub score: f32,
    pub session_id: Option<String>,
    pub project_id: Option<i64>,
    pub search_type: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

pub struct SessionStartInput {
    pub project_path: String,
    pub name: Option<String>,
}

pub struct SessionStartOutput {
    pub project_id: i64,
    pub project_name: String,
    pub project_path: String,
    pub project_type: Option<String>,
    pub usage_guidelines_loaded: usize,
    pub corrections: Vec<CorrectionSummary>,
    pub goals: Vec<GoalSummary>,
    pub tasks: Vec<TaskSummary>,
    pub recent_session_topics: Vec<String>,
    pub active_todos: Option<Vec<WorkStateTodo>>,
    pub active_plan: Option<ActivePlan>,
    pub working_docs: Vec<WorkingDoc>,
    pub index_fresh: bool,
    pub stale_file_count: usize,
}

#[derive(Debug, Clone)]
pub struct CorrectionSummary {
    pub what_was_wrong: String,
    pub what_is_right: String,
}

#[derive(Debug, Clone)]
pub struct GoalSummary {
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

#[derive(Debug, Clone)]
pub struct TaskSummary {
    pub title: String,
    pub status: String,
}

// ============================================================================
// Operations
// ============================================================================

/// Store a session summary
pub async fn store_session(ctx: &OpContext, input: StoreSessionInput) -> CoreResult<StoreSessionOutput> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let session_id = input.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in SQLite
    sqlx::query(r#"
        INSERT INTO memory_entries (id, session_id, role, content, created_at, project_id)
        VALUES ($1, $2, 'session_summary', $3, $4, $5)
        ON CONFLICT(id) DO UPDATE SET
            content = excluded.content,
            created_at = excluded.created_at,
            project_id = COALESCE(excluded.project_id, memory_entries.project_id)
    "#)
    .bind(&session_id)
    .bind(&session_id)
    .bind(&input.summary)
    .bind(now)
    .bind(input.project_id)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search
    let mut semantic_stored = false;
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let mut metadata = HashMap::new();
            metadata.insert("session_id".to_string(), serde_json::Value::String(session_id.clone()));
            metadata.insert("type".to_string(), serde_json::Value::String("session_summary".to_string()));
            metadata.insert("timestamp".to_string(), serde_json::Value::Number(now.into()));

            if let Some(pid) = input.project_id {
                metadata.insert("project_id".to_string(), serde_json::Value::Number(pid.into()));
            }

            if let Some(ref project) = input.project_path {
                metadata.insert("project_path".to_string(), serde_json::Value::String(project.clone()));
            }

            if let Some(ref topics) = input.topics {
                metadata.insert("topics".to_string(), serde_json::Value::String(topics.join(",")));
            }

            let _ = semantic.ensure_collection(COLLECTION_CONVERSATION).await;
            if semantic.store(COLLECTION_CONVERSATION, &session_id, &input.summary, metadata).await.is_ok() {
                semantic_stored = true;
            }
        }
    }

    Ok(StoreSessionOutput {
        session_id,
        project_id: input.project_id,
        semantic_search: semantic_stored,
    })
}

/// Search past sessions using semantic similarity
pub async fn search_sessions(ctx: &OpContext, input: SearchSessionsInput) -> CoreResult<Vec<SessionSearchResult>> {
    let db = ctx.require_db()?;

    // Try semantic search first
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let filter = if let Some(pid) = input.project_id {
                Some(qdrant_client::qdrant::Filter::must([
                    qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
                    qdrant_client::qdrant::Condition::matches("project_id", pid),
                ]))
            } else {
                Some(qdrant_client::qdrant::Filter::must([
                    qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
                ]))
            };

            if let Ok(results) = semantic.search(COLLECTION_CONVERSATION, &input.query, input.limit, filter).await {
                return Ok(results.into_iter().map(|r| {
                    SessionSearchResult {
                        content: r.content,
                        score: r.score,
                        session_id: r.metadata.get("session_id").and_then(|v| v.as_str()).map(String::from),
                        project_id: r.metadata.get("project_id").and_then(|v| v.as_i64()),
                        search_type: "semantic".to_string(),
                        metadata: r.metadata,
                    }
                }).collect());
            }
        }
    }

    // Fallback to SQLite text search
    let query = r#"
        SELECT id, session_id, content,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               project_id
        FROM memory_entries
        WHERE role = 'session_summary'
          AND content LIKE '%' || $1 || '%'
          AND (project_id IS NULL OR $2 IS NULL OR project_id = $2)
        ORDER BY created_at DESC
        LIMIT $3
    "#;

    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>)>(query)
        .bind(&input.query)
        .bind(input.project_id)
        .bind(input.limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, session_id, content, created_at, proj_id)| {
        let mut metadata = HashMap::new();
        metadata.insert("id".to_string(), serde_json::Value::String(id));
        metadata.insert("created_at".to_string(), serde_json::Value::String(created_at));

        SessionSearchResult {
            content,
            score: 1.0,
            session_id: Some(session_id),
            project_id: proj_id,
            search_type: "text".to_string(),
            metadata,
        }
    }).collect())
}

/// Combined session startup
/// This is a multi-step operation that checks for cancellation between steps
pub async fn session_start(ctx: &OpContext, input: SessionStartInput) -> CoreResult<SessionStartOutput> {
    let db = ctx.require_db()?;

    // Check cancellation before starting
    ctx.check_cancelled()?;

    // 1. Set up project
    let path = Path::new(&input.project_path);
    let canonical_path = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        std::env::current_dir()?.join(path).canonicalize()?
    };

    if !canonical_path.exists() {
        return Err(super::super::CoreError::InvalidArgument(
            format!("Project path does not exist: {}", canonical_path.display())
        ));
    }

    let path_str = canonical_path.to_string_lossy().to_string();
    let name = input.name.unwrap_or_else(|| {
        canonical_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });

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

    let (project_id,): (i64,) = sqlx::query_as("SELECT id FROM projects WHERE path = $1")
        .bind(&path_str)
        .fetch_one(db)
        .await?;

    // Check cancellation after project setup
    ctx.check_cancelled()?;

    // 2. Count mira_usage guidelines
    let (usage_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM coding_guidelines WHERE category = 'mira_usage'"
    )
    .fetch_one(db)
    .await?;

    // 3. Get active corrections
    let corrections = sqlx::query_as::<_, (String, String)>(r#"
        SELECT what_was_wrong, what_is_right
        FROM corrections
        WHERE status = 'active'
          AND (project_id IS NULL OR project_id = $1)
          AND confidence > 0.5
        ORDER BY confidence DESC, times_validated DESC
        LIMIT 5
    "#)
    .bind(project_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    // 4. Get active goals
    let goals = sqlx::query_as::<_, (String, String, i32)>(r#"
        SELECT title, status, progress_percent
        FROM goals
        WHERE status IN ('planning', 'in_progress', 'blocked')
          AND (project_id IS NULL OR project_id = $1)
        ORDER BY
            CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 ELSE 3 END,
            CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END
        LIMIT 5
    "#)
    .bind(project_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    // 5. Get pending tasks
    let tasks = sqlx::query_as::<_, (String, String)>(r#"
        SELECT title, status
        FROM tasks
        WHERE status IN ('pending', 'in_progress', 'blocked')
        ORDER BY
            CASE status WHEN 'in_progress' THEN 1 WHEN 'blocked' THEN 2 ELSE 3 END
        LIMIT 5
    "#)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    // 6. Get recent session topics
    let recent_sessions = sqlx::query_as::<_, (String,)>(r#"
        SELECT content
        FROM memory_entries
        WHERE role = 'session_summary'
          AND (project_id IS NULL OR project_id = $1)
        ORDER BY created_at DESC
        LIMIT 3
    "#)
    .bind(project_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let session_topics: Vec<String> = recent_sessions.into_iter()
        .filter_map(|(content,)| {
            content.lines().next().map(|line| {
                if line.len() > 60 {
                    format!("{}...", &line[..57])
                } else {
                    line.to_string()
                }
            })
        })
        .collect();

    // 7. Get work state
    let active_todos = get_active_todos(ctx, Some(project_id)).await.ok().flatten();
    let active_plan = get_active_plan(ctx, Some(project_id)).await.ok().flatten();
    let working_docs = get_working_docs(ctx, Some(project_id)).await.unwrap_or_default();

    // 8. Check index freshness
    let (index_fresh, stale_count) = check_index_freshness(db, project_id).await.unwrap_or((true, 0));

    Ok(SessionStartOutput {
        project_id,
        project_name: name,
        project_path: path_str,
        project_type,
        usage_guidelines_loaded: usage_count as usize,
        corrections: corrections.into_iter()
            .map(|(wrong, right)| CorrectionSummary { what_was_wrong: wrong, what_is_right: right })
            .collect(),
        goals: goals.into_iter()
            .map(|(title, status, progress)| GoalSummary { title, status, progress_percent: progress })
            .collect(),
        tasks: tasks.into_iter()
            .map(|(title, status)| TaskSummary { title, status })
            .collect(),
        recent_session_topics: session_topics,
        active_todos,
        active_plan,
        working_docs,
        index_fresh,
        stale_file_count: stale_count,
    })
}

/// Check if the code index is fresh
async fn check_index_freshness(db: &sqlx::SqlitePool, project_id: i64) -> anyhow::Result<(bool, usize)> {
    let indexed_files: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT DISTINCT file_path, MAX(analyzed_at) as last_analyzed
        FROM code_symbols
        WHERE project_id = $1
        GROUP BY file_path
        ORDER BY last_analyzed DESC
        LIMIT 50
        "#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    if indexed_files.is_empty() {
        return Ok((false, 0));
    }

    let mut stale_count = 0;
    for (file_path, analyzed_at) in indexed_files {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_ts = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                if mtime_ts > analyzed_at {
                    stale_count += 1;
                }
            }
        }

        if stale_count >= 10 {
            break;
        }
    }

    Ok((stale_count == 0, stale_count))
}

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
