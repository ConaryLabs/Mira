// src/tools/code_fix.rs
use anyhow::Result;
use std::sync::Arc;
use tracing::{info, warn};

use crate::llm::provider::LlmProvider;
use crate::llm::structured::{CompleteResponse, code_fix_processor};
use crate::llm::provider::Message;

// Re-export ErrorContext from code_fix_processor
pub use crate::llm::structured::code_fix_processor::ErrorContext;

pub struct CodeFixService {
    llm: Arc<dyn LlmProvider>,
}

impl CodeFixService {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    pub async fn process_error_fix(
        &self,
        error_context: &ErrorContext,
        file_content: &str,
        persona: &str,
        context: Vec<String>,
        project_id: Option<i64>,
    ) -> Result<CompleteResponse> {
        info!("🔧 Starting error-to-fix workflow for: {}", error_context.file_path);
        
        let file_lines = file_content.lines().count();
        info!("File has {} lines, error context has {} original lines", 
            file_lines, error_context.original_line_count);

        // PHASE 1: Analysis
        info!("Phase 1 - Analysis");
        
        let analysis_prompt = format!(
            "Analyze this compilation error:\n\
            \n\
            Error: {}\n\
            File: {}\n\
            ({} lines)\n\
            \n\
            ```\n{}\n```",
            error_context.error_message,
            error_context.file_path,
            file_lines,
            file_content
        );

        let system_prompt = format!(
            "You are {}, a helpful AI assistant specialized in debugging and code fixes.",
            persona
        );

        let analysis_messages = vec![Message {
            role: "user".to_string(),
            content: analysis_prompt,
        }];

        let analysis_response = self.llm
            .chat(analysis_messages, system_prompt.clone())
            .await?;

        info!("Phase 1 complete - {} chars of analysis", analysis_response.content.len());

        // PHASE 2: Generate structured fix
        info!("Phase 2 - Generating complete file fix");

        let fix_system_prompt = format!(
            "You are a code fix specialist. Generate complete, working file fixes.\n\
            Context: {}\n\
            Project ID: {:?}",
            context.join(", "),
            project_id
        );

        let fix_request = code_fix_processor::build_code_fix_request(
            &error_context.error_message,
            &error_context.file_path,
            file_content,
            fix_system_prompt,
            vec![],
        )?;

        let fix_messages: Vec<Message> = fix_request["messages"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing messages in fix request"))?
            .iter()
            .filter_map(|m| {
                Some(Message {
                    role: m["role"].as_str()?.to_string(),
                    content: m["content"].as_str()?.to_string(),
                })
            })
            .collect();

        let tools = fix_request["tools"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing tools in fix request"))?
            .clone();

        let fix_response = self.llm
            .chat_with_tools(
                fix_messages,
                fix_request["system"].as_str().unwrap_or("").to_string(),
                tools,
                None,
            )
            .await?;

        let code_fix = code_fix_processor::extract_code_fix_response(&fix_response.raw_response)?;

        info!("Phase 2 complete - Generated {} file(s)", code_fix.files.len());

        let warnings = code_fix.validate_line_counts(error_context);
        if !warnings.is_empty() {
            warn!("Code fix validation warnings:");
            for warning in warnings {
                warn!("  - {}", warning);
            }
        }

        // Build metadata from ToolResponse
        let metadata = crate::llm::structured::LLMMetadata {
            response_id: Some(fix_response.id.clone()),
            model_version: "provider".to_string(),
            prompt_tokens: Some(fix_response.tokens.input),
            completion_tokens: Some(fix_response.tokens.output),
            thinking_tokens: if fix_response.tokens.reasoning > 0 {
                Some(fix_response.tokens.reasoning)
            } else {
                None
            },
            total_tokens: Some(
                fix_response.tokens.input + 
                fix_response.tokens.output + 
                fix_response.tokens.reasoning
            ),
            latency_ms: fix_response.latency_ms,
            finish_reason: Some("tool_use".to_string()),
            temperature: 0.7,
            max_tokens: 4096,
        };

        let complete_response = code_fix.into_complete_response(
            metadata,
            fix_response.raw_response,
        );

        info!("✅ Error-to-fix workflow complete");
        Ok(complete_response)
    }
}
