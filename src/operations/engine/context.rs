// src/operations/engine/context.rs
// Context building for operations: memory, code intelligence, file trees

use crate::config::CONFIG;
use crate::memory::features::recall_engine::RecallContext;
use crate::memory::core::types::MemoryEntry;
use crate::memory::service::MemoryService;
use crate::git::client::{FileNode, FileNodeType};
use crate::relationship::service::RelationshipService;
use crate::persona::PersonaOverlay;
use crate::tools::types::Tool;
use crate::prompt::UnifiedPromptBuilder;
use crate::operations::delegation_tools::get_delegation_tools;

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub struct ContextBuilder {
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
}

impl ContextBuilder {
    pub fn new(
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
    ) -> Self {
        Self {
            memory_service,
            relationship_service,
        }
    }

    /// Load memory context for operation
    pub async fn load_memory_context(
        &self,
        session_id: &str,
        query: &str,
    ) -> Result<RecallContext> {
        debug!("Loading memory context for session: {}", session_id);
        
        let recent_count = CONFIG.context_recent_messages as usize;
        let semantic_count = CONFIG.context_semantic_matches as usize;
        
        match self.memory_service.parallel_recall_context(
            session_id,
            query,
            recent_count,
            semantic_count,
        ).await {
            Ok(mut context) => {
                info!(
                    "Loaded context: {} recent, {} semantic memories",
                    context.recent.len(),
                    context.semantic.len()
                );
                
                if CONFIG.use_rolling_summaries_in_context {
                    context.rolling_summary = self.memory_service
                        .get_rolling_summary(session_id)
                        .await
                        .ok()
                        .flatten();
                    
                    context.session_summary = self.memory_service
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
        let tools_json = get_delegation_tools();
        
        let tools: Vec<Tool> = tools_json.iter()
            .filter_map(|v| serde_json::from_value::<Tool>(v.clone()).ok())
            .collect();
        
        let _relationship_ctx = self.relationship_service
            .context_loader()
            .load_context(session_id)
            .await
            .ok();
        
        UnifiedPromptBuilder::build_system_prompt(
            &persona,
            context,
            Some(&tools),
            None,  // metadata
            None,  // project_id
            code_context.map(|v| &**v),
            file_tree.map(|v| &**v),
        )
    }

    /// Build enriched context string for DeepSeek with all available information
    pub fn build_enriched_context(
        args: &serde_json::Value,
        file_tree: Option<&Vec<FileNode>>,
        code_context: Option<&Vec<MemoryEntry>>,
        recall_context: &RecallContext,
    ) -> String {
        let mut enriched_context = String::new();

        // 1. GPT-5's context from tool call (if any)
        if let Some(gpt_context) = args.get("context").and_then(|v| v.as_str()) {
            if !gpt_context.is_empty() {
                enriched_context.push_str("=== TASK CONTEXT ===\n");
                enriched_context.push_str(gpt_context);
                enriched_context.push_str("\n\n");
            }
        }

        // 2. Repository structure (so DeepSeek knows what files exist)
        if let Some(tree) = file_tree {
            enriched_context.push_str("=== PROJECT STRUCTURE ===\n");
            enriched_context.push_str(&Self::format_file_tree(tree, 0, 3)); // max depth 3
            enriched_context.push_str("\n\n");
        }

        // 3. Code intelligence (relevant code patterns from semantic search)
        if let Some(code_ctx) = code_context {
            if !code_ctx.is_empty() {
                enriched_context.push_str("=== RELEVANT CODE CONTEXT ===\n");
                for entry in code_ctx.iter().take(5) { // Top 5 most relevant
                    // Extract file path from tags (format: "file:/path/to/file")
                    let file_path = entry.tags.as_ref()
                        .and_then(|tags| tags.iter().find(|t| t.starts_with("file:")))
                        .map(|tag| tag.strip_prefix("file:").unwrap_or("unknown"))
                        .or_else(|| entry.error_file.as_deref())
                        .unwrap_or("unknown");
                    
                    let preview: String = entry.content.chars().take(500).collect(); // Preview
                    enriched_context.push_str(&format!(
                        "File: {}\n{}\n\n",
                        file_path,
                        preview
                    ));
                }
            }
        }

        // 4. Memory context (user preferences, coding style)
        if !recall_context.recent.is_empty() || !recall_context.semantic.is_empty() || recall_context.rolling_summary.is_some() {
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
                    || content_lower.contains("format") {
                    enriched_context.push_str(&format!("- {}\n", &memory.content));
                }
            }
            enriched_context.push_str("\n");
        }

        info!("[ENGINE] Built enriched context: {} chars", enriched_context.len());
        enriched_context
    }

    /// Format file tree for context (limit depth to avoid overwhelming DeepSeek)
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
                        output.push_str(&Self::format_file_tree(&node.children, depth + 1, max_depth));
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
