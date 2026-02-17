// crates/mira-server/src/mcp/resources.rs
// MCP Resource handlers — read-only data access via the Resource protocol

use super::MiraServer;
use rmcp::{
    model::{
        AnnotateAble, Annotated, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, RawResource, RawResourceTemplate, ReadResourceRequestParams,
        ReadResourceResult, ResourceContents,
    },
    service::{RequestContext, RoleServer},
};

/// Helper to wrap a raw resource/template without annotations.
fn no_ann<T: AnnotateAble>(raw: T) -> Annotated<T> {
    Annotated::new(raw, None)
}

impl MiraServer {
    /// Build the static list of available resources.
    fn resource_list() -> Vec<Annotated<RawResource>> {
        vec![
            no_ann(RawResource {
                uri: "mira://goals".into(),
                name: "goals".into(),
                title: Some("Active Goals".into()),
                description: Some("List of active goals with progress percentages".into()),
                mime_type: Some("application/json".into()),
                size: None,
                icons: None,
                meta: None,
            }),
            no_ann(RawResource {
                uri: "mira://memories/recent".into(),
                name: "memories-recent".into(),
                title: Some("Recent Memories".into()),
                description: Some("Most recent 20 memories".into()),
                mime_type: Some("application/json".into()),
                size: None,
                icons: None,
                meta: None,
            }),
        ]
    }

    /// Build the list of resource templates (parameterized URIs).
    fn resource_template_list() -> Vec<Annotated<RawResourceTemplate>> {
        vec![no_ann(RawResourceTemplate {
            uri_template: "mira://goals/{id}".into(),
            name: "goal-detail".into(),
            title: Some("Goal Detail".into()),
            description: Some("Individual goal with milestones".into()),
            mime_type: Some("application/json".into()),
            icons: None,
        })]
    }

    /// Handle `resources/list`.
    pub(super) async fn handle_list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        Ok(ListResourcesResult {
            resources: Self::resource_list(),
            next_cursor: None,
            meta: None,
        })
    }

    /// Handle `resources/templates/list`.
    pub(super) async fn handle_list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, rmcp::ErrorData> {
        Ok(ListResourceTemplatesResult {
            resource_templates: Self::resource_template_list(),
            next_cursor: None,
            meta: None,
        })
    }

    /// Handle `resources/read`.
    pub(super) async fn handle_read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let uri = &request.uri;

        match uri.as_str() {
            "mira://goals" => self.read_goals_list().await,
            "mira://memories/recent" => self.read_recent_memories().await,
            _ if uri.starts_with("mira://goals/") => {
                let id_str = &uri["mira://goals/".len()..];
                let id: i64 = id_str.parse().map_err(|_| {
                    rmcp::ErrorData::invalid_params(format!("Invalid goal ID: {id_str}"), None)
                })?;
                self.read_goal_detail(id).await
            }
            _ => Err(rmcp::ErrorData::invalid_params(
                format!("Unknown resource URI: {uri}"),
                None,
            )),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Individual resource readers
    // ─────────────────────────────────────────────────────────────────────────

    /// Read all active goals as JSON.
    async fn read_goals_list(&self) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let project_id = self.project.read().await.as_ref().map(|p| p.id);

        let goals = self
            .pool
            .interact(move |conn| {
                crate::db::get_active_goals_sync(conn, project_id, 100).map(|goals| {
                    goals
                        .into_iter()
                        .map(|g| {
                            serde_json::json!({
                                "id": g.id,
                                "title": g.title,
                                "status": g.status,
                                "priority": g.priority,
                                "progress_percent": g.progress_percent,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .await
            .map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Failed to read goals: {e}"), None)
            })?;

        let json = serde_json::to_string_pretty(&goals).unwrap_or_else(|_| "[]".into());

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: "mira://goals".into(),
                mime_type: Some("application/json".into()),
                text: json,
                meta: None,
            }],
        })
    }

    /// Read a single goal with its milestones as JSON (scoped to active project).
    async fn read_goal_detail(&self, id: i64) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let project_id = self.project.read().await.as_ref().map(|p| p.id);
        let pool = self.pool.clone();

        let result = pool
            .interact(move |conn| {
                let goal = crate::db::get_goal_by_id_sync(conn, id)?;
                let goal = match goal {
                    Some(g) => g,
                    None => return Ok(None),
                };

                // Scope check: goal must belong to the active project or be global
                if goal.project_id.is_some() && goal.project_id != project_id {
                    return Ok(None);
                }

                let milestones = crate::db::get_milestones_for_goal_sync(conn, id)
                    .map_err(|e| anyhow::anyhow!(e))?;

                let milestone_json: Vec<serde_json::Value> = milestones
                    .into_iter()
                    .map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "title": m.title,
                            "completed": m.completed,
                            "weight": m.weight,
                        })
                    })
                    .collect();

                Ok(Some(serde_json::json!({
                    "id": goal.id,
                    "title": goal.title,
                    "description": goal.description,
                    "status": goal.status,
                    "priority": goal.priority,
                    "progress_percent": goal.progress_percent,
                    "created_at": goal.created_at,
                    "milestones": milestone_json,
                })))
            })
            .await
            .map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Failed to read goal: {e}"), None)
            })?;

        let Some(goal_json) = result else {
            return Err(rmcp::ErrorData::invalid_params(
                format!("Goal not found: {id}"),
                None,
            ));
        };

        let uri = format!("mira://goals/{id}");
        let json = serde_json::to_string_pretty(&goal_json).unwrap_or_else(|_| "{}".into());

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri,
                mime_type: Some("application/json".into()),
                text: json,
                meta: None,
            }],
        })
    }

    /// Read recent memories as JSON (scoped to active project, project-scope only).
    async fn read_recent_memories(&self) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let project_id = self.project.read().await.as_ref().map(|p| p.id);

        let Some(pid) = project_id else {
            // No active project — return empty to avoid cross-project data leak
            return Ok(ReadResourceResult {
                contents: vec![ResourceContents::TextResourceContents {
                    uri: "mira://memories/recent".into(),
                    mime_type: Some("application/json".into()),
                    text: "[]".into(),
                    meta: None,
                }],
            });
        };

        let memories = self
            .pool
            .interact(move |conn| {
                let sql = "SELECT id, content, fact_type, category, confidence, \
                           created_at, status, scope
                    FROM memory_facts
                    WHERE project_id = ?1
                      AND scope = 'project'
                      AND status != 'archived'
                      AND COALESCE(suspicious, 0) = 0
                    ORDER BY updated_at DESC, id DESC
                    LIMIT 20";
                let mut stmt = conn.prepare(sql)?;
                let rows = stmt.query_map(rusqlite::params![pid], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, i64>(0)?,
                        "content": row.get::<_, String>(1)?,
                        "fact_type": row.get::<_, String>(2)?,
                        "category": row.get::<_, Option<String>>(3)?,
                        "confidence": row.get::<_, f64>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                        "status": row.get::<_, String>(6)?,
                        "scope": row.get::<_, String>(7)?,
                    }))
                })?;

                let results: rusqlite::Result<Vec<serde_json::Value>> = rows.collect();
                results.map_err(|e| anyhow::anyhow!(e))
            })
            .await
            .map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("Failed to read recent memories: {e}"),
                    None,
                )
            })?;

        let json = serde_json::to_string_pretty(&memories).unwrap_or_else(|_| "[]".into());

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: "mira://memories/recent".into(),
                mime_type: Some("application/json".into()),
                text: json,
                meta: None,
            }],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::DatabasePool;
    use crate::tools::core::ToolContext;
    use mira_types::ProjectContext;
    use std::sync::Arc;

    #[test]
    fn resource_list_has_expected_entries() {
        let resources = MiraServer::resource_list();
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].raw.uri, "mira://goals");
        assert_eq!(resources[0].raw.name, "goals");
        assert!(resources[0].raw.mime_type.as_deref() == Some("application/json"));

        assert_eq!(resources[1].raw.uri, "mira://memories/recent");
        assert_eq!(resources[1].raw.name, "memories-recent");
    }

    #[test]
    fn resource_template_list_has_goal_detail() {
        let templates = MiraServer::resource_template_list();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].raw.uri_template, "mira://goals/{id}");
        assert_eq!(templates[0].raw.name, "goal-detail");
    }

    /// Helper: create a MiraServer with in-memory DBs and an active project.
    async fn server_with_project() -> (MiraServer, i64) {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let code_pool = Arc::new(DatabasePool::open_code_db_in_memory().await.unwrap());
        let server = MiraServer::new(pool.clone(), code_pool, None);

        let project_id = pool
            .interact(|conn| {
                crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                    .map(|(id, _)| id)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        server
            .set_project(ProjectContext {
                id: project_id,
                path: "/test/project".into(),
                name: Some("test".into()),
            })
            .await;

        (server, project_id)
    }

    #[tokio::test]
    async fn goal_detail_allows_project_goal() {
        let (server, project_id) = server_with_project().await;

        let goal_id = server
            .pool
            .interact(move |conn| {
                crate::db::create_goal_sync(
                    conn,
                    Some(project_id),
                    "Project goal",
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        let result = server.read_goal_detail(goal_id).await;
        assert!(result.is_ok(), "Project-scoped goal should be readable");
    }

    #[tokio::test]
    async fn goal_detail_allows_global_goal() {
        let (server, _project_id) = server_with_project().await;

        let goal_id = server
            .pool
            .interact(move |conn| {
                crate::db::create_goal_sync(conn, None, "Global goal", None, None, None, None)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        let result = server.read_goal_detail(goal_id).await;
        assert!(
            result.is_ok(),
            "Global goal (project_id=NULL) should be readable"
        );
    }

    #[tokio::test]
    async fn goal_detail_rejects_other_project_goal() {
        let (server, _project_id) = server_with_project().await;

        // Create a second project and a goal belonging to it
        let other_pid = server
            .pool
            .interact(|conn| {
                crate::db::get_or_create_project_sync(conn, "/other/project", Some("other"))
                    .map(|(id, _)| id)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        let goal_id = server
            .pool
            .interact(move |conn| {
                crate::db::create_goal_sync(
                    conn,
                    Some(other_pid),
                    "Other project goal",
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        let result = server.read_goal_detail(goal_id).await;
        assert!(
            result.is_err(),
            "Goal from another project should be rejected"
        );
    }
}
