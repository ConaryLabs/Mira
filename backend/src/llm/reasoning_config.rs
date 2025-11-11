// src/llm/reasoning_config.rs
// Dynamic reasoning/verbosity configuration per tool/task type

pub struct ReasoningConfig;

impl ReasoningConfig {
    /// Settings for initial tool selection phase
    pub fn for_tool_selection() -> (&'static str, &'static str) {
        ("low", "low")  // Tool selection is lightweight
    }
    
    /// Settings for synthesizing response after tool(s) executed
    /// CRITICAL: Uses HIGH reasoning to force synthesis instead of more tool calls
    pub fn for_synthesis_after_tools(tools_called: &[&str]) -> (&'static str, &'static str) {
        // FIXED: Always use high reasoning after tools to force synthesis
        // This prevents infinite tool-calling loops where GPT-5 keeps searching
        // instead of answering with what it has.
        
        // Check if any tool requires special handling
        let has_code_gen = tools_called.iter().any(|&t| t == "create_artifact");
        
        if has_code_gen {
            // Code generation needs clean output, but still high reasoning
            ("high", "medium")
        } else {
            // Default: HIGH reasoning to synthesize from tool results
            ("high", "high")
        }
    }
    
    /// Settings for direct response (no tools)
    pub fn for_direct_response() -> (&'static str, &'static str) {
        ("medium", "medium")  // Use defaults
    }
    
    /// Get configuration for a specific tool (legacy, no longer used)
    /// Kept for backwards compatibility but synthesis now always uses high
    #[allow(dead_code)]
    fn get_tool_config(tool: &str) -> Option<(&'static str, &'static str)> {
        match tool {
            // Fast I/O - minimal synthesis needed
            "read_file" => Some(("minimal", "low")),
            "list_files" => Some(("minimal", "low")),
            "read_files" => Some(("minimal", "low")),
            "search_code" => Some(("low", "low")),  // Changed from minimal - needs better synthesis
            
            // Write operations - basic confirmation
            "write_files" => Some(("low", "low")),
            
            // Analysis - comprehensive synthesis
            "get_project_context" => Some(("medium", "high")),
            
            // Code generation - clean output
            "create_artifact" => Some(("low", "low")),
            
            _ => None,
        }
    }
    
    #[allow(dead_code)]
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
