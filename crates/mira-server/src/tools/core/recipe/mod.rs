// crates/mira-server/src/tools/core/recipe/mod.rs
// Reusable team recipes — static data defining team blueprints for Agent Teams.

mod expert_review;
mod full_cycle;
mod prompts;
mod qa_hardening;
mod refactor;

use crate::error::MiraError;
use crate::mcp::requests::{RecipeAction, RecipeRequest};
use crate::mcp::responses::{
    Json, RecipeData, RecipeGetData, RecipeListData, RecipeListItem, RecipeMemberData,
    RecipeOutput, RecipeTaskData, ToolOutput,
};

/// Static recipe data model (not stored in DB).
struct Recipe {
    name: &'static str,
    description: &'static str,
    use_when: &'static str,
    members: &'static [RecipeMember],
    tasks: &'static [RecipeTask],
    coordination: &'static str,
}

struct RecipeMember {
    name: &'static str,
    agent_type: &'static str,
    prompt: &'static str,
}

struct RecipeTask {
    subject: &'static str,
    description: &'static str,
    assignee: &'static str,
}

/// All built-in recipes.
const ALL_RECIPES: &[&Recipe] = &[
    &expert_review::RECIPE,
    &full_cycle::RECIPE,
    &qa_hardening::RECIPE,
    &refactor::RECIPE,
];

// ============================================================================
// Handler
// ============================================================================

/// Handle recipe tool actions.
pub async fn handle_recipe(req: RecipeRequest) -> Result<Json<RecipeOutput>, MiraError> {
    match req.action {
        RecipeAction::List => action_list(),
        RecipeAction::Get => action_get(req.name),
    }
}

fn action_list() -> Result<Json<RecipeOutput>, MiraError> {
    let recipes: Vec<RecipeListItem> = ALL_RECIPES
        .iter()
        .map(|r| RecipeListItem {
            name: r.name.to_string(),
            description: r.description.to_string(),
            member_count: r.members.len(),
            use_when: r.use_when.to_string(),
        })
        .collect();
    let count = recipes.len();

    Ok(Json(ToolOutput {
        action: "list".to_string(),
        message: format!("{} recipe(s) available.", count),
        data: Some(RecipeData::List(RecipeListData { recipes })),
    }))
}

fn action_get(name: Option<String>) -> Result<Json<RecipeOutput>, MiraError> {
    let name = name.ok_or_else(|| {
        let available: Vec<&str> = ALL_RECIPES.iter().map(|r| r.name).collect();
        MiraError::InvalidInput(format!(
            "name is required for recipe(action=get). Available: {}",
            available.join(", ")
        ))
    })?;

    let recipe = ALL_RECIPES
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&name))
        .ok_or_else(|| {
            let available: Vec<&str> = ALL_RECIPES.iter().map(|r| r.name).collect();
            MiraError::InvalidInput(format!(
                "Recipe '{}' not found. Available: {}",
                name,
                available.join(", ")
            ))
        })?;

    let members: Vec<RecipeMemberData> = recipe
        .members
        .iter()
        .map(|m| RecipeMemberData {
            name: m.name.to_string(),
            agent_type: m.agent_type.to_string(),
            prompt: m.prompt.to_string(),
        })
        .collect();

    let tasks: Vec<RecipeTaskData> = recipe
        .tasks
        .iter()
        .map(|t| RecipeTaskData {
            subject: t.subject.to_string(),
            description: t.description.to_string(),
            assignee: t.assignee.to_string(),
        })
        .collect();

    Ok(Json(ToolOutput {
        action: "get".to_string(),
        message: format!(
            "Recipe '{}': {} members, {} tasks.",
            recipe.name,
            members.len(),
            tasks.len()
        ),
        data: Some(RecipeData::Get(RecipeGetData {
            name: recipe.name.to_string(),
            description: recipe.description.to_string(),
            members,
            tasks,
            coordination: recipe.coordination.to_string(),
        })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recipe_action_variants() {
        let list: RecipeAction = serde_json::from_str(r#""list""#).unwrap();
        assert!(matches!(list, RecipeAction::List));

        let get: RecipeAction = serde_json::from_str(r#""get""#).unwrap();
        assert!(matches!(get, RecipeAction::Get));
    }

    #[tokio::test]
    async fn test_list_recipes() {
        let req = RecipeRequest {
            action: RecipeAction::List,
            name: None,
        };
        let Json(output) = handle_recipe(req).await.expect("list should succeed");
        assert_eq!(output.action, "list");
        assert!(output.message.contains("4 recipe(s)"));
        match output.data {
            Some(RecipeData::List(data)) => {
                assert_eq!(data.recipes.len(), 4);
                assert_eq!(data.recipes[0].name, "expert-review");
                assert_eq!(data.recipes[0].member_count, 7);
                assert!(!data.recipes[0].use_when.is_empty());
                assert_eq!(data.recipes[2].name, "qa-hardening");
                assert_eq!(data.recipes[2].member_count, 5);
                assert_eq!(data.recipes[3].name, "refactor");
                assert_eq!(data.recipes[3].member_count, 3);
            }
            _ => panic!("Expected RecipeData::List"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("expert-review".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "expert-review");
                assert_eq!(data.members.len(), 7);
                assert_eq!(data.tasks.len(), 7);
                assert_eq!(data.members[0].name, "architect");
                assert_eq!(data.tasks[0].assignee, "architect");
                assert!(!data.coordination.is_empty());
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_full_cycle_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("full-cycle".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "full-cycle");
                assert_eq!(data.members.len(), 9); // 7 discovery + 2 QA
                assert_eq!(data.tasks.len(), 9);
                // Verify discovery experts
                assert_eq!(data.members[0].name, "architect");
                assert_eq!(data.members[4].name, "ux-strategist");
                assert_eq!(data.members[5].name, "growth-strategist");
                assert_eq!(data.members[6].name, "plan-reviewer");
                // Verify QA agents
                assert_eq!(data.members[7].name, "test-runner");
                assert_eq!(data.members[8].name, "ux-reviewer");
                assert!(data.coordination.contains("Phase 1"));
                assert!(data.coordination.contains("Phase 2"));
                assert!(data.coordination.contains("Phase 3"));
                assert!(data.coordination.contains("Phase 4"));
                assert!(data.coordination.contains("Phase 5"));
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_qa_hardening_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("qa-hardening".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "qa-hardening");
                assert_eq!(data.members.len(), 5);
                assert_eq!(data.tasks.len(), 5);
                assert_eq!(data.members[0].name, "test-runner");
                assert_eq!(data.members[1].name, "error-auditor");
                assert_eq!(data.members[2].name, "security");
                assert_eq!(data.members[3].name, "edge-case-hunter");
                assert_eq!(data.members[4].name, "ux-reviewer");
                assert!(data.coordination.contains("Production Readiness"));
                assert!(data.coordination.contains("hardening backlog"));
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_refactor_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("refactor".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "refactor");
                assert_eq!(data.members.len(), 3);
                assert_eq!(data.tasks.len(), 3);
                assert_eq!(data.members[0].name, "architect");
                assert_eq!(data.members[1].name, "code-reviewer");
                assert_eq!(data.members[2].name, "test-runner");
                assert!(data.coordination.contains("Safe Restructuring"));
                assert!(data.coordination.contains("Phase 4: Implementation"));
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe_not_found() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("nonexistent".to_string()),
        };
        match handle_recipe(req).await {
            Err(e) => assert!(e.to_string().contains("not found"), "unexpected error: {e}"),
            Ok(_) => panic!("Expected error for nonexistent recipe"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe_missing_name() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: None,
        };
        match handle_recipe(req).await {
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("required"), "unexpected error: {e}");
                assert!(
                    msg.contains("Available:"),
                    "should hint available recipes: {e}"
                );
            }
            Ok(_) => panic!("Expected error for missing name"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe_case_insensitive() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("Expert-Review".to_string()),
        };
        let Json(output) = handle_recipe(req)
            .await
            .expect("case-insensitive get should succeed");
        match output.data {
            Some(RecipeData::Get(data)) => assert_eq!(data.name, "expert-review"),
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[test]
    fn test_all_task_assignees_match_members() {
        for recipe in ALL_RECIPES {
            let member_names: Vec<&str> = recipe.members.iter().map(|m| m.name).collect();
            for task in recipe.tasks {
                assert!(
                    member_names.contains(&task.assignee),
                    "Recipe '{}': task '{}' has assignee '{}' but no member with that name. Members: {:?}",
                    recipe.name,
                    task.subject,
                    task.assignee,
                    member_names
                );
            }
        }
    }

    #[test]
    fn test_all_member_names_unique() {
        for recipe in ALL_RECIPES {
            let mut seen = std::collections::HashSet::new();
            for member in recipe.members {
                assert!(
                    seen.insert(member.name),
                    "Recipe '{}': duplicate member name '{}'",
                    recipe.name,
                    member.name
                );
            }
        }
    }

    #[test]
    fn test_no_empty_fields() {
        for recipe in ALL_RECIPES {
            assert!(!recipe.name.is_empty(), "Recipe has empty name");
            assert!(
                !recipe.description.is_empty(),
                "Recipe '{}' has empty description",
                recipe.name
            );
            assert!(
                !recipe.use_when.is_empty(),
                "Recipe '{}' has empty use_when",
                recipe.name
            );
            assert!(
                !recipe.coordination.is_empty(),
                "Recipe '{}' has empty coordination",
                recipe.name
            );
            assert!(
                !recipe.members.is_empty(),
                "Recipe '{}' has no members",
                recipe.name
            );
            assert!(
                !recipe.tasks.is_empty(),
                "Recipe '{}' has no tasks",
                recipe.name
            );

            for member in recipe.members {
                assert!(
                    !member.name.is_empty(),
                    "Recipe '{}': member has empty name",
                    recipe.name
                );
                assert!(
                    !member.agent_type.is_empty(),
                    "Recipe '{}': member '{}' has empty agent_type",
                    recipe.name,
                    member.name
                );
                assert!(
                    !member.prompt.is_empty(),
                    "Recipe '{}': member '{}' has empty prompt",
                    recipe.name,
                    member.name
                );
            }

            for task in recipe.tasks {
                assert!(
                    !task.subject.is_empty(),
                    "Recipe '{}': task has empty subject",
                    recipe.name
                );
                assert!(
                    !task.description.is_empty(),
                    "Recipe '{}': task '{}' has empty description",
                    recipe.name,
                    task.subject
                );
                assert!(
                    !task.assignee.is_empty(),
                    "Recipe '{}': task '{}' has empty assignee",
                    recipe.name,
                    task.subject
                );
            }
        }
    }

    #[test]
    fn test_all_recipes_have_when_to_use() {
        for recipe in ALL_RECIPES {
            assert!(
                recipe.coordination.contains("When to Use"),
                "Recipe '{}' is missing a 'When to Use' section in coordination",
                recipe.name
            );
        }
    }
}
