// crates/mira-server/src/llm/prompt.rs
// PromptBuilder for consistent prompt construction to maximize KV cache hit rate

use super::Message;

/// Static shared prefix for all Mira prompts
/// This section remains identical across all calls to maximize KV cache reuse
const STATIC_PREFIX: &str = r#"You are Mira, an AI assistant designed to help with software engineering tasks.
Core principles:
- Be accurate, thorough, and practical
- Prioritize security, performance, and maintainability
- Use available tools to explore codebases before making assumptions
- Provide actionable advice with clear reasoning

Safety guidelines:
- Never generate harmful, unethical, or malicious content
- Respect user privacy and data security
- Follow best practices for secure coding
"#;

/// Tool usage guidance (appended when tools are available)
const TOOL_GUIDANCE: &str = r#"Use tools to explore codebase before analysis. Don't ask for context - use tools:

- search_code: Find code by meaning (e.g., "authentication", "error handling")
- get_symbols: See file structure (functions, structs)
- read_file: Read file contents
- find_callers: See what calls a function
- find_callees: See what a function calls
- recall: Retrieve past decisions and context

Explore proactively based on task. Verify code with tools before making assumptions."#;

/// PromptBuilder constructs standardized prompts with a "funnel" structure:
/// 1. Static shared prefix (same across all calls)
/// 2. Semi-static role/task definition (role-specific instructions)
/// 3. Dynamic user context (task description, code snippets, questions)
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    role_instructions: String,
    include_tool_guidance: bool,
}

impl PromptBuilder {
    /// Create a new PromptBuilder with role-specific instructions
    pub fn new(role_instructions: impl Into<String>) -> Self {
        Self {
            role_instructions: role_instructions.into(),
            include_tool_guidance: false,
        }
    }

    /// Include tool usage guidance (for expert consultations and tool-using tasks)
    pub fn with_tool_guidance(mut self) -> Self {
        self.include_tool_guidance = true;
        self
    }

    /// Build the complete system prompt
    pub fn build_system_prompt(&self) -> String {
        let mut prompt = STATIC_PREFIX.to_string();
        prompt.push_str("\n\n");
        prompt.push_str(&self.role_instructions);

        if self.include_tool_guidance {
            prompt.push_str("\n\n");
            prompt.push_str(TOOL_GUIDANCE);
        }

        prompt
    }

    /// Build a vector of messages with system prompt and user content
    pub fn build_messages(&self, user_content: impl Into<String>) -> Vec<Message> {
        vec![
            Message::system(self.build_system_prompt()),
            Message::user(user_content),
        ]
    }

    /// Factory method for expert consultations
    pub fn for_expert(role_name: &str, role_description: &str) -> Self {
        let instructions = format!(
            r#"You are a {role_name}.

Your role:
{role_description}

When responding:
1. Start with key recommendation
2. Explain reasoning
3. Present alternatives with tradeoffs
4. Be specific - reference patterns or technologies
5. Prioritize issues by impact

You are advisory - analyze and recommend, not implement."#
        );

        Self::new(instructions).with_tool_guidance()
    }

    /// Factory method for code health analysis (complexity)
    pub fn for_code_health_complexity() -> Self {
        let instructions = "You are a code reviewer focused on function complexity and maintainability. Be direct and concise.";
        Self::new(instructions)
    }

    /// Factory method for code health analysis (error handling quality)
    pub fn for_code_health_error_quality() -> Self {
        let instructions = "You are a code reviewer focused on error handling quality and debuggability. Be direct and concise.";
        Self::new(instructions)
    }

    /// Factory method for capabilities scanning
    pub fn for_capabilities() -> Self {
        let instructions = "You are a codebase analyst extracting implemented capabilities. List what users and developers can DO with the codebase, focusing on working features.";
        Self::new(instructions)
    }

    /// Factory method for summaries and briefings
    pub fn for_summaries() -> Self {
        let instructions = "You are a technical writer creating concise summaries of codebases, sessions, or discussions. Focus on key points, decisions, and actionable information.";
        Self::new(instructions)
    }

    /// Factory method for tool extraction (MCP protocol)
    pub fn for_tool_extraction() -> Self {
        let instructions = "You are a protocol analyzer extracting tool definitions from code. Identify MCP tool implementations, their parameters, and descriptions.";
        Self::new(instructions)
    }

    /// Factory method for general briefings
    pub fn for_briefings() -> Self {
        let instructions = "You are a project analyst providing briefings on codebase status, recent changes, and recommendations.";
        Self::new(instructions)
    }

    /// Factory method for background pondering/reasoning
    pub fn for_background() -> Self {
        let instructions = r#"You are an observant analyst studying developer behavior patterns and project evolution.

Your role:
- Identify meaningful patterns in tool usage and workflows
- Spot friction points and inefficiencies
- Notice recurring themes in project decisions
- Suggest workflow improvements

Be concise and data-driven. Only report insights with clear supporting evidence."#;
        Self::new(instructions)
    }

    /// Factory method for documentation generation
    pub fn for_documentation() -> Self {
        let instructions = r#"You are a technical writer creating documentation for software projects.

CRITICAL RULES:
1. ONLY document what is explicitly shown in the provided code and context
2. NEVER invent, hallucinate, or assume features, parameters, or behaviors not shown
3. If information is missing, state "Not documented" rather than guessing
4. Use the EXACT function/type names, parameters, and signatures from the provided code
5. Write all code examples in the language specified in the prompt (Rust, Python, etc.)
6. Output ONLY the markdown documentation - no preamble, no "Let me explore", no code execution attempts

Write clear, accurate markdown that helps developers understand and use the code."#;
        Self::new(instructions)
    }

    /// Factory method for semantic diff analysis
    pub fn for_diff_analysis() -> Self {
        let instructions = r#"Analyze git diffs and classify each change semantically.

CHANGE TYPES:
- NewFunction: A new function/method was added
- ModifiedFunction: An existing function was changed
- DeletedFunction: A function was removed
- SignatureChange: Function signature changed (parameters, return type)
- Refactoring: Code reorganization without behavior change
- BugFix: Fix for a bug or error condition
- ConfigChange: Configuration or settings changes
- TestChange: Test code modifications
- Documentation: Comments or documentation changes

For each meaningful change, identify:
1. What type of change is it?
2. Is it a breaking change? (API changes, removed features, signature changes)
3. Is it security-relevant? (auth, input handling, SQL, file access, crypto)

Output valid JSON with this structure:
{
  "changes": [
    {
      "change_type": "NewFunction",
      "file_path": "src/example.rs",
      "symbol_name": "function_name",
      "description": "Brief description of what changed",
      "breaking": false,
      "security_relevant": false
    }
  ],
  "summary": "One paragraph summary of all changes",
  "risk_flags": ["flag1", "flag2"]
}

Risk flags to consider: breaking_api, security_change, removes_feature, complex_refactor, database_migration, auth_change, input_validation"#;
        Self::new(instructions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // PromptBuilder construction tests
    // ============================================================================

    #[test]
    fn test_prompt_builder_new() {
        let builder = PromptBuilder::new("Test instructions");
        assert_eq!(builder.role_instructions, "Test instructions");
        assert!(!builder.include_tool_guidance);
    }

    #[test]
    fn test_prompt_builder_with_tool_guidance() {
        let builder = PromptBuilder::new("Test").with_tool_guidance();
        assert!(builder.include_tool_guidance);
    }

    #[test]
    fn test_prompt_builder_clone() {
        let builder = PromptBuilder::new("Test").with_tool_guidance();
        let cloned = builder.clone();
        assert_eq!(builder.role_instructions, cloned.role_instructions);
        assert_eq!(builder.include_tool_guidance, cloned.include_tool_guidance);
    }

    // ============================================================================
    // build_system_prompt tests
    // ============================================================================

    #[test]
    fn test_build_system_prompt_includes_static_prefix() {
        let builder = PromptBuilder::new("Custom instructions");
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("Core principles"));
        assert!(prompt.contains("Safety guidelines"));
    }

    #[test]
    fn test_build_system_prompt_includes_role_instructions() {
        let builder = PromptBuilder::new("My custom role instructions here");
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("My custom role instructions here"));
    }

    #[test]
    fn test_build_system_prompt_without_tool_guidance() {
        let builder = PromptBuilder::new("Test");
        let prompt = builder.build_system_prompt();
        assert!(!prompt.contains("search_code"));
        assert!(!prompt.contains("get_symbols"));
    }

    #[test]
    fn test_build_system_prompt_with_tool_guidance() {
        let builder = PromptBuilder::new("Test").with_tool_guidance();
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("search_code"));
        assert!(prompt.contains("get_symbols"));
        assert!(prompt.contains("find_callers"));
        assert!(prompt.contains("recall"));
    }

    // ============================================================================
    // build_messages tests
    // ============================================================================

    #[test]
    fn test_build_messages_structure() {
        let builder = PromptBuilder::new("Role instructions");
        let messages = builder.build_messages("User content here");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
    }

    #[test]
    fn test_build_messages_content() {
        let builder = PromptBuilder::new("Role instructions");
        let messages = builder.build_messages("User content here");

        assert!(
            messages[0]
                .content
                .as_ref()
                .unwrap()
                .contains("Role instructions")
        );
        assert_eq!(messages[1].content.as_ref().unwrap(), "User content here");
    }

    // ============================================================================
    // Factory method tests
    // ============================================================================

    #[test]
    fn test_for_expert() {
        let builder =
            PromptBuilder::for_expert("Security Expert", "Analyze code for vulnerabilities");
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("Security Expert"));
        assert!(prompt.contains("Analyze code for vulnerabilities"));
        assert!(prompt.contains("advisory")); // Expert role mentions being advisory
        assert!(builder.include_tool_guidance);
    }

    #[test]
    fn test_for_code_health_complexity() {
        let builder = PromptBuilder::for_code_health_complexity();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("complexity"));
        assert!(prompt.contains("maintainability"));
        assert!(!builder.include_tool_guidance);
    }

    #[test]
    fn test_for_code_health_error_quality() {
        let builder = PromptBuilder::for_code_health_error_quality();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("error handling"));
        assert!(prompt.contains("debuggability"));
    }

    #[test]
    fn test_for_capabilities() {
        let builder = PromptBuilder::for_capabilities();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("capabilities"));
        assert!(prompt.contains("working features"));
    }

    #[test]
    fn test_for_summaries() {
        let builder = PromptBuilder::for_summaries();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("technical writer"));
        assert!(prompt.contains("summaries"));
    }

    #[test]
    fn test_for_tool_extraction() {
        let builder = PromptBuilder::for_tool_extraction();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("protocol analyzer"));
        assert!(prompt.contains("MCP"));
    }

    #[test]
    fn test_for_briefings() {
        let builder = PromptBuilder::for_briefings();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("project analyst"));
        assert!(prompt.contains("briefings"));
    }

    #[test]
    fn test_for_documentation() {
        let builder = PromptBuilder::for_documentation();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("technical writer"));
        assert!(prompt.contains("documentation"));
        assert!(prompt.contains("NEVER invent"));
        assert!(prompt.contains("hallucinate"));
    }

    #[test]
    fn test_for_diff_analysis() {
        let builder = PromptBuilder::for_diff_analysis();
        let prompt = builder.build_system_prompt();

        assert!(prompt.contains("git diffs"));
        assert!(prompt.contains("CHANGE TYPES"));
        assert!(prompt.contains("NewFunction"));
        assert!(prompt.contains("ModifiedFunction"));
        assert!(prompt.contains("breaking"));
        assert!(prompt.contains("security-relevant"));
    }

    // ============================================================================
    // Static content tests
    // ============================================================================

    #[test]
    fn test_static_prefix_content() {
        // Verify STATIC_PREFIX contains expected content
        assert!(STATIC_PREFIX.contains("Mira"));
        assert!(STATIC_PREFIX.contains("Core principles"));
        assert!(STATIC_PREFIX.contains("accurate"));
        assert!(STATIC_PREFIX.contains("security"));
    }

    #[test]
    fn test_tool_guidance_content() {
        // Verify TOOL_GUIDANCE contains expected tools
        assert!(TOOL_GUIDANCE.contains("search_code"));
        assert!(TOOL_GUIDANCE.contains("get_symbols"));
        assert!(TOOL_GUIDANCE.contains("read_file"));
        assert!(TOOL_GUIDANCE.contains("find_callers"));
        assert!(TOOL_GUIDANCE.contains("find_callees"));
        assert!(TOOL_GUIDANCE.contains("recall"));
    }
}
