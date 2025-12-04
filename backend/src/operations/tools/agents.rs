// backend/src/operations/tools/agents.rs
// Agent spawning tool schemas
//
// Note: These are static schemas with the built-in agents.
// For dynamic schemas including custom agents, use AgentManager::get_agent_tools()

use serde_json::{json, Value};

/// Get agent tool schemas with built-in agents only
/// For dynamic schemas including custom agents, use AgentManager::get_agent_tools()
pub fn get_tools() -> Vec<Value> {
    vec![spawn_agent_tool(), spawn_agents_parallel_tool()]
}

/// Tool: spawn_agent - spawn a specialized agent for a subtask
fn spawn_agent_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "spawn_agent",
            "description": "Spawn a specialized agent to handle a subtask. Use this to delegate work to agents \
                           with specific capabilities. The agent will work autonomously and return a summary.\n\n\
                           Available agents:\n\
                           - `explore`: Read-only codebase exploration. Use for understanding code, finding patterns, \
                             tracing execution flow, analyzing architecture.\n\
                           - `plan`: Research and planning for complex tasks. Use for designing implementations, \
                             creating technical plans, exploring options.\n\
                           - `general`: General-purpose coding assistant with full capabilities. Use for implementing \
                             features, fixing bugs, refactoring code, running commands.",
            "parameters": {
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "enum": ["explore", "plan", "general"],
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

/// Tool: spawn_agents_parallel - spawn multiple agents concurrently
fn spawn_agents_parallel_tool() -> Value {
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
                                    "enum": ["explore", "plan", "general"],
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
    fn test_get_tools() {
        let tools = get_tools();
        assert_eq!(tools.len(), 2);

        let spawn_agent = &tools[0];
        assert_eq!(spawn_agent["function"]["name"], "spawn_agent");

        let spawn_parallel = &tools[1];
        assert_eq!(spawn_parallel["function"]["name"], "spawn_agents_parallel");
    }

    #[test]
    fn test_spawn_agent_schema() {
        let tool = spawn_agent_tool();
        let params = &tool["function"]["parameters"];

        // Check required fields
        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&json!("agent_id")));
        assert!(required.contains(&json!("task")));

        // Check agent enum
        let agent_enum = &params["properties"]["agent_id"]["enum"];
        assert!(agent_enum.as_array().unwrap().contains(&json!("explore")));
        assert!(agent_enum.as_array().unwrap().contains(&json!("plan")));
        assert!(agent_enum.as_array().unwrap().contains(&json!("general")));
    }
}
