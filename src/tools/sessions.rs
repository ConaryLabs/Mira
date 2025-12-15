// src/tools/sessions.rs
// Cross-session memory tools - remember and search across Claude Code sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;
use std::path::Path;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Get session context - combines recent sessions, memories, pending tasks, goals, and corrections
/// This is the "where did we leave off?" tool for session startup
pub async fn get_session_context(
    db: &SqlitePool,
    req: GetSessionContextRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let limit = req.limit.unwrap_or(5);
    let include_memories = req.include_memories.unwrap_or(true);
    let include_tasks = req.include_tasks.unwrap_or(true);
    let include_sessions = req.include_sessions.unwrap_or(true);
    let include_goals = req.include_goals.unwrap_or(true);
    let include_corrections = req.include_corrections.unwrap_or(true);

    let mut context = serde_json::json!({
        "project_id": project_id,
    });

    // Get recent memories (decisions, context, preferences)
    if include_memories {
        let memories = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, String)>(r#"
            SELECT id, key, value, fact_type, category,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM memory_facts
            WHERE (project_id IS NULL OR project_id = $1)
            ORDER BY updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        let memories_json: Vec<serde_json::Value> = memories.into_iter().map(|(id, key, value, fact_type, category, updated)| {
            serde_json::json!({
                "id": id,
                "key": key,
                "value": value,
                "fact_type": fact_type,
                "category": category,
                "updated": updated,
            })
        }).collect();

        context["recent_memories"] = serde_json::json!(memories_json);
    }

    // Get pending/in-progress tasks
    if include_tasks {
        let tasks = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, String)>(r#"
            SELECT id, title, description, status, priority,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM tasks
            WHERE status IN ('pending', 'in_progress', 'blocked')
              AND (project_path IS NULL OR $1 IS NULL OR project_path LIKE '%' || $1 || '%')
            ORDER BY
                CASE status
                    WHEN 'in_progress' THEN 1
                    WHEN 'blocked' THEN 2
                    WHEN 'pending' THEN 3
                END,
                CASE priority
                    WHEN 'urgent' THEN 1
                    WHEN 'high' THEN 2
                    WHEN 'medium' THEN 3
                    WHEN 'low' THEN 4
                END,
                updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id.map(|_| "")) // Just checking if project is set
        .bind(limit)
        .fetch_all(db)
        .await?;

        let tasks_json: Vec<serde_json::Value> = tasks.into_iter().map(|(id, title, description, status, priority, updated)| {
            serde_json::json!({
                "id": id,
                "title": title,
                "description": description,
                "status": status,
                "priority": priority,
                "updated": updated,
            })
        }).collect();

        context["pending_tasks"] = serde_json::json!(tasks_json);
    }

    // Get recent session summaries
    if include_sessions {
        let sessions = sqlx::query_as::<_, (String, String, String)>(r#"
            SELECT session_id, content,
                   datetime(created_at, 'unixepoch', 'localtime') as created
            FROM memory_entries
            WHERE role = 'session_summary'
              AND (project_id IS NULL OR project_id = $1)
            ORDER BY created_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        let sessions_json: Vec<serde_json::Value> = sessions.into_iter().map(|(session_id, content, created)| {
            serde_json::json!({
                "session_id": session_id,
                "summary": content,
                "created": created,
            })
        }).collect();

        context["recent_sessions"] = serde_json::json!(sessions_json);
    }

    // Get active goals
    if include_goals {
        let goals = sqlx::query_as::<_, (String, String, String, String, i32, Option<String>)>(r#"
            SELECT id, title, status, priority, progress_percent, blockers
            FROM goals
            WHERE status IN ('planning', 'in_progress', 'blocked')
              AND (project_id IS NULL OR project_id = $1)
            ORDER BY
                CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 ELSE 3 END,
                CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        let goals_json: Vec<serde_json::Value> = goals.into_iter().map(|(id, title, status, priority, progress, blockers)| {
            serde_json::json!({
                "id": id,
                "title": title,
                "status": status,
                "priority": priority,
                "progress_percent": progress,
                "has_blockers": blockers.map(|b| !b.is_empty()).unwrap_or(false),
            })
        }).collect();

        if !goals_json.is_empty() {
            context["active_goals"] = serde_json::json!(goals_json);
        }
    }

    // Get recent/important corrections
    if include_corrections {
        let corrections = sqlx::query_as::<_, (String, String, String, String, f64)>(r#"
            SELECT id, correction_type, what_was_wrong, what_is_right, confidence
            FROM corrections
            WHERE status = 'active'
              AND (project_id IS NULL OR project_id = $1)
              AND confidence > 0.5
            ORDER BY confidence DESC, times_validated DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        let corrections_json: Vec<serde_json::Value> = corrections.into_iter().map(|(id, ctype, wrong, right, confidence)| {
            serde_json::json!({
                "id": id,
                "correction_type": ctype,
                "what_was_wrong": wrong,
                "what_is_right": right,
                "confidence": confidence,
            })
        }).collect();

        if !corrections_json.is_empty() {
            context["active_corrections"] = serde_json::json!(corrections_json);
        }
    }

    Ok(context)
}

/// Store a session summary for cross-session recall
/// Session is scoped to the active project if project_id is provided
pub async fn store_session(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreSessionRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in SQLite for persistence (upsert - update if session exists)
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
    .bind(&req.summary)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search (if available)
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("session_id".to_string(), serde_json::Value::String(session_id.clone()));
        metadata.insert("type".to_string(), serde_json::Value::String("session_summary".to_string()));
        metadata.insert("timestamp".to_string(), serde_json::Value::Number(now.into()));

        if let Some(pid) = project_id {
            metadata.insert("project_id".to_string(), serde_json::Value::Number(pid.into()));
        }

        if let Some(ref project) = req.project_path {
            metadata.insert("project_path".to_string(), serde_json::Value::String(project.clone()));
        }

        if let Some(ref topics) = req.topics {
            metadata.insert("topics".to_string(), serde_json::Value::String(topics.join(",")));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(
            COLLECTION_CONVERSATION,
            &session_id,
            &req.summary,
            metadata,
        ).await {
            tracing::warn!("Failed to store session in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": "stored",
        "session_id": session_id,
        "project_id": project_id,
        "semantic_search": semantic.is_available(),
    }))
}

/// Search across past sessions using semantic similarity
/// Returns sessions from the active project AND global sessions
pub async fn search_sessions(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: SearchSessionsRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10) as usize;

    // If semantic search is available, use it
    if semantic.is_available() {
        // Filter for session summaries
        // Note: Qdrant doesn't support OR conditions with is_null easily,
        // so we filter by type only and rely on SQLite for strict project filtering.
        let filter = Some(qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
        ]));
        // If we have a specific project, also add that filter
        let filter = if let Some(pid) = project_id {
            Some(qdrant_client::qdrant::Filter::must([
                qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
                qdrant_client::qdrant::Condition::matches("project_id", pid),
            ]))
        } else {
            filter
        };

        let results = semantic.search(
            COLLECTION_CONVERSATION,
            &req.query,
            limit,
            filter,
        ).await?;

        return Ok(results.into_iter().map(|r| {
            let mut result = serde_json::json!({
                "content": r.content,
                "score": r.score,
            });

            // Add metadata fields
            if let Some(obj) = result.as_object_mut() {
                for (key, value) in r.metadata {
                    obj.insert(key, value);
                }
            }

            result
        }).collect());
    }

    // Fallback to SQLite text search
    // Include sessions from this project AND global sessions
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
        .bind(&req.query)
        .bind(project_id)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, session_id, content, created_at, proj_id)| {
        serde_json::json!({
            "id": id,
            "session_id": session_id,
            "content": content,
            "created_at": created_at,
            "project_id": proj_id,
            "search_type": "text",
        })
    }).collect())
}

/// Store a key decision or important context from a session
/// Decisions are project-scoped by default (unlike preferences)
pub async fn store_decision(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreDecisionRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = uuid::Uuid::new_v4().to_string();

    // Store in memory_facts for structured recall
    sqlx::query(r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
        VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            updated_at = excluded.updated_at
    "#)
    .bind(&id)
    .bind(&req.key)
    .bind(&req.decision)
    .bind(&req.category)
    .bind(&req.context)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), serde_json::Value::String("decision".to_string()));
        metadata.insert("key".to_string(), serde_json::Value::String(req.key.clone()));
        metadata.insert("fact_type".to_string(), serde_json::Value::String("decision".to_string()));

        if let Some(pid) = project_id {
            metadata.insert("project_id".to_string(), serde_json::Value::Number(pid.into()));
        }

        if let Some(ref category) = req.category {
            metadata.insert("category".to_string(), serde_json::Value::String(category.clone()));
        }

        if let Some(ref context) = req.context {
            metadata.insert("context".to_string(), serde_json::Value::String(context.clone()));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(
            COLLECTION_CONVERSATION,
            &id,
            &req.decision,
            metadata,
        ).await {
            tracing::warn!("Failed to store decision in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": "stored",
        "id": id,
        "key": req.key,
        "project_id": project_id,
    }))
}

/// Combined session startup - sets project and loads all context in one call
/// Returns a concise summary instead of raw JSON
pub async fn session_start(
    db: &SqlitePool,
    req: SessionStartRequest,
) -> anyhow::Result<SessionStartResult> {
    // 1. Set up project (like set_project)
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

    let (project_id,): (i64,) = sqlx::query_as("SELECT id FROM projects WHERE path = $1")
        .bind(&path_str)
        .fetch_one(db)
        .await?;

    // 2. Count mira_usage guidelines (don't return content - just note they're loaded)
    let (usage_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM coding_guidelines WHERE category = 'mira_usage'"
    )
    .fetch_one(db)
    .await?;

    // 4. Get active corrections
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

    // 5. Get active goals
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

    // 6. Get pending tasks
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

    // 7. Get recent session topics (just first lines)
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

    // 8. Get active todos from work state (for seamless resume)
    let active_todos = get_active_todos(db, Some(project_id)).await.unwrap_or(None);

    // 9. Get active plan from work state (for seamless resume)
    let active_plan = get_active_plan(db, Some(project_id)).await.unwrap_or(None);

    // 10. Get working documents from work state (for seamless resume)
    let working_docs = get_working_docs(db, Some(project_id)).await.unwrap_or_default();

    Ok(SessionStartResult {
        project_id,
        project_name: name,
        project_path: path_str,
        project_type,
        persona_summary: None, // Persona only used in Studio chat, not Claude Code
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
    })
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

// Re-export work state types and functions from dedicated module
pub use super::work_state::{
    ActivePlan, WorkingDoc, WorkStateTodo,
    sync_work_state, get_work_state, get_active_todos, get_active_plan, get_working_docs,
};

// Result types for session_start
#[derive(Debug)]
pub struct SessionStartResult {
    pub project_id: i64,
    pub project_name: String,
    pub project_path: String,
    pub project_type: Option<String>,
    pub persona_summary: Option<String>,
    pub usage_guidelines_loaded: usize,
    pub corrections: Vec<CorrectionSummary>,
    pub goals: Vec<GoalSummary>,
    pub tasks: Vec<TaskSummary>,
    pub recent_session_topics: Vec<String>,
    pub active_todos: Option<Vec<WorkStateTodo>>,
    pub active_plan: Option<ActivePlan>,
    pub working_docs: Vec<WorkingDoc>,
}

#[derive(Debug)]
pub struct CorrectionSummary {
    pub what_was_wrong: String,
    pub what_is_right: String,
}

#[derive(Debug)]
pub struct GoalSummary {
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

#[derive(Debug)]
pub struct TaskSummary {
    pub title: String,
    pub status: String,
}
