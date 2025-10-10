// src/llm/reasoning_config.rs
// Dynamic reasoning/verbosity configuration per tool/task type

pub struct ReasoningConfig;

impl ReasoningConfig {
    /// Settings for initial tool selection phase
    pub fn for_tool_selection() -> (&'static str, &'static str) {
        ("low", "low")  // Tool selection is lightweight
    }
    
    /// Settings for synthesizing response after tool(s) executed
    pub fn for_synthesis_after_tools(tools_called: &[&str]) -> (&'static str, &'static str) {
        // Use highest reasoning requirement from all tools called
        let mut max_reasoning = ("medium", "medium");
        let mut max_rank = Self::reasoning_rank(max_reasoning.0);
        
        for tool in tools_called {
            if let Some(config) = Self::get_tool_config(tool) {
                let rank = Self::reasoning_rank(config.0);
                if rank > max_rank {
                    max_rank = rank;
                    max_reasoning = config;
                }
            }
        }
        
        max_reasoning
    }
    
    /// Settings for direct response (no tools)
    pub fn for_direct_response() -> (&'static str, &'static str) {
        ("medium", "medium")  // Use defaults
    }
    
    /// Get configuration for a specific tool
    fn get_tool_config(tool: &str) -> Option<(&'static str, &'static str)> {
        match tool {
            // Fast I/O - minimal synthesis needed
            "read_file" => Some(("minimal", "low")),
            "list_files" => Some(("minimal", "low")),
            "read_files" => Some(("minimal", "low")),
            "search_code" => Some(("minimal", "low")),
            
            // Write operations - basic confirmation
            "write_files" => Some(("low", "low")),
            
            // Analysis - comprehensive synthesis
            "get_project_context" => Some(("medium", "high")),
            
            // Code generation - clean output
            "create_artifact" => Some(("low", "low")),
            
            _ => None,
        }
    }
    
    fn reasoning_rank(reasoning: &str) -> u8 {
        match reasoning {
            "minimal" => 1,
            "low" => 2,
            "medium" => 3,
            "high" => 4,
            _ => 2,
        }
    }
}
