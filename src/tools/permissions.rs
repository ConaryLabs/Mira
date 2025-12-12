// src/tools/permissions.rs
// Persistent permission management for Claude Code cross-session auto-approval

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

// === Parameter structs for consolidated permission tool ===

pub struct SavePermissionParams {
    pub tool_name: String,
    pub input_field: Option<String>,
    pub input_pattern: Option<String>,
    pub match_type: Option<String>,
    pub scope: Option<String>,
    pub description: Option<String>,
}

pub struct ListPermissionsParams {
    pub tool_name: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
}

/// Save a new permission rule
pub async fn save_permission(
    db: &SqlitePool,
    req: SavePermissionParams,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let scope = req.scope.as_deref().unwrap_or("project");
    let match_type = req.match_type.as_deref().unwrap_or("prefix");

    let rule_project_id = if scope == "global" { None } else { project_id };

    sqlx::query(
        r#"
        INSERT INTO permission_rules (
            id, scope, project_id, tool_name, input_field, input_pattern,
            match_type, description, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        ON CONFLICT(scope, project_id, tool_name, input_field, input_pattern) DO UPDATE SET
            match_type = excluded.match_type,
            description = excluded.description,
            updated_at = excluded.updated_at
    "#,
    )
    .bind(&id)
    .bind(scope)
    .bind(rule_project_id)
    .bind(&req.tool_name)
    .bind(&req.input_field)
    .bind(&req.input_pattern)
    .bind(match_type)
    .bind(&req.description)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "saved",
        "rule_id": id,
        "tool_name": req.tool_name,
        "input_field": req.input_field,
        "input_pattern": req.input_pattern,
        "match_type": match_type,
        "scope": scope,
        "project_id": rule_project_id,
    }))
}

/// List permission rules
pub async fn list_permissions(
    db: &SqlitePool,
    req: ListPermissionsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(50);
    let scope_filter = req.scope.as_deref().unwrap_or("all");

    let results = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            i64,
            Option<i64>,
        ),
    >(
        r#"
        SELECT id, scope, tool_name, project_id, input_field, input_pattern,
               match_type, description, times_used, last_used_at
        FROM permission_rules
        WHERE ($1 IS NULL OR tool_name = $1)
          AND (
            $2 = 'all' OR
            ($2 = 'global' AND scope = 'global') OR
            ($2 = 'project' AND scope = 'project' AND project_id = $3)
          )
        ORDER BY tool_name, scope, created_at DESC
        LIMIT $4
    "#,
    )
    .bind(&req.tool_name)
    .bind(scope_filter)
    .bind(project_id)
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(results
        .into_iter()
        .map(
            |(id, scope, tool, proj_id, field, pattern, match_type, desc, used, last_used)| {
                serde_json::json!({
                    "id": id,
                    "scope": scope,
                    "tool_name": tool,
                    "project_id": proj_id,
                    "input_field": field,
                    "input_pattern": pattern,
                    "match_type": match_type,
                    "description": desc,
                    "times_used": used,
                    "last_used_at": last_used,
                })
            },
        )
        .collect())
}

/// Delete a permission rule
pub async fn delete_permission(
    db: &SqlitePool,
    rule_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let result = sqlx::query("DELETE FROM permission_rules WHERE id = $1")
        .bind(rule_id)
        .execute(db)
        .await?;

    if result.rows_affected() > 0 {
        Ok(serde_json::json!({
            "status": "deleted",
            "rule_id": rule_id,
        }))
    } else {
        Ok(serde_json::json!({
            "status": "not_found",
            "rule_id": rule_id,
        }))
    }
}
