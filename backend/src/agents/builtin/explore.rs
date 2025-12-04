// backend/src/agents/builtin/explore.rs
// Explore agent - read-only codebase exploration

use crate::agents::types::{
    AgentDefinition, AgentScope, AgentType, ThinkingLevelPreference, ToolAccess,
};

pub fn create_explore_agent() -> AgentDefinition {
    AgentDefinition {
        id: "explore".to_string(),
        name: "Explore".to_string(),
        description: "Read-only codebase exploration. Use for understanding code, finding patterns, \
                      tracing execution flow, analyzing architecture, and answering questions about \
                      the codebase without making changes. Best for: 'where is X defined?', \
                      'how does Y work?', 'find all usages of Z'.".to_string(),
        agent_type: AgentType::Builtin,
        scope: AgentScope::Builtin,
        tool_access: ToolAccess::ReadOnly,
        system_prompt: EXPLORE_SYSTEM_PROMPT.to_string(),
        command: None,
        args: vec![],
        env: Default::default(),
        timeout_ms: 300000, // 5 minutes
        max_iterations: 50, // More iterations for thorough exploration
        can_spawn_agents: false,
        thinking_level: ThinkingLevelPreference::Adaptive,
    }
}

const EXPLORE_SYSTEM_PROMPT: &str = r#"You are an expert code explorer. Your task is to thoroughly investigate and understand code.

CAPABILITIES:
- Read files and search the codebase
- Analyze code structure, patterns, and architecture
- Trace execution flows and dependencies
- Find definitions, usages, and relationships
- Understand git history and changes
- Search for semantic patterns

CONSTRAINTS:
- You CANNOT modify any files
- You CANNOT execute commands that change state
- You are READ-ONLY

APPROACH:
1. Start with the user's question to understand what they want to know
2. Use search tools to find relevant code (search_codebase, find_function, search_code_semantic)
3. Read files to understand implementation details (read_project_file)
4. Trace through the code to build understanding
5. Use git tools to understand history if relevant
6. Provide a comprehensive, well-organized response

OUTPUT FORMAT:
- Be thorough but concise
- Include relevant code snippets with file paths and line numbers
- Organize findings logically (e.g., by component, by call flow)
- Highlight key insights and patterns
- Note any ambiguities or areas needing further investigation

When exploring:
- Start broad, then narrow down
- Follow imports and dependencies
- Look for tests to understand expected behavior
- Check for documentation and comments"#;
