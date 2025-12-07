// src/llm/provider/openai/types.rs
// Type definitions for OpenAI Responses API (December 2025)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OpenAI model variants (all use Responses API)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAIModel {
    /// GPT-5.1 - Main model for voice/chat tier
    Gpt51,
    /// GPT-5.1-Codex-Mini - Fast tier for simple tasks
    Gpt51Mini,
    /// GPT-5.1-Codex-Max - Code tier for code-heavy tasks and agentic tier for long-running
    Gpt51CodexMax,
}

/// Reasoning effort level for models that support extended thinking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// No reasoning tokens - fastest, lowest cost (GPT-5.1's new mode)
    /// Best for: file ops, search, simple queries, low-latency use cases
    None,
    /// Quick reasoning for simple tasks
    Medium,
    /// Standard reasoning for most tasks
    High,
    /// Extended thinking for complex, long-running tasks (Codex-Max only)
    #[serde(rename = "xhigh")]
    XHigh,
}

impl Default for ReasoningEffort {
    fn default() -> Self {
        ReasoningEffort::High
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReasoningEffort::None => write!(f, "none"),
            ReasoningEffort::Medium => write!(f, "medium"),
            ReasoningEffort::High => write!(f, "high"),
            ReasoningEffort::XHigh => write!(f, "xhigh"),
        }
    }
}

/// Reasoning configuration for API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: ReasoningEffort,
}

/// User updates (preamble) configuration for agentic workflows
/// Controls how the model provides progress updates during long-running tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreambleConfig {
    /// How often to provide updates (every N tool calls, default: 6)
    pub frequency: u32,
    /// Verbosity level: "concise" or "detailed"
    pub verbosity: PreambleVerbosity,
    /// Tone: "technical" or "friendly"
    pub tone: PreambleTone,
}

/// Preamble verbosity options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreambleVerbosity {
    /// 1-2 sentences with concrete outcomes
    Concise,
    /// Full context with reasoning
    Detailed,
}

/// Preamble tone options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreambleTone {
    /// Technical, direct communication
    Technical,
    /// Conversational, friendly updates
    Friendly,
}

impl Default for PreambleConfig {
    fn default() -> Self {
        Self {
            frequency: 6,
            verbosity: PreambleVerbosity::Concise,
            tone: PreambleTone::Technical,
        }
    }
}

impl OpenAIModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpenAIModel::Gpt51 => "gpt-5.1",
            OpenAIModel::Gpt51Mini => "gpt-5.1-codex-mini",
            OpenAIModel::Gpt51CodexMax => "gpt-5.1-codex-max",
        }
    }

    /// Get the display name for this model
    pub fn display_name(&self) -> &'static str {
        match self {
            OpenAIModel::Gpt51 => "GPT-5.1",
            OpenAIModel::Gpt51Mini => "GPT-5.1 Codex Mini",
            OpenAIModel::Gpt51CodexMax => "GPT-5.1 Codex Max",
        }
    }

    /// Get max context window size
    /// Note: Codex-Max uses compaction for effectively unlimited context,
    /// but we report 1M as a conservative working limit
    pub fn max_context_tokens(&self) -> i64 {
        match self {
            OpenAIModel::Gpt51 => 272_000,
            OpenAIModel::Gpt51Mini => 400_000,
            OpenAIModel::Gpt51CodexMax => 1_000_000, // Compaction handles overflow
        }
    }

    /// Get max output tokens
    pub fn max_output_tokens(&self) -> i64 {
        match self {
            OpenAIModel::Gpt51 => 128_000,
            OpenAIModel::Gpt51Mini => 128_000,
            OpenAIModel::Gpt51CodexMax => 128_000,
        }
    }

    /// Check if this model supports reasoning effort configuration
    pub fn supports_reasoning(&self) -> bool {
        // All GPT-5.1 models support reasoning effort
        true
    }
}

impl Default for OpenAIModel {
    fn default() -> Self {
        OpenAIModel::Gpt51
    }
}

impl std::fmt::Display for OpenAIModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Responses API Types (December 2025)
// All GPT-5.1 models use /v1/responses endpoint
// ============================================================================

/// OpenAI Responses API request
#[derive(Debug, Clone, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    /// Input can be a string or array of input items
    pub input: ResponsesInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// Reasoning configuration for reasoning-capable models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    /// Reference to previous response for conversation chaining
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
}

/// Input type for Responses API - can be a simple string or array of items
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    /// Simple string input
    Text(String),
    /// Array of input items (messages, function outputs, etc.)
    Items(Vec<InputItem>),
}

/// Input item types for Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    /// Message input with role and content
    Message {
        role: String,
        content: MessageContent,
    },
    /// Function call output (response to a tool call)
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

/// Message content - can be a string or array of content parts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content
    Text(String),
    /// Array of content parts (for multimodal)
    Parts(Vec<ContentPart>),
}

/// Content part for multimodal messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Text content
    InputText { text: String },
    /// Image content
    InputImage { image_url: String },
    /// File content
    InputFile { file_id: String },
}

/// Tool definition for Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    /// Enable strict mode for structured outputs (near-zero parsing errors)
    /// When true, model output always conforms to the parameter schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// OpenAI Responses API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub status: String,
    /// Array of output items (messages, function calls, etc.)
    #[serde(default)]
    pub output: Vec<OutputItem>,
    /// Convenience field with just the text output
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
    #[serde(default)]
    pub error: Option<ResponsesError>,
}

/// Output item types from Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputItem {
    /// Message output from assistant
    Message {
        id: String,
        role: String,
        content: Vec<OutputContent>,
        #[serde(default)]
        status: String,
    },
    /// Function call from model (custom tools)
    FunctionCall {
        #[serde(default)]
        id: Option<String>,
        call_id: String,
        name: String,
        arguments: String,
    },
    /// Native apply_patch call from model (GPT-5.1 built-in file editing)
    /// Uses V4A diff format for reliable file operations
    ApplyPatchCall {
        #[serde(default)]
        id: Option<String>,
        call_id: String,
        /// V4A format patch content
        patch: String,
    },
    /// Native shell call from model (GPT-5.1 built-in command execution)
    ShellCall {
        #[serde(default)]
        id: Option<String>,
        call_id: String,
        /// Command to execute
        command: Vec<String>,
        /// Working directory (optional)
        #[serde(default)]
        workdir: Option<String>,
        /// Timeout in seconds (default 120)
        #[serde(default)]
        timeout: Option<u32>,
    },
    /// Catch-all for unknown output types
    #[serde(other)]
    Unknown,
}

/// Output content types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputContent {
    /// Text output
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Vec<Value>,
        #[serde(default)]
        logprobs: Vec<Value>,
    },
    /// Catch-all for unknown content types
    #[serde(other)]
    Unknown,
}

/// Token usage for Responses API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

/// Detailed input token breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: i64,
}

/// Detailed output token breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: i64,
}

/// Error in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesError {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: String,
}

/// Streaming event from Responses API
/// Events include: response.created, response.in_progress, response.output_item.added,
/// response.content_part.added, response.output_text.delta, response.output_text.done,
/// response.content_part.done, response.output_item.done, response.completed
#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub sequence_number: i64,
    #[serde(default)]
    pub response: Option<ResponsesResponse>,
    /// Text delta for response.output_text.delta events
    #[serde(default)]
    pub delta: Option<String>,
    #[serde(default)]
    pub item: Option<Value>,
    #[serde(default)]
    pub part: Option<Value>,
    /// Full text for response.output_text.done events
    #[serde(default)]
    pub text: Option<String>,
}

/// OpenAI API error response
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

/// Error detail
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: Option<String>,
}

// ============================================================================
// Legacy Chat Completions Types (kept for compatibility during transition)
// These will be removed once all code migrates to Responses API
// ============================================================================

/// OpenAI chat completion request (legacy - use ResponsesRequest)
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
}

/// Chat message for legacy API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Tool call in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCallMessage,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallMessage {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for legacy API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// OpenAI chat completion response (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// Choice in completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: i64,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// Message in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallMessage>>,
}

/// Token usage (legacy format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// Streaming chunk (legacy)
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

/// Choice in streaming chunk
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkChoice {
    pub index: i64,
    pub delta: DeltaMessage,
    pub finish_reason: Option<String>,
}

/// Delta message in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaMessage {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

/// Tool call delta in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaToolCall {
    pub index: i64,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub call_type: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFunction>,
}

/// Function delta in streaming
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

// ============================================================================
// Native Tool Types (GPT-5.1 Built-in Tools)
// ============================================================================

/// Native tool type identifiers for Responses API
/// These are built-in tools that GPT-5.1 knows how to use natively
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeToolType {
    /// File editing with V4A diff format - 35% fewer failures than custom tools
    ApplyPatch,
    /// Command execution with proper timeout handling
    Shell,
}

impl NativeToolType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NativeToolType::ApplyPatch => "apply_patch",
            NativeToolType::Shell => "shell",
        }
    }
}

/// Create a native apply_patch tool definition
pub fn native_apply_patch_tool() -> ResponsesTool {
    ResponsesTool {
        tool_type: "apply_patch".to_string(),
        name: None,
        description: None,
        parameters: None,
        strict: None, // Native tools have their own semantics
    }
}

/// Create a native shell tool definition
pub fn native_shell_tool() -> ResponsesTool {
    ResponsesTool {
        tool_type: "shell".to_string(),
        name: None,
        description: None,
        parameters: None,
        strict: None, // Native tools have their own semantics
    }
}

/// Parsed V4A patch operation from apply_patch_call
#[derive(Debug, Clone)]
pub struct PatchOperation {
    /// File path
    pub path: String,
    /// Operation type
    pub op_type: PatchOpType,
    /// File content (for create) or diff hunks (for update)
    pub content: String,
}

/// V4A patch operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchOpType {
    /// Create a new file
    Create,
    /// Update existing file with diff
    Update,
    /// Delete a file
    Delete,
}

impl PatchOperation {
    /// Parse V4A format patch into operations
    /// V4A format:
    /// *** Begin Patch
    /// *** Add File: path/to/new/file.txt
    /// content here
    /// *** Update File: path/to/existing.txt
    /// @@ -start,count +start,count @@
    ///  context
    /// -removed
    /// +added
    /// *** Delete File: path/to/delete.txt
    /// *** End Patch
    pub fn parse_v4a(patch: &str) -> Vec<PatchOperation> {
        let mut operations = Vec::new();
        let mut current_path: Option<String> = None;
        let mut current_type: Option<PatchOpType> = None;
        let mut current_content = String::new();
        let mut in_patch = false;

        for line in patch.lines() {
            let trimmed = line.trim();

            if trimmed == "*** Begin Patch" {
                in_patch = true;
                continue;
            }

            if trimmed == "*** End Patch" {
                // Save last operation
                if let (Some(path), Some(op_type)) = (current_path.take(), current_type.take()) {
                    operations.push(PatchOperation {
                        path,
                        op_type,
                        content: std::mem::take(&mut current_content),
                    });
                }
                break;
            }

            if !in_patch {
                continue;
            }

            // Check for file operation headers
            if let Some(path) = trimmed.strip_prefix("*** Add File: ") {
                // Save previous operation
                if let (Some(p), Some(t)) = (current_path.take(), current_type.take()) {
                    operations.push(PatchOperation {
                        path: p,
                        op_type: t,
                        content: std::mem::take(&mut current_content),
                    });
                }
                current_path = Some(path.to_string());
                current_type = Some(PatchOpType::Create);
                current_content.clear();
            } else if let Some(path) = trimmed.strip_prefix("*** Update File: ") {
                if let (Some(p), Some(t)) = (current_path.take(), current_type.take()) {
                    operations.push(PatchOperation {
                        path: p,
                        op_type: t,
                        content: std::mem::take(&mut current_content),
                    });
                }
                current_path = Some(path.to_string());
                current_type = Some(PatchOpType::Update);
                current_content.clear();
            } else if let Some(path) = trimmed.strip_prefix("*** Delete File: ") {
                if let (Some(p), Some(t)) = (current_path.take(), current_type.take()) {
                    operations.push(PatchOperation {
                        path: p,
                        op_type: t,
                        content: std::mem::take(&mut current_content),
                    });
                }
                current_path = Some(path.to_string());
                current_type = Some(PatchOpType::Delete);
                current_content.clear();
            } else if current_path.is_some() {
                // Accumulate content
                if !current_content.is_empty() {
                    current_content.push('\n');
                }
                current_content.push_str(line);
            }
        }

        operations
    }
}

/// Result of executing a shell command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResult {
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: i32,
    /// Whether command timed out
    pub timed_out: bool,
}

impl ShellResult {
    /// Format result for sending back to model
    pub fn to_output(&self, max_length: usize) -> String {
        let mut output = String::new();

        if !self.stdout.is_empty() {
            output.push_str(&truncate_output(&self.stdout, max_length / 2));
        }

        if !self.stderr.is_empty() {
            if !output.is_empty() {
                output.push_str("\n\n[stderr]\n");
            }
            output.push_str(&truncate_output(&self.stderr, max_length / 2));
        }

        if self.timed_out {
            output.push_str("\n\n[TIMEOUT]");
        }

        if output.is_empty() {
            output = format!("[exit code: {}]", self.exit_code);
        }

        output
    }
}

/// Truncate output to max length with indicator
fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...[truncated]", &s[..max_len])
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn test_parse_v4a_create() {
        let patch = r#"*** Begin Patch
*** Add File: src/new_file.rs
fn main() {
    println!("Hello");
}
*** End Patch"#;

        let ops = PatchOperation::parse_v4a(patch);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].path, "src/new_file.rs");
        assert_eq!(ops[0].op_type, PatchOpType::Create);
        assert!(ops[0].content.contains("fn main()"));
    }

    #[test]
    fn test_parse_v4a_update() {
        let patch = r#"*** Begin Patch
*** Update File: src/lib.rs
@@ -10,3 +10,4 @@
 existing line
-old line
+new line
+another new line
*** End Patch"#;

        let ops = PatchOperation::parse_v4a(patch);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].path, "src/lib.rs");
        assert_eq!(ops[0].op_type, PatchOpType::Update);
        assert!(ops[0].content.contains("-old line"));
        assert!(ops[0].content.contains("+new line"));
    }

    #[test]
    fn test_parse_v4a_multiple() {
        let patch = r#"*** Begin Patch
*** Add File: new.txt
content
*** Update File: existing.txt
@@ -1,1 +1,1 @@
-old
+new
*** Delete File: remove.txt
*** End Patch"#;

        let ops = PatchOperation::parse_v4a(patch);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].op_type, PatchOpType::Create);
        assert_eq!(ops[1].op_type, PatchOpType::Update);
        assert_eq!(ops[2].op_type, PatchOpType::Delete);
    }
}
