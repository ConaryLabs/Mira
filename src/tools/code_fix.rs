// src/tools/code_fix.rs
use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::info;

use crate::llm::client::OpenAIClient;
use crate::llm::structured::{CompleteResponse, code_fix_processor, claude_processor};
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::{UnifiedPromptBuilder, CodeElement, QualityIssue};
use crate::config::CONFIG;

pub struct CodeFixHandler {
    llm_client: Arc<OpenAIClient>,
    code_intelligence: Arc<CodeIntelligenceService>,
    sqlite_pool: SqlitePool,
}

impl CodeFixHandler {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        code_intelligence: Arc<CodeIntelligenceService>,
        sqlite_pool: SqlitePool,
    ) -> Self {
        Self {
            llm_client,
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

        let (thinking_budget, _) = claude_processor::analyze_message_complexity(&error_context.error_message);

        let analysis_request = json!({
            "model": CONFIG.anthropic_model,
            "max_tokens": 4000,
            "temperature": 1.0,
            "thinking": {
                "type": "enabled",
                "budget_tokens": thinking_budget
            },
            "system": UnifiedPromptBuilder::build_system_prompt(
                persona,
                context,
                None,
                metadata,
                Some(project_id),
            ),
            "messages": [
                json!({
                    "role": "user",
                    "content": analysis_prompt
                })
            ]
        });

        let analysis_response = self.llm_client.post_response_with_retry(analysis_request).await?;

        // Extract thinking content
        let thinking = self.extract_thinking(&analysis_response);
        let analysis_text = self.extract_text_content(&analysis_response);

        info!("Phase 1 complete - {} chars of thinking, {} chars analysis", 
            thinking.len(), analysis_text.len());

        // PHASE 2: Generate structured fix using code_fix tool
        info!("Phase 2 - Generating complete file fix");

        let system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
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
            system_prompt,
            vec![], // No context messages for code fix
        )?;

        let fix_response = self.llm_client.post_response_with_retry(fix_request).await?;

        // Extract structured code fix
        let code_fix = code_fix_processor::extract_code_fix_response(&fix_response)?;

        info!("Code fix generated: {} files", code_fix.files.len());

        // Build complete response with artifacts
        self.build_complete_response(code_fix, thinking, &fix_response).await
    }

    async fn get_code_intelligence_for_file(
        &self,
        file_path: &str,
        project_id: &str,
    ) -> Result<Option<FileIntelligence>> {
        // Fetch code elements - prefix ambiguous columns with table alias
        let elements = sqlx::query!(
            r#"
            SELECT ce.element_type, ce.name, ce.start_line, ce.end_line, ce.visibility, ce.is_async, ce.documentation, ce.complexity_score
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

        let code_elements: Vec<CodeElement> = elements
            .into_iter()
            .map(|row| CodeElement {
                element_type: row.element_type,
                name: row.name,
                start_line: row.start_line as i32, // Cast i64 to i32
                end_line: row.end_line as i32,     // Cast i64 to i32
                complexity: row.complexity_score.map(|v| v as i32), // Cast Option<i64> to Option<i32>
                is_async: row.is_async,  // Already Option<bool>
                is_public: Some(row.visibility == "public"),
                documentation: row.documentation,
            })
            .collect();

        // Fetch quality issues - using actual schema columns
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

        let quality_issues: Vec<QualityIssue> = issues
            .into_iter()
            .map(|row| QualityIssue {
                severity: row.severity,
                category: row.title, // Using title as category
                description: row.description,
                element_name: Some(row.element_name), // Wrap in Some()
                suggestion: row.suggested_fix,
            })
            .collect();

        Ok(Some(FileIntelligence {
            elements: code_elements,
            issues: quality_issues,
        }))
    }

    async fn build_complete_response(
        &self,
        code_fix: crate::llm::structured::code_fix_processor::CodeFixResponse,
        _thinking: String,
        raw_response: &Value,
    ) -> Result<CompleteResponse> {
        // Build artifacts as JSON values (not project::Artifact struct)
        let artifacts = Some(code_fix.files.iter().map(|file| {
            json!({
                "path": file.path.clone(),
                "content": file.content.clone(),
                "change_type": match file.change_type {
                    crate::llm::structured::code_fix_processor::ChangeType::Primary => "primary",
                    crate::llm::structured::code_fix_processor::ChangeType::Import => "import",
                    crate::llm::structured::code_fix_processor::ChangeType::Type => "type",
                    crate::llm::structured::code_fix_processor::ChangeType::Cascade => "cascade",
                },
            })
        }).collect());

        let metadata = claude_processor::extract_claude_metadata(raw_response, 0)?;
        let structured = claude_processor::extract_claude_content_from_tool(raw_response)?;

        Ok(CompleteResponse {
            structured,
            metadata,
            raw_response: raw_response.clone(),
            artifacts,
        })
    }

    fn extract_thinking(&self, response: &Value) -> String {
        let mut thinking = String::new();

        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "thinking" {
                    if let Some(text) = block["thinking"].as_str() {
                        if !thinking.is_empty() {
                            thinking.push_str("\n\n");
                        }
                        thinking.push_str(text);
                    }
                }
            }
        }

        thinking
    }

    fn extract_text_content(&self, response: &Value) -> String {
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str() {
                        return text.to_string();
                    }
                }
            }
        }

        String::new()
    }
}

#[derive(Debug, Clone)]
struct FileIntelligence {
    elements: Vec<CodeElement>,
    issues: Vec<QualityIssue>,
}
