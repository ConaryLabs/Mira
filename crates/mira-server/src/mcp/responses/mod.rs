//! Structured output types for MCP tools.
//!
//! Each tool returns a wrapper struct with `action`, `message`, and optional typed `data`.
//! Using `Json<T>` return types, rmcp auto-infers `outputSchema` for each tool.
//! The root type is always an object (MCP requirement).
//!
//! Domain-specific types are split into per-tool modules; this root re-exports everything
//! so existing `use crate::mcp::responses::X` imports continue to work.

mod code;
mod diff;
mod documentation;
mod goal;
mod index;
mod memory;
mod project;
mod recipe;
mod session;
mod tasks;
pub mod team;

// Re-export all domain types (preserves existing import paths)
pub use code::*;
pub use diff::*;
pub use documentation::*;
pub use goal::*;
pub use index::*;
pub use memory::*;
pub use project::*;
pub use recipe::*;
pub use session::*;
pub use tasks::*;
pub use team::*;

use rmcp::ErrorData;
use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Serialize;
use std::borrow::Cow;

/// Trait for outputs that expose a human-readable message.
pub trait HasMessage {
    fn message(&self) -> &str;
}

/// Generic tool output with action, message, and optional typed data.
///
/// All MCP tools return this shape. Concrete output types are type aliases:
/// `pub type MemoryOutput = ToolOutput<MemoryData>;`
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolOutput<D> {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<D>,
}

impl<D> HasMessage for ToolOutput<D> {
    fn message(&self) -> &str {
        &self.message
    }
}

/// JSON wrapper that preserves human-readable `message` in MCP content.
pub struct Json<T>(pub T);

// Implement JsonSchema for Json<T> to delegate to T's schema
impl<T: JsonSchema> JsonSchema for Json<T> {
    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        T::json_schema(generator)
    }
}

impl<T: Serialize + JsonSchema + HasMessage + 'static> IntoCallToolResult for Json<T> {
    fn into_call_tool_result(self) -> Result<CallToolResult, ErrorData> {
        let message = self.0.message().to_string();
        let value = serde_json::to_value(&self.0).map_err(|e| {
            ErrorData::internal_error(
                format!("Failed to serialize structured content: {}", e),
                None,
            )
        })?;

        Ok(CallToolResult {
            content: vec![Content::text(message)],
            structured_content: Some(value),
            is_error: Some(false),
            meta: None,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::tool::schema_for_output;

    #[test]
    fn all_schemas_are_valid_mcp_output() {
        // Each output type must produce a root type "object" schema
        assert!(schema_for_output::<MemoryOutput>().is_ok(), "MemoryOutput");
        assert!(
            schema_for_output::<ProjectOutput>().is_ok(),
            "ProjectOutput"
        );
        assert!(schema_for_output::<CodeOutput>().is_ok(), "CodeOutput");
        assert!(schema_for_output::<GoalOutput>().is_ok(), "GoalOutput");
        assert!(schema_for_output::<IndexOutput>().is_ok(), "IndexOutput");
        assert!(
            schema_for_output::<SessionOutput>().is_ok(),
            "SessionOutput"
        );
        assert!(schema_for_output::<DiffOutput>().is_ok(), "DiffOutput");
        assert!(schema_for_output::<TasksOutput>().is_ok(), "TasksOutput");
        // DocOutput, TeamOutput, RecipeOutput removed from MCP but still valid types for CLI
    }
}
