// backend/src/context_oracle/gatherer.rs
// Context Oracle: Unified context gathering from all intelligence systems

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::build::BuildTracker;
use crate::git::intelligence::{CochangeService, ExpertiseService, FixService};
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::patterns::{MatchContext, PatternMatcher, PatternStorage};

use super::types::*;

/// Context Oracle - unified context gathering from all intelligence systems
pub struct ContextOracle {
    pool: Arc<SqlitePool>,
    code_intelligence: Option<Arc<CodeIntelligenceService>>,
    cochange_service: Option<Arc<CochangeService>>,
    expertise_service: Option<Arc<ExpertiseService>>,
    fix_service: Option<Arc<FixService>>,
    build_tracker: Option<Arc<BuildTracker>>,
    pattern_storage: Option<Arc<PatternStorage>>,
    pattern_matcher: Option<Arc<PatternMatcher>>,
}

impl ContextOracle {
    /// Create a new context oracle with database pool
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self {
            pool,
            code_intelligence: None,
            cochange_service: None,
            expertise_service: None,
            fix_service: None,
            build_tracker: None,
            pattern_storage: None,
            pattern_matcher: None,
        }
    }

    /// Add code intelligence service
    pub fn with_code_intelligence(mut self, service: Arc<CodeIntelligenceService>) -> Self {
        self.code_intelligence = Some(service);
        self
    }

    /// Add co-change service
    pub fn with_cochange(mut self, service: Arc<CochangeService>) -> Self {
        self.cochange_service = Some(service);
        self
    }

    /// Add expertise service
    pub fn with_expertise(mut self, service: Arc<ExpertiseService>) -> Self {
        self.expertise_service = Some(service);
        self
    }

    /// Add fix service
    pub fn with_fix_service(mut self, service: Arc<FixService>) -> Self {
        self.fix_service = Some(service);
        self
    }

    /// Add build tracker
    pub fn with_build_tracker(mut self, tracker: Arc<BuildTracker>) -> Self {
        self.build_tracker = Some(tracker);
        self
    }

    /// Add pattern storage
    pub fn with_pattern_storage(mut self, storage: Arc<PatternStorage>) -> Self {
        self.pattern_storage = Some(storage);
        self
    }

    /// Add pattern matcher
    pub fn with_pattern_matcher(mut self, matcher: Arc<PatternMatcher>) -> Self {
        self.pattern_matcher = Some(matcher);
        self
    }

    /// Gather context from all enabled intelligence systems
    pub async fn gather(&self, request: &ContextRequest) -> Result<GatheredContext> {
        let start = Instant::now();
        info!(
            "Gathering context for query: {}",
            &request.query[..50.min(request.query.len())]
        );

        let mut context = GatheredContext::empty();
        let config = &request.config;

        // Gather code context
        if config.include_code_search {
            if let Some(code_ctx) = self.gather_code_context(request).await {
                context.sources_used.push("code_intelligence".to_string());
                context.code_context = Some(code_ctx);
            }
        }

        // Gather call graph context
        if config.include_call_graph {
            if let Some(cg_ctx) = self.gather_call_graph_context(request).await {
                context.sources_used.push("call_graph".to_string());
                context.call_graph = Some(cg_ctx);
            }
        }

        // Gather co-change suggestions
        if config.include_cochange {
            let suggestions = self.gather_cochange_suggestions(request).await;
            if !suggestions.is_empty() {
                context.sources_used.push("cochange".to_string());
                context.cochange_suggestions = suggestions;
            }
        }

        // Gather historical fixes
        if config.include_historical_fixes {
            let fixes = self.gather_historical_fixes(request).await;
            if !fixes.is_empty() {
                context.sources_used.push("historical_fixes".to_string());
                context.historical_fixes = fixes;
            }
        }

        // Gather design patterns
        if config.include_patterns {
            let patterns = self.gather_design_patterns(request).await;
            if !patterns.is_empty() {
                context.sources_used.push("design_patterns".to_string());
                context.design_patterns = patterns;
            }
        }

        // Gather reasoning pattern suggestions
        if config.include_reasoning_patterns {
            let patterns = self.gather_reasoning_patterns(request).await;
            if !patterns.is_empty() {
                context.sources_used.push("reasoning_patterns".to_string());
                context.reasoning_patterns = patterns;
            }
        }

        // Gather build errors
        if config.include_build_errors {
            let errors = self.gather_build_errors(request).await;
            if !errors.is_empty() {
                context.sources_used.push("build_errors".to_string());
                context.build_errors = errors;
            }
        }

        // Gather expertise information
        if config.include_expertise {
            let expertise = self.gather_expertise(request).await;
            if !expertise.is_empty() {
                context.sources_used.push("expertise".to_string());
                context.expertise = expertise;
            }
        }

        // Estimate tokens
        context.estimated_tokens = self.estimate_tokens(&context);
        context.duration_ms = start.elapsed().as_millis() as i64;

        info!(
            "Gathered context in {}ms: {} sources, ~{} tokens",
            context.duration_ms,
            context.sources_used.len(),
            context.estimated_tokens
        );

        Ok(context)
    }

    /// Gather code context from semantic search
    async fn gather_code_context(&self, request: &ContextRequest) -> Option<CodeContext> {
        let code_intel = self.code_intelligence.as_ref()?;
        let project_id = request.project_id.as_ref()?;

        match code_intel
            .search_code(&request.query, project_id, request.config.max_code_results)
            .await
        {
            Ok(entries) => {
                if entries.is_empty() {
                    return None;
                }

                let elements: Vec<CodeElement> = entries
                    .into_iter()
                    .map(|e| {
                        // Extract metadata from tags
                        let tags = e.tags.as_ref();
                        let element_type = tags
                            .and_then(|t| {
                                t.iter()
                                    .find(|tag| tag.starts_with("element_type:"))
                                    .and_then(|tag| tag.strip_prefix("element_type:"))
                            })
                            .unwrap_or("unknown")
                            .to_string();

                        let name = tags
                            .and_then(|t| {
                                t.iter()
                                    .find(|tag| tag.starts_with("name:"))
                                    .and_then(|tag| tag.strip_prefix("name:"))
                            })
                            .unwrap_or("")
                            .to_string();

                        let file_path = tags
                            .and_then(|t| {
                                t.iter()
                                    .find(|tag| tag.starts_with("path:"))
                                    .and_then(|tag| tag.strip_prefix("path:"))
                            })
                            .unwrap_or("")
                            .to_string();

                        CodeElement {
                            name,
                            element_type,
                            file_path,
                            content: e.content,
                            line_number: None,
                        }
                    })
                    .collect();

                Some(CodeContext {
                    elements,
                    relevance: 0.8, // Default relevance
                })
            }
            Err(e) => {
                warn!("Failed to gather code context: {}", e);
                None
            }
        }
    }

    /// Gather call graph context for current file/function
    async fn gather_call_graph_context(&self, request: &ContextRequest) -> Option<CallGraphContext> {
        let code_intel = self.code_intelligence.as_ref()?;
        let current_file = request.current_file.as_ref()?;

        // Get file_id from current file
        let file_id = match self.get_file_id(current_file).await {
            Ok(Some(id)) => id,
            _ => return None,
        };

        // Get call graph service
        let call_graph = code_intel.call_graph();

        // Get elements for this file and aggregate call graph info
        // Query code_elements for this file to get element IDs
        let element_ids: Vec<i64> = match sqlx::query_scalar!(
            r#"SELECT id as "id!" FROM code_elements WHERE file_id = ?"#,
            file_id
        )
        .fetch_all(self.pool.as_ref())
        .await
        {
            Ok(ids) => ids,
            Err(e) => {
                debug!("Failed to get code elements: {}", e);
                return None;
            }
        };

        if element_ids.is_empty() {
            return None;
        }

        let mut all_callers = Vec::new();
        let mut all_callees = Vec::new();

        // Get call info for each element
        for elem_id in element_ids.iter().take(10) {
            // Limit to avoid too many queries
            if let Ok(callers) = call_graph.get_callers(*elem_id).await {
                for caller in callers {
                    all_callers.push(caller.name);
                }
            }
            if let Ok(callees) = call_graph.get_callees(*elem_id).await {
                for callee in callees {
                    all_callees.push(callee.name);
                }
            }
        }

        // Deduplicate
        all_callers.sort();
        all_callers.dedup();
        all_callees.sort();
        all_callees.dedup();

        if all_callers.is_empty() && all_callees.is_empty() {
            return None;
        }

        Some(CallGraphContext {
            callers: all_callers,
            callees: all_callees,
            impact_summary: None,
        })
    }

    /// Gather co-change suggestions
    async fn gather_cochange_suggestions(&self, request: &ContextRequest) -> Vec<CochangeSuggestion> {
        let cochange = match &self.cochange_service {
            Some(s) => s,
            None => return Vec::new(),
        };

        let current_file = match &request.current_file {
            Some(f) => f,
            None => return Vec::new(),
        };

        let project_id = match &request.project_id {
            Some(p) => p,
            None => return Vec::new(),
        };

        match cochange.get_suggestions(project_id, current_file).await {
            Ok(suggestions) => suggestions
                .into_iter()
                .take(request.config.max_cochange_suggestions)
                .map(|s| CochangeSuggestion {
                    file_path: s.file_path,
                    confidence: s.confidence,
                    reason: s.reason,
                    change_count: s.cochange_count as i32,
                })
                .collect(),
            Err(e) => {
                debug!("Failed to get co-change suggestions: {}", e);
                Vec::new()
            }
        }
    }

    /// Gather historical fixes for similar errors
    async fn gather_historical_fixes(&self, request: &ContextRequest) -> Vec<HistoricalFixInfo> {
        let fix_service = match &self.fix_service {
            Some(s) => s,
            None => return Vec::new(),
        };

        let error_message = match &request.error_message {
            Some(e) => e,
            None => return Vec::new(),
        };

        let project_id = match &request.project_id {
            Some(p) => p,
            None => return Vec::new(),
        };

        // Get affected files if we have a current file
        let affected_files: Option<Vec<String>> = request
            .current_file
            .as_ref()
            .map(|f| vec![f.clone()]);

        match fix_service
            .find_similar_fixes(project_id, error_message, affected_files.as_deref())
            .await
        {
            Ok(matches) => matches
                .into_iter()
                .take(request.config.max_historical_fixes)
                .map(|m| HistoricalFixInfo {
                    commit_hash: m.fix.fix_commit_hash,
                    commit_message: m.match_reason.clone(),
                    fix_description: m.fix.fix_description.unwrap_or_else(|| m.match_reason),
                    similarity: m.similarity_score,
                    files_changed: m.fix.files_modified,
                })
                .collect(),
            Err(e) => {
                debug!("Failed to get historical fixes: {}", e);
                Vec::new()
            }
        }
    }

    /// Gather design pattern context
    async fn gather_design_patterns(&self, request: &ContextRequest) -> Vec<PatternContext> {
        let _code_intel = match &self.code_intelligence {
            Some(s) => s,
            None => return Vec::new(),
        };

        let project_id = match &request.project_id {
            Some(p) => p,
            None => return Vec::new(),
        };

        // Query design patterns for this project with high confidence
        match sqlx::query!(
            r#"
            SELECT pattern_type, pattern_name, description, confidence, involved_symbols
            FROM design_patterns
            WHERE project_id = ? AND confidence >= 0.7
            ORDER BY confidence DESC
            LIMIT 5
            "#,
            project_id
        )
        .fetch_all(self.pool.as_ref())
        .await
        {
            Ok(rows) => rows
                .into_iter()
                .map(|r| PatternContext {
                    pattern_type: r.pattern_type,
                    pattern_name: r.pattern_name,
                    description: r.description.unwrap_or_default(),
                    relevant_files: Vec::new(), // Would need to resolve symbols to files
                    confidence: r.confidence,
                })
                .collect(),
            Err(e) => {
                debug!("Failed to get design patterns: {}", e);
                Vec::new()
            }
        }
    }

    /// Gather reasoning pattern suggestions
    async fn gather_reasoning_patterns(
        &self,
        request: &ContextRequest,
    ) -> Vec<ReasoningPatternSuggestion> {
        let matcher = match &self.pattern_matcher {
            Some(m) => m,
            None => return Vec::new(),
        };

        // Build match context
        let mut match_ctx = MatchContext::new().with_message(&request.query);

        if let Some(ref file) = request.current_file {
            match_ctx = match_ctx.with_file(file, None);
        }
        if let Some(ref error) = request.error_message {
            match_ctx = match_ctx.with_error(error, request.error_code.as_deref());
        }

        // Extract keywords
        let keywords = crate::patterns::PatternMatcher::extract_keywords(&request.query);
        match_ctx = match_ctx.with_keywords(keywords);

        match matcher.find_matches(&match_ctx).await {
            Ok(matches) => matches
                .into_iter()
                .take(3)
                .map(|m| ReasoningPatternSuggestion {
                    pattern_id: m.pattern.id,
                    pattern_name: m.pattern.name,
                    description: m.pattern.description,
                    match_score: m.match_score,
                    match_reasons: m.match_reasons,
                    suggested_steps: m
                        .pattern
                        .steps
                        .into_iter()
                        .map(|s| s.description)
                        .collect(),
                })
                .collect(),
            Err(e) => {
                debug!("Failed to find reasoning patterns: {}", e);
                Vec::new()
            }
        }
    }

    /// Gather recent build errors
    async fn gather_build_errors(&self, request: &ContextRequest) -> Vec<BuildErrorContext> {
        let tracker = match &self.build_tracker {
            Some(t) => t,
            None => return Vec::new(),
        };

        let project_id = match &request.project_id {
            Some(p) => p,
            None => return Vec::new(),
        };

        match tracker.get_errors_for_context(project_id, 10).await {
            Ok(errors) => errors
                .into_iter()
                .take(5)
                .map(|e| BuildErrorContext {
                    error_hash: e.error_hash,
                    error_message: e.message,
                    file_path: e.file_path,
                    line_number: e.line_number,
                    category: e.category.as_str().to_string(),
                    occurrence_count: e.occurrence_count,
                    last_seen: e.last_seen_at,
                    suggested_fix: None,
                })
                .collect(),
            Err(e) => {
                debug!("Failed to get build errors: {}", e);
                Vec::new()
            }
        }
    }

    /// Gather expertise information
    async fn gather_expertise(&self, request: &ContextRequest) -> Vec<ExpertiseContext> {
        let expertise_service = match &self.expertise_service {
            Some(s) => s,
            None => return Vec::new(),
        };

        let current_file = match &request.current_file {
            Some(f) => f,
            None => return Vec::new(),
        };

        let project_id = match &request.project_id {
            Some(p) => p,
            None => return Vec::new(),
        };

        match expertise_service
            .find_experts_for_file(project_id, current_file, 3)
            .await
        {
            Ok(experts) => experts
                .into_iter()
                .map(|e| ExpertiseContext {
                    author: e.author_email,
                    expertise_areas: e.matching_patterns,
                    overall_score: e.expertise_score,
                    relevant_files: vec![current_file.clone()],
                })
                .collect(),
            Err(e) => {
                debug!("Failed to get expertise: {}", e);
                Vec::new()
            }
        }
    }

    /// Get file_id from file path
    async fn get_file_id(&self, file_path: &str) -> Result<Option<i64>> {
        let row = sqlx::query_scalar!(
            r#"SELECT id as "id!" FROM repository_files WHERE file_path = ? LIMIT 1"#,
            file_path
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row)
    }

    /// Estimate token count for context
    fn estimate_tokens(&self, context: &GatheredContext) -> usize {
        let formatted = context.format_for_prompt();
        // Rough estimate: ~4 characters per token
        formatted.len() / 4
    }

    /// Get statistics about context gathering
    pub async fn get_stats(&self) -> GatheringStats {
        // Return basic stats - in production, these would be tracked
        GatheringStats {
            total_queries: 0,
            avg_duration_ms: 0.0,
            avg_tokens: 0.0,
            cache_hit_rate: 0.0,
            sources_used: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_context_oracle_creation() {
        let pool = create_test_pool().await;
        let oracle = ContextOracle::new(Arc::new(pool));

        // Should create successfully
        assert!(oracle.code_intelligence.is_none());
        assert!(oracle.cochange_service.is_none());
    }

    #[tokio::test]
    async fn test_empty_gather() {
        let pool = create_test_pool().await;
        let oracle = ContextOracle::new(Arc::new(pool));

        let request = ContextRequest::new("test query".to_string(), "session-1".to_string());

        let context = oracle.gather(&request).await.unwrap();

        // Should return empty context when no services configured
        assert!(context.is_empty());
        assert!(context.sources_used.is_empty());
    }

    #[test]
    fn test_context_config_presets() {
        let minimal = ContextConfig::minimal();
        assert!(minimal.include_code_search);
        assert!(!minimal.include_call_graph);
        assert_eq!(minimal.max_context_tokens, 4000);

        let full = ContextConfig::full();
        assert!(full.include_code_search);
        assert!(full.include_call_graph);
        assert!(full.include_expertise);
        assert_eq!(full.max_context_tokens, 16000);

        let error = ContextConfig::for_error();
        assert!(error.include_historical_fixes);
        assert!(error.include_build_errors);
    }

    #[test]
    fn test_gathered_context_format() {
        let mut context = GatheredContext::empty();

        context.code_context = Some(CodeContext {
            elements: vec![CodeElement {
                name: "test_func".to_string(),
                element_type: "function".to_string(),
                file_path: "src/lib.rs".to_string(),
                content: "fn test_func() {}".to_string(),
                line_number: Some(10),
            }],
            relevance: 0.9,
        });

        context.cochange_suggestions = vec![CochangeSuggestion {
            file_path: "src/test.rs".to_string(),
            confidence: 0.85,
            reason: "Often changed together".to_string(),
            change_count: 5,
        }];

        let formatted = context.format_for_prompt();
        assert!(formatted.contains("Relevant Code"));
        assert!(formatted.contains("test_func"));
        assert!(formatted.contains("Related Files"));
        assert!(formatted.contains("src/test.rs"));
    }
}
