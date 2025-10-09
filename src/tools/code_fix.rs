// src/tools/code_fix.rs
use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::info;

use crate::llm::provider::{LlmProvider, Message};
use crate::llm::structured::{CompleteResponse, code_fix_processor, has_tool_calls, extract_claude_content_from_tool, extract_claude_metadata, analyze_message_complexity};
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::{UnifiedPromptBuilder, CodeElement, QualityIssue};

pub struct CodeFixHandler {
    llm: Arc<dyn LlmProvider>,
    code_intelligence: Arc<CodeIntelligenceService>,
    sqlite_pool: SqlitePool,
}

impl CodeFixHandler {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        code_intelligence: Arc<CodeIntelligenceService>,
        sqlite_pool: SqlitePool,
    ) -> Self {
        Self {
            llm,
            code_intelligence,
            sqlite_pool,
        }
    }

    /// Two-phase error fix: analyze with thinking → generate structured fix
    pub async fn handle_error_fix(
        &self,
        error_context: &ErrorContext,
        file_content: &str,
        context: &RecallContext,
        persona: &PersonaOverlay,
        project_id: &str,
        metadata: Option<&crate::api::ws::message::MessageMetadata>,
    ) -> Result<CompleteResponse> {
        info!("Two-phase error fix for: {}", error_context.file_path);

        let file_lines = file_content.lines().count();
        info!("Loaded file with {} lines", file_lines);

        // Get code intelligence context
        let code_intel = self.get_code_intelligence_for_file(&error_context.file_path, project_id).await?;

        if code_intel.is_some() {
            info!("Retrieved code intelligence for {}", error_context.file_path);
        }

        // PHASE 1: Deep analysis with thinking
        info!("Phase 1 - Analyzing error with extended thinking");

        let analysis_prompt = format!(
            "Analyze this error and plan how to fix it:\n\n\
             Error: {}\n\
             File: {}\n\
             Lines: {}\n\n\
             What's the root cause and what changes are needed?",
            error_context.error_message,
            error_context.file_path,
            file_lines
        );

        let (thinking_budget, _) = analyze_message_complexity(&error_context.error_message);

        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            persona,
            context,
            None,
            metadata,
            Some(project_id),
        );

        // Use Value::String for content
        let analysis_messages = vec![Message {
            role: "user".to_string(),
            content: analysis_prompt,
        }];

        let analysis_response = self.llm
            .chat(analysis_messages, system_prompt.clone())
            .await?;

        // Extract thinking content
        let thinking = analysis_response.content;
        let analysis_text = analysis_response.content;

        info!("Phase 1 complete - {} chars of thinking, {} chars analysis", 
            thinking.len(), analysis_text.len());

        // PHASE 2: Generate structured fix using code_fix tool
        info!("Phase 2 - Generating complete file fix");

        let fix_system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
            persona,
            context,
            error_context,
            file_content,
            metadata,
            Some(project_id),
            code_intel.as_ref().map(|ci| ci.elements.clone()),
            code_intel.as_ref().map(|ci| ci.issues.clone()),
        );

        let fix_request = code_fix_processor::build_code_fix_request(
            &error_context.error_message,
            &error_context.file_path,
            file_content,
            fix_system_prompt,
            vec![], // No context messages for code fix
        )?;

        // Extract and convert to provider format with Value::String
        let fix_messages: Vec<Message> = fix_request["messages"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing messages in fix request"))?
            .iter()
            .filter_map(|m| {
                Some(Message {
                    role: m["role"].as_str()?.to_string(),
                    content: m["content"].as_str(?.to_string()),
                })
            })
            .collect();

        let tools = fix_request["tools"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing tools in fix request"))?
            .clone();

        // Use provider with tools
        let fix_response = self.llm
            .chat_with_tools(
                fix_messages,
                fix_request["system"].as_str().unwrap_or("").to_string(),
                tools,
                None,  // No tool_choice - let Claude decide when to use code_fix tool
            )
            .await?;

        // Extract structured code fix
        let code_fix = code_fix_processor::extract_code_fix_response(&fix_response.raw_response)?;

        info!("Code fix generated: {} files", code_fix.files.len());

        // Convert to CompleteResponse using the built-in method
        let metadata = extract_claude_metadata(&fix_response, 0)?;
        Ok(code_fix.into_complete_response(metadata, fix_response.raw_response))
    }

    /// Get code intelligence for specific file
    async fn get_code_intelligence_for_file(
        &self,
        file_path: &str,
        project_id: &str,
    ) -> Result<Option<FileIntelligence>> {
        // Query code elements - JOIN through repository_files and git_repo_attachments
        let elements = sqlx::query!(
            r#"
            SELECT ce.element_type, ce.name, ce.start_line, ce.end_line, ce.visibility, 
                   ce.is_async, ce.documentation, ce.complexity_score
            FROM code_elements ce
            JOIN repository_files rf ON ce.file_id = rf.id
            JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
            WHERE rf.file_path = ? AND gra.project_id = ?
            ORDER BY ce.start_line
            "#,
            file_path,
            project_id
        )
        .fetch_all(&self.sqlite_pool)
        .await?;

        if elements.is_empty() {
            return Ok(None);
        }

        // Map database fields to prompt struct fields
        let code_elements: Vec<CodeElement> = elements
            .into_iter()
            .map(|row| CodeElement {
                element_type: row.element_type,
                name: row.name,
                start_line: row.start_line,
                end_line: row.end_line,
                complexity: row.complexity_score,
                is_async: row.is_async,
                is_public: Some(row.visibility == "public"), // Map visibility to is_public
                documentation: row.documentation,
            })
            .collect();

        // Query quality issues - JOIN through code_elements, repository_files, git_repo_attachments
        let issues = sqlx::query!(
            r#"
            SELECT cqi.severity, cqi.title, cqi.description, cqi.suggested_fix, ce.name as element_name
            FROM code_quality_issues cqi
            JOIN code_elements ce ON cqi.element_id = ce.id
            JOIN repository_files rf ON ce.file_id = rf.id
            JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
            WHERE rf.file_path = ? AND gra.project_id = ?
            "#,
            file_path,
            project_id
        )
        .fetch_all(&self.sqlite_pool)
        .await?;

        // Map database fields to prompt struct fields
        let quality_issues: Vec<QualityIssue> = issues
            .into_iter()
            .map(|row| QualityIssue {
                severity: row.severity,
                category: row.title,              // Map title to category
                description: row.description,
                element_name: Some(row.element_name),
                suggestion: row.suggested_fix,    // Map suggested_fix to suggestion
            })
            .collect();

        Ok(Some(FileIntelligence {
            elements: code_elements,
            issues: quality_issues,
        }))
    }
}

#[derive(Debug, Clone)]
struct FileIntelligence {
    elements: Vec<CodeElement>,
    issues: Vec<QualityIssue>,
}
