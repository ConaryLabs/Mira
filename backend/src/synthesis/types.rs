// src/synthesis/types.rs
// Core types for tool synthesis system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pattern types that can be synthesized into tools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    FileOperation,
    ApiCall,
    DataTransformation,
    Validation,
    DatabaseQuery,
    ConfigParsing,
    ErrorHandling,
    CodeGeneration,
    Testing,
    Logging,
    Caching,
    Other(String),
}

impl PatternType {
    pub fn as_str(&self) -> &str {
        match self {
            PatternType::FileOperation => "file_operation",
            PatternType::ApiCall => "api_call",
            PatternType::DataTransformation => "data_transformation",
            PatternType::Validation => "validation",
            PatternType::DatabaseQuery => "database_query",
            PatternType::ConfigParsing => "config_parsing",
            PatternType::ErrorHandling => "error_handling",
            PatternType::CodeGeneration => "code_generation",
            PatternType::Testing => "testing",
            PatternType::Logging => "logging",
            PatternType::Caching => "caching",
            PatternType::Other(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "file_operation" => PatternType::FileOperation,
            "api_call" => PatternType::ApiCall,
            "data_transformation" => PatternType::DataTransformation,
            "validation" => PatternType::Validation,
            "database_query" => PatternType::DatabaseQuery,
            "config_parsing" => PatternType::ConfigParsing,
            "error_handling" => PatternType::ErrorHandling,
            "code_generation" => PatternType::CodeGeneration,
            "testing" => PatternType::Testing,
            "logging" => PatternType::Logging,
            "caching" => PatternType::Caching,
            other => PatternType::Other(other.to_string()),
        }
    }
}

/// Location where a pattern was detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternLocation {
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub symbol_name: Option<String>,
}

/// A detected pattern that could become a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPattern {
    pub id: Option<i64>,
    pub project_id: String,
    pub pattern_name: String,
    pub pattern_type: PatternType,
    pub description: String,
    pub detected_occurrences: i64,
    pub example_locations: Vec<PatternLocation>,
    pub confidence_score: f64,
    pub should_synthesize: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ToolPattern {
    pub fn new(
        project_id: String,
        pattern_name: String,
        pattern_type: PatternType,
        description: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            project_id,
            pattern_name,
            pattern_type,
            description,
            detected_occurrences: 1,
            example_locations: Vec::new(),
            confidence_score: 0.0,
            should_synthesize: false,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Compilation status for a synthesized tool
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CompilationStatus {
    Pending,
    Compiling,
    Success,
    Failed,
}

impl CompilationStatus {
    pub fn as_str(&self) -> &str {
        match self {
            CompilationStatus::Pending => "pending",
            CompilationStatus::Compiling => "compiling",
            CompilationStatus::Success => "success",
            CompilationStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => CompilationStatus::Pending,
            "compiling" => CompilationStatus::Compiling,
            "success" => CompilationStatus::Success,
            "failed" => CompilationStatus::Failed,
            _ => CompilationStatus::Pending,
        }
    }
}

/// A synthesized tool generated from a pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizedTool {
    pub id: String,
    pub project_id: String,
    pub tool_pattern_id: Option<i64>,
    pub name: String,
    pub description: String,
    pub version: i64,
    pub source_code: String,
    pub language: String,
    pub compilation_status: CompilationStatus,
    pub compilation_error: Option<String>,
    pub binary_path: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SynthesizedTool {
    pub fn new(
        project_id: String,
        name: String,
        description: String,
        source_code: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_id,
            tool_pattern_id: None,
            name,
            description,
            version: 1,
            source_code,
            language: "rust".to_string(),
            compilation_status: CompilationStatus::Pending,
            compilation_error: None,
            binary_path: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Record of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: Option<i64>,
    pub tool_id: String,
    pub operation_id: Option<String>,
    pub session_id: String,
    pub user_id: Option<String>,
    pub arguments: Option<Value>,
    pub success: bool,
    pub output: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: i64,
    pub executed_at: DateTime<Utc>,
}

/// Effectiveness metrics for a synthesized tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEffectiveness {
    pub tool_id: String,
    pub total_executions: i64,
    pub successful_executions: i64,
    pub failed_executions: i64,
    pub average_duration_ms: Option<f64>,
    pub total_time_saved_ms: i64,
    pub last_executed: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ToolEffectiveness {
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            return 0.0;
        }
        self.successful_executions as f64 / self.total_executions as f64
    }

    pub fn is_below_threshold(&self, threshold: f64) -> bool {
        self.success_rate() < threshold
    }
}

/// User feedback on a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFeedback {
    pub id: Option<i64>,
    pub tool_id: String,
    pub execution_id: Option<i64>,
    pub user_id: String,
    pub rating: Option<i64>,
    pub comment: Option<String>,
    pub issue_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Reason for tool evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionReason {
    LowEffectiveness,
    UserFeedback,
    Manual,
    PatternChange,
}

impl EvolutionReason {
    pub fn as_str(&self) -> &str {
        match self {
            EvolutionReason::LowEffectiveness => "low_effectiveness",
            EvolutionReason::UserFeedback => "user_feedback",
            EvolutionReason::Manual => "manual",
            EvolutionReason::PatternChange => "pattern_change",
        }
    }
}

/// Record of a tool evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEvolution {
    pub id: Option<i64>,
    pub tool_id: String,
    pub old_version: i64,
    pub new_version: i64,
    pub change_description: String,
    pub motivation: Option<String>,
    pub source_code_diff: Option<String>,
    pub evolved_at: DateTime<Utc>,
}

/// Result of code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResult {
    pub success: bool,
    pub tool_name: String,
    pub source_code: Option<String>,
    pub error: Option<String>,
    pub attempts: u32,
    pub validation_errors: Vec<String>,
}

impl GenerationResult {
    pub fn success(tool_name: String, source_code: String, attempts: u32) -> Self {
        Self {
            success: true,
            tool_name,
            source_code: Some(source_code),
            error: None,
            attempts,
            validation_errors: Vec::new(),
        }
    }

    pub fn failure(tool_name: String, error: String, validation_errors: Vec<String>) -> Self {
        Self {
            success: false,
            tool_name,
            source_code: None,
            error: Some(error),
            attempts: 0,
            validation_errors,
        }
    }
}

/// Result of tool compilation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationResult {
    pub success: bool,
    pub tool_name: String,
    pub binary_path: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// OpenAI-compatible tool definition for dynamic tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Arguments passed to a tool execution
#[derive(Debug, Clone)]
pub struct ToolArgs {
    args: std::collections::HashMap<String, Value>,
}

impl ToolArgs {
    pub fn from_json(value: Value) -> anyhow::Result<Self> {
        let args = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be a JSON object"))?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(ToolArgs { args })
    }

    pub fn get_string(&self, key: &str) -> anyhow::Result<String> {
        self.args
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid string argument: {}", key))
    }

    pub fn get_optional_string(&self, key: &str) -> Option<String> {
        self.args
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub fn get_integer(&self, key: &str) -> anyhow::Result<i64> {
        self.args
            .get(key)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid integer argument: {}", key))
    }

    pub fn get_optional_integer(&self, key: &str) -> Option<i64> {
        self.args.get(key).and_then(|v| v.as_i64())
    }

    pub fn get_boolean(&self, key: &str, default: bool) -> bool {
        self.args
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }
}

/// Result returned from a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub metadata: Option<Value>,
}

impl ToolResult {
    pub fn success(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
            metadata: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Statistics about the synthesis system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisStats {
    pub total_patterns: i64,
    pub patterns_with_tools: i64,
    pub total_tools: i64,
    pub active_tools: i64,
    pub total_executions: i64,
    pub successful_executions: i64,
    pub average_success_rate: f64,
    pub tools_below_threshold: i64,
}
