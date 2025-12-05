// src/operations/engine/context.rs
// Context building for operations: memory, code intelligence, file trees, and Context Oracle

use crate::config::CONFIG;
use crate::context_oracle::{ContextOracle, ContextRequest, GatheredContext};
use crate::git::client::{FileNode, FileNodeType};
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::recall_engine::RecallContext;
use crate::memory::service::MemoryService;
use crate::operations::delegation_tools::get_llm_tools;
use crate::persona::PersonaOverlay;
use crate::project::ProjectTaskService;
use crate::prompt::UnifiedPromptBuilder;
use crate::relationship::service::RelationshipService;
use crate::tools::types::Tool;

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub struct ContextBuilder {
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
    context_oracle: Option<Arc<ContextOracle>>,
    project_task_service: Option<Arc<ProjectTaskService>>,
}

impl ContextBuilder {
    pub fn new(
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
    ) -> Self {
        Self {
            memory_service,
            relationship_service,
            context_oracle: None,
            project_task_service: None,
        }
    }

    /// Add Context Oracle for enhanced context gathering
    pub fn with_context_oracle(mut self, oracle: Arc<ContextOracle>) -> Self {
        self.context_oracle = Some(oracle);
        self
    }

    /// Add ProjectTaskService for task context injection
    pub fn with_project_task_service(mut self, service: Arc<ProjectTaskService>) -> Self {
        self.project_task_service = Some(service);
        self
    }

    /// Load active tasks for a project and format for system prompt injection
    pub async fn load_task_context(&self, project_id: Option<&str>) -> Option<String> {
        let service = self.project_task_service.as_ref()?;
        let project_id = project_id?;

        match service.format_for_prompt(project_id).await {
            Ok(Some(context)) if !context.is_empty() => {
                info!("[ENGINE] Loaded task context for project {}", project_id);
                Some(context)
            }
            Ok(_) => {
                debug!("[ENGINE] No active tasks for project {}", project_id);
                None
            }
            Err(e) => {
                warn!("[ENGINE] Failed to load task context: {}", e);
                None
            }
        }
    }

    /// Gather context from the Context Oracle
    pub async fn gather_oracle_context(
        &self,
        query: &str,
        session_id: &str,
        project_id: Option<&str>,
        current_file: Option<&str>,
        error_message: Option<&str>,
    ) -> Option<GatheredContext> {
        let oracle = self.context_oracle.as_ref()?;

        let mut request = ContextRequest::new(query.to_string(), session_id.to_string());

        if let Some(pid) = project_id {
            request = request.with_project(pid);
        }

        if let Some(file) = current_file {
            request = request.with_file(file);
        }

        if let Some(error) = error_message {
            request = request.with_error(error, None);
        }

        match oracle.gather(&request).await {
            Ok(context) => {
                if context.is_empty() {
                    debug!("Oracle gathered empty context");
                    None
                } else {
                    info!(
                        "Oracle gathered context: {} sources, ~{} tokens, {}ms",
                        context.sources_used.len(),
                        context.estimated_tokens,
                        context.duration_ms
                    );
                    Some(context)
                }
            }
            Err(e) => {
                warn!("Failed to gather oracle context: {}", e);
                None
            }
        }
    }

    /// Load memory context for operation
    pub async fn load_memory_context(
        &self,
        session_id: &str,
        query: &str,
        project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Loading memory context for session: {}", session_id);

        let recent_count = CONFIG.context_recent_messages as usize;
        let semantic_count = CONFIG.context_semantic_matches as usize;

        match self
            .memory_service
            .parallel_recall_context(session_id, query, recent_count, semantic_count, project_id)
            .await
        {
            Ok(mut context) => {
                info!(
                    "Loaded context: {} recent, {} semantic memories",
                    context.recent.len(),
                    context.semantic.len()
                );

                if CONFIG.use_rolling_summaries_in_context {
                    context.rolling_summary = self
                        .memory_service
                        .get_rolling_summary(session_id)
                        .await
                        .ok()
                        .flatten();

                    context.session_summary = self
                        .memory_service
                        .get_session_summary(session_id)
                        .await
                        .ok()
                        .flatten();

                    if context.rolling_summary.is_some() || context.session_summary.is_some() {
                        info!("Loaded summaries for context");
                    }
                }

                Ok(context)
            }
            Err(e) => {
                warn!("Failed to load memory context: {}, using empty context", e);
                Ok(RecallContext {
                    recent: vec![],
                    semantic: vec![],
                    rolling_summary: None,
                    session_summary: None,
                    code_intelligence: None,
                })
            }
        }
    }

    /// Build system prompt with all context
    pub async fn build_system_prompt(
        &self,
        session_id: &str,
        context: &RecallContext,
        code_context: Option<&Vec<MemoryEntry>>,
        file_tree: Option<&Vec<FileNode>>,
    ) -> String {
        let persona = PersonaOverlay::Default;
        let tools_json = get_llm_tools();

        let tools: Vec<Tool> = tools_json
            .iter()
            .filter_map(|v| serde_json::from_value::<Tool>(v.clone()).ok())
            .collect();

        let _relationship_ctx = self
            .relationship_service
            .context_loader()
            .load_context(session_id)
            .await
            .ok();

        UnifiedPromptBuilder::build_system_prompt(
            &persona,
            context,
            Some(&tools),
            None, // metadata
            None, // project_id
            code_context.map(|v| &**v),
            file_tree.map(|v| &**v),
        )
    }

    /// Build enriched context string for LLM with all available information
    pub fn build_enriched_context(
        args: &serde_json::Value,
        file_tree: Option<&Vec<FileNode>>,
        code_context: Option<&Vec<MemoryEntry>>,
        recall_context: &RecallContext,
    ) -> String {
        Self::build_enriched_context_with_oracle(args, file_tree, code_context, recall_context, None)
    }

    /// Build enriched context with optional Context Oracle output
    pub fn build_enriched_context_with_oracle(
        args: &serde_json::Value,
        file_tree: Option<&Vec<FileNode>>,
        code_context: Option<&Vec<MemoryEntry>>,
        recall_context: &RecallContext,
        oracle_context: Option<&GatheredContext>,
    ) -> String {
        let mut enriched_context = String::new();

        // 1. LLM's context from tool call (if any)
        if let Some(gpt_context) = args.get("context").and_then(|v| v.as_str()) {
            if !gpt_context.is_empty() {
                enriched_context.push_str("=== TASK CONTEXT ===\n");
                enriched_context.push_str(gpt_context);
                enriched_context.push_str("\n\n");
            }
        }

        // 2. Repository structure (so model knows what files exist)
        if let Some(tree) = file_tree {
            enriched_context.push_str("=== PROJECT STRUCTURE ===\n");
            enriched_context.push_str(&Self::format_file_tree(tree, 0, 3)); // max depth 3
            enriched_context.push_str("\n\n");
        }

        // 3. Code intelligence (relevant code patterns from semantic search)
        if let Some(code_ctx) = code_context {
            if !code_ctx.is_empty() {
                enriched_context.push_str("=== RELEVANT CODE CONTEXT ===\n");
                for entry in code_ctx.iter().take(5) {
                    // Top 5 most relevant
                    // Extract file path from tags (format: "file:/path/to/file")
                    let file_path = entry
                        .tags
                        .as_ref()
                        .and_then(|tags| tags.iter().find(|t| t.starts_with("file:")))
                        .map(|tag| tag.strip_prefix("file:").unwrap_or("unknown"))
                        .or_else(|| entry.error_file.as_deref())
                        .unwrap_or("unknown");

                    let preview: String = entry.content.chars().take(500).collect(); // Preview
                    enriched_context.push_str(&format!("File: {}\n{}\n\n", file_path, preview));
                }
            }
        }

        // 4. Context Oracle intelligence (co-change, patterns, fixes, etc.)
        if let Some(oracle) = oracle_context {
            let oracle_output = oracle.format_for_prompt();
            if !oracle_output.is_empty() {
                enriched_context.push_str("=== CODEBASE INTELLIGENCE ===\n");
                enriched_context.push_str(&oracle_output);
                enriched_context.push_str("\n");
            }
        }

        // 5. Memory context (user preferences, coding style)
        if !recall_context.recent.is_empty()
            || !recall_context.semantic.is_empty()
            || recall_context.rolling_summary.is_some()
        {
            enriched_context.push_str("=== USER PREFERENCES & CODING STYLE ===\n");

            // Include rolling summary if available (contains recent coding patterns)
            if let Some(summary) = &recall_context.rolling_summary {
                enriched_context.push_str("Recent coding patterns:\n");
                enriched_context.push_str(summary);
                enriched_context.push_str("\n\n");
            }

            // Include relevant semantic memories (coding style preferences)
            for memory in recall_context.semantic.iter().take(3) {
                // Only include if related to coding/style preferences
                let content_lower = memory.content.to_lowercase();
                if content_lower.contains("style")
                    || content_lower.contains("prefer")
                    || content_lower.contains("pattern")
                    || content_lower.contains("coding")
                    || content_lower.contains("format")
                {
                    enriched_context.push_str(&format!("- {}\n", &memory.content));
                }
            }
            enriched_context.push_str("\n");
        }

        info!(
            "[ENGINE] Built enriched context: {} chars{}",
            enriched_context.len(),
            if oracle_context.is_some() { " (with oracle)" } else { "" }
        );
        enriched_context
    }

    /// Format file tree for context (limit depth to keep context manageable)
    fn format_file_tree(nodes: &[FileNode], depth: usize, max_depth: usize) -> String {
        if depth >= max_depth {
            return String::new();
        }

        let mut output = String::new();
        let indent = "  ".repeat(depth);

        for node in nodes {
            match node.node_type {
                FileNodeType::Directory => {
                    output.push_str(&format!("{}{}/\n", indent, node.name));
                    if !node.children.is_empty() {
                        output.push_str(&Self::format_file_tree(
                            &node.children,
                            depth + 1,
                            max_depth,
                        ));
                    }
                }
                FileNodeType::File => {
                    output.push_str(&format!("{}{}\n", indent, node.name));
                }
            }
        }

        output
    }
}
