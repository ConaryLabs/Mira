// src/operations/engine/delegation.rs
// DeepSeek delegation for code generation tasks

use crate::git::client::FileNode;
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::recall_engine::RecallContext;
use crate::operations::engine::context::ContextBuilder;

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct DelegationHandler {
    deepseek: DeepSeekProvider,
}

impl DelegationHandler {
    pub fn new(deepseek: DeepSeekProvider) -> Self {
        Self { deepseek }
    }

    /// Delegate to DeepSeek with enriched context
    pub async fn delegate_to_deepseek(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        cancel_token: Option<CancellationToken>,
        file_tree: Option<&Vec<FileNode>>,
        code_context: Option<&Vec<MemoryEntry>>,
        recall_context: &RecallContext,
    ) -> Result<serde_json::Value> {
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!(
                    "Operation cancelled before DeepSeek delegation"
                ));
            }
        }

        info!("Delegating {} to DeepSeek", tool_name);

        // Build enriched context from all sources
        let enriched_context =
            ContextBuilder::build_enriched_context(&args, file_tree, code_context, recall_context);

        // Build CodeGenRequest based on tool type
        let request = match tool_name {
            "generate_code" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("task").and_then(|v| v.as_str()))
                    .unwrap_or("Generate code")
                    .to_string();

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: args
                        .get("framework")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: enriched_context,
                }
            }
            "modify_code" | "refactor_code" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let instructions = args
                    .get("instructions")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("refactoring_goals").and_then(|v| v.as_str()))
                    .unwrap_or("Modify code")
                    .to_string();
                let existing = args
                    .get("existing_code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let description = if existing.is_empty() {
                    instructions
                } else {
                    format!("{}\n\nExisting code:\n{}", instructions, existing)
                };

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: args
                        .get("framework")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: enriched_context,
                }
            }
            "fix_code" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let error_msg = args
                    .get("error_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Fix error");
                let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");

                let description = format!(
                    "Fix the following error:\n{}\n\nExisting code:\n{}",
                    error_msg, code
                );

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: args
                        .get("framework")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: enriched_context,
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown tool: {}", tool_name));
            }
        };

        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!(
                    "Operation cancelled during DeepSeek request"
                ));
            }
        }

        let response = self.deepseek.generate_code(request).await?;

        Ok(serde_json::json!({
            "artifact": {
                "path": response.artifact.path,
                "content": response.artifact.content,
                "language": response.artifact.language,
                "explanation": response.artifact.explanation,
            }
        }))
    }
}
