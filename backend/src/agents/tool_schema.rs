// backend/src/agents/tool_schema.rs
// Tool schema for spawning agents (used by LLM for agent invocation)

use serde_json::{json, Value};

/// Build the spawn_agent tool schema with dynamic agent list
pub fn build_spawn_agent_tool(agents: &[(String, String)]) -> Value {
    let agent_ids: Vec<&str> = agents.iter().map(|(id, _)| id.as_str()).collect();
    let descriptions: String = agents
        .iter()
        .map(|(id, desc)| format!("- `{}`: {}", id, desc))
        .collect::<Vec<_>>()
        .join("\n");

    json!({
        "type": "function",
        "function": {
            "name": "spawn_agent",
            "description": format!(
                "Spawn a specialized agent to handle a subtask. Use this to delegate work to agents \
                 with specific capabilities. The agent will work autonomously and return a summary.\n\n\
                 Available agents:\n{}",
                descriptions
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "enum": agent_ids,
                        "description": "The agent to spawn"
                    },
                    "task": {
                        "type": "string",
                        "description": "Clear description of what the agent should accomplish"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context, constraints, or background information for the agent"
                    },
                    "context_files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Specific file paths the agent should focus on"
                    }
                },
                "required": ["agent_id", "task"]
            }
        }
    })
}

/// Build tool schema for spawning multiple agents in parallel
pub fn build_spawn_agents_parallel_tool(agents: &[(String, String)]) -> Value {
    let agent_ids: Vec<&str> = agents.iter().map(|(id, _)| id.as_str()).collect();

    json!({
        "type": "function",
        "function": {
            "name": "spawn_agents_parallel",
            "description": "Spawn multiple specialized agents to work in parallel on independent tasks. \
                           Returns when all agents complete. Use this when you have multiple independent \
                           subtasks that can be executed concurrently.",
            "parameters": {
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "description": "Array of agent configurations to spawn in parallel",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent_id": {
                                    "type": "string",
                                    "enum": agent_ids,
                                    "description": "The agent to spawn"
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Task description for this agent"
                                },
                                "context": {
                                    "type": "string",
                                    "description": "Additional context for this agent"
                                },
                                "context_files": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Files for this agent to focus on"
                                }
                            },
                            "required": ["agent_id", "task"]
                        }
                    }
                },
                "required": ["agents"]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_agent_tool_schema() {
        let agents = vec![
            ("explore".to_string(), "Read-only codebase exploration".to_string()),
            ("plan".to_string(), "Research and planning".to_string()),
            ("general".to_string(), "Full capabilities".to_string()),
        ];

        let schema = build_spawn_agent_tool(&agents);

        assert_eq!(schema["type"], "function");
        assert_eq!(schema["function"]["name"], "spawn_agent");

        let params = &schema["function"]["parameters"];
        assert_eq!(params["type"], "object");

        // Check agent_id enum
        let agent_id_enum = &params["properties"]["agent_id"]["enum"];
        assert!(agent_id_enum.as_array().unwrap().contains(&json!("explore")));
        assert!(agent_id_enum.as_array().unwrap().contains(&json!("plan")));
        assert!(agent_id_enum.as_array().unwrap().contains(&json!("general")));

        // Check required fields
        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&json!("agent_id")));
        assert!(required.contains(&json!("task")));
    }

    #[test]
    fn test_spawn_agents_parallel_tool_schema() {
        let agents = vec![
            ("explore".to_string(), "Exploration".to_string()),
            ("plan".to_string(), "Planning".to_string()),
        ];

        let schema = build_spawn_agents_parallel_tool(&agents);

        assert_eq!(schema["function"]["name"], "spawn_agents_parallel");

        let items = &schema["function"]["parameters"]["properties"]["agents"]["items"];
        let item_agent_id = &items["properties"]["agent_id"]["enum"];
        assert!(item_agent_id.as_array().unwrap().contains(&json!("explore")));
    }
}
