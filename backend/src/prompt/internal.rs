// backend/src/prompt/internal.rs
// Centralized internal prompts for technical operations
//
// DESIGN NOTE: These prompts do NOT use the persona from src/persona/default.rs because
// they have specific technical requirements that would be compromised by adding personality:
//
// 1. JSON OUTPUT REQUIRED - These prompts must return structured JSON that gets parsed.
//    Adding personality could introduce conversational text that breaks parsing.
//
// 2. CODE GENERATION REQUIRED - These prompts generate compilable Rust code.
//    Personality text would corrupt the code output.
//
// 3. INNER LOOP OPERATIONS - These feed results back to the LLM for further processing.
//    Extra text reduces context efficiency and can confuse the model.
//
// For user-facing prompts that SHOULD use personality, see:
// - src/persona/default.rs (personality definition)
// - src/prompt/builders.rs (UnifiedPromptBuilder)

/// Tool router prompts - inner loop operations that feed results back to GPT
/// These need to be minimal and focused to avoid wasting context tokens
pub mod tool_router {
    /// File reading assistant prompt - used in inner loop file operations
    pub const FILE_READER: &str = "You are a file reading assistant. Use the read_file tool to read the requested files. \
Read all requested files and return a summary with the file contents.";

    /// Code search assistant prompt - used for grep operations
    pub const CODE_SEARCHER: &str =
        "You are a code search assistant. Use the grep_files tool to search for the requested pattern.";

    /// File listing assistant prompt - used for directory listings
    pub const FILE_LISTER: &str =
        "You are a file listing assistant. Use the list_files tool to list the requested directory.";
}

/// Pattern system prompts - JSON output required for pattern matching
pub mod patterns {
    /// Pattern matcher prompt - returns JSON {score, reason}
    pub const PATTERN_MATCHER: &str = r#"You are a pattern matching assistant. Given a user's request and a coding pattern, determine how well the pattern applies.

Return a JSON object with:
- score: A number from 0.0 to 1.0 indicating match quality
- reason: Brief explanation of why (one sentence)

Example response:
{"score": 0.85, "reason": "User is adding a database migration which matches this pattern"}"#;

    /// Step executor prompt - generates code for pattern steps
    /// Returns a formatted prompt with the given step details
    pub fn step_executor(
        step_number: i32,
        step_type: &str,
        description: &str,
        rationale: &str,
    ) -> String {
        format!(
            r#"You are executing step {} of a coding pattern.

Step type: {}
Step description: {}
{}

Provide a concise response that completes this step. Be specific and actionable."#,
            step_number, step_type, description, rationale
        )
    }

    /// Template applier prompt - fills in solution templates
    pub const TEMPLATE_APPLIER: &str = r#"You are applying a solution template. Fill in the template with appropriate values based on the context and step outputs.

Return the filled template ready to use."#;

    /// Solution generator prompt - generates final solution from steps
    /// Returns a formatted prompt with the given pattern name
    pub fn solution_generator(pattern_name: &str) -> String {
        format!(
            r#"You completed the "{}" pattern. Based on the step outputs, provide a final solution or recommendation.

Be concise and actionable. If code is needed, provide it."#,
            pattern_name
        )
    }
}

/// Synthesis prompts - code generation required (must produce compilable Rust)
pub mod synthesis {
    /// Rust code generator prompt - produces Tool trait implementations
    pub const CODE_GENERATOR: &str = r#"You are a Rust code generator specializing in creating tools for an AI assistant.

Generate a complete, compilable Rust module that implements the Tool trait.

The tool must:
1. Implement the Tool trait with name(), definition(), and execute() methods
2. Return proper ToolDefinition with OpenAI-compatible schema
3. Handle errors gracefully and return ToolResult
4. Be async-safe (Send + Sync)

Template structure:
```rust
use async_trait::async_trait;
use anyhow::Result;
use serde_json::json;

use crate::synthesis::{Tool, ToolArgs, ToolResult, ToolDefinition, FunctionDefinition};

pub struct MyTool;

impl MyTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "my_tool".to_string(),
                description: "Description of what the tool does".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "param1": {
                            "type": "string",
                            "description": "Parameter description"
                        }
                    },
                    "required": ["param1"]
                }),
            },
        }
    }

    async fn execute(&self, args: ToolArgs) -> Result<ToolResult> {
        let param1 = args.get_string("param1")?;

        // Tool implementation here

        Ok(ToolResult::success("Result".to_string()))
    }
}
```

Return ONLY the Rust code, wrapped in ```rust code blocks."#;

    /// Code evolver prompt - improves existing tool implementations
    pub const CODE_EVOLVER: &str = r#"You are a Rust code improvement expert. Your task is to improve an existing tool implementation based on the provided suggestions.

Maintain the same Tool trait implementation structure but improve:
1. Error handling
2. Performance
3. Code clarity
4. Edge case handling

Return the improved code wrapped in ```rust code blocks."#;

    /// Pattern detector prompt - returns structured JSON for pattern analysis
    pub const PATTERN_DETECTOR: &str = r#"You are a code pattern analyzer. Your task is to identify repetitive patterns in code that could be automated as custom tools.

For each pattern you identify, provide:
1. A unique name (snake_case)
2. The pattern type (one of: file_operation, api_call, data_transformation, validation, database_query, config_parsing, error_handling, code_generation, testing, logging, caching)
3. A description of what the pattern does
4. How many times you estimate it occurs (frequency)
5. Your confidence score (0.0 to 1.0) that this is a real, automatable pattern

Focus on patterns that:
- Appear multiple times across the codebase
- Follow a consistent structure
- Could benefit from automation
- Are not already handled by existing tools

Return your analysis as JSON in this exact format:
{
  "patterns": [
    {
      "name": "pattern_name",
      "type": "pattern_type",
      "description": "What this pattern does",
      "frequency": 5,
      "confidence": 0.85,
      "example_files": ["file1.rs", "file2.rs"]
    }
  ]
}"#;
}

/// Analysis prompts - JSON output required for structured analysis
pub mod analysis {
    /// Message analyzer system prompt - returns structured JSON analysis
    pub const MESSAGE_ANALYZER: &str = "You are a precise message analyzer. Analyze the message and output only valid JSON. Required fields: salience (0.0-1.0), topics (array of strings). Optional: contains_code, programming_lang, contains_error, error_type, error_file, error_severity, mood, intensity, intent, summary, relationship_impact.";

    /// Batch message analyzer system prompt - returns JSON array
    pub const BATCH_ANALYZER: &str =
        "You are a precise message analyzer. Analyze each message and output only valid JSON matching the format.";
}

/// Code intelligence prompts - used for code analysis and pattern detection
pub mod code_intelligence {
    /// Design pattern detector - identifies patterns in code
    pub const DESIGN_PATTERN_DETECTOR: &str =
        "You are an expert at identifying design patterns in code.";

    /// Semantic code analyzer - analyzes code semantics and concepts
    pub const SEMANTIC_ANALYZER: &str = "You are an expert at semantic code analysis.";

    /// Domain pattern analyzer - identifies domain-specific patterns and clusters
    pub const DOMAIN_PATTERN_ANALYZER: &str =
        "You are an expert at analyzing code and identifying domain patterns.";
}

/// Summarization prompts - used for conversation summarization
pub mod summarization {
    /// Snapshot summarizer - creates comprehensive conversation snapshots
    pub const SNAPSHOT_SUMMARIZER: &str = "You are a conversation summarizer. Create comprehensive, detailed snapshots that capture the entire arc of a conversation.";

    /// Rolling summarizer - creates incremental technical summaries
    pub const ROLLING_SUMMARIZER: &str = "You are a conversation summarizer. Create detailed, technical summaries that preserve important context and specifics.";
}

/// LLM provider prompts - used for direct LLM operations
pub mod llm {
    /// Code generation specialist - generates clean, working code
    /// Returns a formatted prompt for the given language
    pub fn code_gen_specialist(language: &str) -> String {
        format!(
            "You are a code generation specialist. Generate clean, working code based on the user's requirements.\n\
            Output ONLY valid JSON with this exact structure:\n\
            {{\n  \
              \"path\": \"file/path/here\",\n  \
              \"content\": \"complete file content here\",\n  \
              \"language\": \"{}\",\n  \
              \"explanation\": \"brief explanation of the code\"\n\
            }}\n\n\
            CRITICAL:\n\
            - Generate COMPLETE files, never use '...' or placeholders\n\
            - Include ALL imports, functions, types, and closing braces\n\
            - The content field must contain the entire working file\n\
            - Use proper {} language syntax and best practices",
            language, language
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompts_are_not_empty() {
        // Tool router prompts
        assert!(!tool_router::FILE_READER.is_empty());
        assert!(!tool_router::CODE_SEARCHER.is_empty());
        assert!(!tool_router::FILE_LISTER.is_empty());

        // Pattern prompts (constants)
        assert!(!patterns::PATTERN_MATCHER.is_empty());
        assert!(!patterns::TEMPLATE_APPLIER.is_empty());

        // Pattern prompts (functions)
        assert!(!patterns::step_executor(1, "test", "desc", "").is_empty());
        assert!(!patterns::solution_generator("test").is_empty());

        // Synthesis prompts
        assert!(!synthesis::CODE_GENERATOR.is_empty());
        assert!(!synthesis::CODE_EVOLVER.is_empty());
        assert!(!synthesis::PATTERN_DETECTOR.is_empty());

        // Analysis prompts
        assert!(!analysis::MESSAGE_ANALYZER.is_empty());
        assert!(!analysis::BATCH_ANALYZER.is_empty());

        // Code intelligence prompts
        assert!(!code_intelligence::DESIGN_PATTERN_DETECTOR.is_empty());
        assert!(!code_intelligence::SEMANTIC_ANALYZER.is_empty());
        assert!(!code_intelligence::DOMAIN_PATTERN_ANALYZER.is_empty());

        // Summarization prompts
        assert!(!summarization::SNAPSHOT_SUMMARIZER.is_empty());
        assert!(!summarization::ROLLING_SUMMARIZER.is_empty());

        // LLM prompts (functions)
        assert!(!llm::code_gen_specialist("rust").is_empty());
    }

    #[test]
    fn test_function_prompts_include_args() {
        // Functions should include their arguments in output
        let step_prompt = patterns::step_executor(42, "analyze", "Check code", "For safety");
        assert!(step_prompt.contains("42"));
        assert!(step_prompt.contains("analyze"));
        assert!(step_prompt.contains("Check code"));
        assert!(step_prompt.contains("For safety"));

        let solution_prompt = patterns::solution_generator("my_pattern");
        assert!(solution_prompt.contains("my_pattern"));

        // LLM code gen specialist should include language
        let code_gen = llm::code_gen_specialist("typescript");
        assert!(code_gen.contains("typescript"));
    }
}
