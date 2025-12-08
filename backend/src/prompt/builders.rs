// backend/src/prompt/builders.rs
// Main prompt building functions for user-facing interactions
//
// PERSONALITY FLOW:
// 1. Persona defined in src/persona/default.rs (SINGLE SOURCE)
// 2. UnifiedPromptBuilder methods inject persona for user-facing prompts
// 3. build_technical_code_prompt() is the ONLY method that skips persona
//    (used when pure technical output is needed without conversational style)
//
// For internal/technical prompts (JSON output, code generation, inner loops),
// see src/prompt/internal.rs instead.

use crate::api::ws::message::MessageMetadata;
use crate::cache::{ContextHashes, SessionCacheState};
use crate::config::SYSTEM_CONTEXT;
use crate::git::client::tree_builder::FileNode;
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::context::{
    add_code_fix_requirements, add_code_intelligence_context, add_file_context,
    add_memory_context, add_project_context, add_repository_structure,
    add_system_environment, add_current_time, add_system_context,
    add_tool_context, add_tool_usage_hints, add_agentic_persistence, add_parallel_tool_guidance,
    add_preamble_guidance, build_project_context_content, build_memory_context_content,
    build_code_intelligence_content, build_file_context_content, add_cached_context_section,
};
use crate::prompt::types::{CodeElement, ErrorContext, QualityIssue};
use crate::prompt::utils::is_code_related;
use crate::tools::types::Tool;

/// Main prompt builder for user-facing interactions
///
/// This builder always injects personality from src/persona/default.rs
/// (except for build_technical_code_prompt which skips persona for accuracy).
///
/// For internal operations requiring structured output, see src/prompt/internal.rs
pub struct UnifiedPromptBuilder;

impl UnifiedPromptBuilder {
    /// Build system prompt for Mira (conversational AI)
    ///
    /// USES PERSONA: Yes - personality injected from persona/default.rs
    /// USE CASE: Primary user-facing conversations
    ///
    /// CACHE OPTIMIZATION: OpenAI caches prompt prefixes >1024 tokens at 50% discount.
    /// This function orders content to maximize cache hits:
    /// - STATIC section (cacheable): persona, system env, tools, guidelines (~1500+ tokens)
    /// - DYNAMIC section (not cached): timestamp, project, memory, code context
    pub fn build_system_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        tools: Option<&[Tool]>,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        code_context: Option<&[MemoryEntry]>, // Code intelligence semantic search results
        file_tree: Option<&[FileNode]>,       // Repository structure
    ) -> String {
        let mut prompt = String::new();

        // ================================================================
        // STATIC SECTION (cacheable prefix - OpenAI caches >1024 tokens)
        // This section should exceed 1024 tokens for optimal cache hits.
        // Content here is stable within a session.
        // ================================================================

        // 1. Core personality - pure, unmodified (static)
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");

        // 2. System environment - OS, shell, tools (static within session)
        add_system_environment(&mut prompt, &SYSTEM_CONTEXT);

        // 3. Tool definitions (static within session)
        add_tool_context(&mut prompt, tools);

        // 4. Tool usage hints and guidelines (static)
        if is_code_related(metadata) {
            add_tool_usage_hints(&mut prompt);
            add_parallel_tool_guidance(&mut prompt);
            add_agentic_persistence(&mut prompt);
            add_preamble_guidance(&mut prompt);
        }

        // ================================================================
        // DYNAMIC SECTION (not cached - changes per request)
        // Content below this point varies and won't benefit from caching.
        // ================================================================

        prompt.push_str("--- CONTEXT ---\n\n");

        // 5. Current timestamp (dynamic - changes every minute)
        add_current_time(&mut prompt);

        // 6. Project context (dynamic - varies by project)
        add_project_context(&mut prompt, metadata, project_id);

        // 7. Memory context (dynamic - varies by conversation)
        add_memory_context(&mut prompt, context);

        // 8. Code intelligence from semantic search (dynamic)
        add_code_intelligence_context(&mut prompt, code_context);

        // 9. Repository structure (dynamic - varies by project)
        add_repository_structure(&mut prompt, file_tree);

        // 10. File context (dynamic - varies by file being viewed)
        add_file_context(&mut prompt, metadata);

        prompt
    }

    /// Build prompt for code fixes with personality intact
    ///
    /// USES PERSONA: Yes - personality injected from persona/default.rs
    /// USE CASE: User-facing code fixes where Mira's style is wanted
    pub fn build_code_fix_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        error_context: &ErrorContext,
        file_content: &str,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        code_elements: Option<Vec<CodeElement>>,
        quality_issues: Option<Vec<QualityIssue>>,
    ) -> String {
        let mut prompt = String::new();

        // Keep persona for Mira's direct code fixes
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");

        // System environment for platform-appropriate commands
        add_system_context(&mut prompt, &SYSTEM_CONTEXT);

        add_project_context(&mut prompt, metadata, project_id);
        add_memory_context(&mut prompt, context);
        add_code_fix_requirements(
            &mut prompt,
            error_context,
            file_content,
            code_elements,
            quality_issues,
        );

        prompt
    }

    /// Build prompt for pure technical code operations (no personality)
    ///
    /// USES PERSONA: NO - this is the ONLY method that skips personality
    /// USE CASE: When technical accuracy is critical and conversational style would interfere
    ///
    /// Note: This is intentionally kept minimal. For most internal operations,
    /// use the prompts in src/prompt/internal.rs instead.
    pub fn build_technical_code_prompt(
        error_context: &ErrorContext,
        file_content: &str,
        code_elements: Option<Vec<CodeElement>>,
        quality_issues: Option<Vec<QualityIssue>>,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str("You are a code fix specialist.\n");
        prompt.push_str(
            "Generate complete, working file fixes with no personality or commentary.\n\n",
        );

        add_code_fix_requirements(
            &mut prompt,
            error_context,
            file_content,
            code_elements,
            quality_issues,
        );

        prompt
    }

    /// Build a simple prompt with just persona and memory context
    ///
    /// USES PERSONA: Yes - personality injected from persona/default.rs
    /// USE CASE: Simple conversations without heavy code context
    pub fn build_simple_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        project_id: Option<&str>,
    ) -> String {
        Self::build_system_prompt(
            persona, context, None, None, project_id,
            None, // No code context for simple prompts
            None, // No file tree for simple prompts
        )
    }

    /// Build system prompt with cache awareness for incremental context updates
    ///
    /// USES PERSONA: Yes - personality injected from persona/default.rs
    /// USE CASE: Primary user-facing conversations with LLM-side cache optimization
    ///
    /// CACHE OPTIMIZATION: This method builds on top of OpenAI's prompt caching.
    /// When cache_state is provided and indicates a warm cache:
    /// - Dynamic sections with unchanged content emit compact markers
    /// - This reduces token usage while preserving semantic meaning
    /// - OpenAI caches the static prefix at 90% discount
    ///
    /// Returns: (prompt, new_context_hashes, sections_using_cache_count)
    pub fn build_system_prompt_cached(
        persona: &PersonaOverlay,
        context: &RecallContext,
        tools: Option<&[Tool]>,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        code_context: Option<&[MemoryEntry]>,
        file_tree: Option<&[FileNode]>,
        cache_state: Option<&SessionCacheState>,
    ) -> (String, ContextHashes, usize) {
        let mut prompt = String::new();
        let mut new_hashes = ContextHashes::default();
        let mut cached_sections = 0;

        // ================================================================
        // STATIC SECTION (cacheable prefix - OpenAI caches >1024 tokens)
        // This section should exceed 1024 tokens for optimal cache hits.
        // Content here is stable within a session - ALWAYS include fully.
        // ================================================================

        // 1. Core personality - pure, unmodified (static)
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");

        // 2. System environment - OS, shell, tools (static within session)
        add_system_environment(&mut prompt, &SYSTEM_CONTEXT);

        // 3. Tool definitions (static within session)
        add_tool_context(&mut prompt, tools);

        // 4. Tool usage hints and guidelines (static)
        if is_code_related(metadata) {
            add_tool_usage_hints(&mut prompt);
            add_parallel_tool_guidance(&mut prompt);
            add_agentic_persistence(&mut prompt);
            add_preamble_guidance(&mut prompt);
        }

        // ================================================================
        // DYNAMIC SECTION (cache-aware - use markers for unchanged content)
        // Content below this point varies and uses incremental updates
        // when cache is warm and content matches previous state.
        // ================================================================

        prompt.push_str("--- CONTEXT ---\n\n");

        // 5. Current timestamp (always dynamic - changes every minute)
        add_current_time(&mut prompt);

        // 6. Project context (cache-aware)
        let project_content = build_project_context_content(metadata, project_id);
        if add_cached_context_section(
            &mut prompt,
            "project",
            project_content.as_deref(),
            cache_state,
            &mut new_hashes,
        ) {
            cached_sections += 1;
        }

        // 7. Memory context (cache-aware)
        let memory_content = build_memory_context_content(context);
        if add_cached_context_section(
            &mut prompt,
            "memory",
            memory_content.as_deref(),
            cache_state,
            &mut new_hashes,
        ) {
            cached_sections += 1;
        }

        // 8. Code intelligence from semantic search (cache-aware)
        let code_intel_content = build_code_intelligence_content(code_context);
        if add_cached_context_section(
            &mut prompt,
            "code_intelligence",
            code_intel_content.as_deref(),
            cache_state,
            &mut new_hashes,
        ) {
            cached_sections += 1;
        }

        // 9. File context including repo structure (cache-aware)
        let file_content = build_file_context_content(metadata, file_tree);
        if add_cached_context_section(
            &mut prompt,
            "file",
            file_content.as_deref(),
            cache_state,
            &mut new_hashes,
        ) {
            cached_sections += 1;
        }

        (prompt, new_hashes, cached_sections)
    }

    /// Calculate the static prefix hash for cache invalidation detection
    ///
    /// The static prefix includes: persona, system env, tools, guidelines
    /// If this changes, the OpenAI cache is invalidated for the session.
    pub fn calculate_static_prefix_hash(
        persona: &PersonaOverlay,
        tools: Option<&[Tool]>,
        is_code_related: bool,
    ) -> String {
        let mut prefix = String::new();

        // Build same static content as build_system_prompt
        prefix.push_str(persona.prompt());
        prefix.push_str("\n\n");
        add_system_environment(&mut prefix, &SYSTEM_CONTEXT);
        add_tool_context(&mut prefix, tools);

        if is_code_related {
            add_tool_usage_hints(&mut prefix);
            add_parallel_tool_guidance(&mut prefix);
            add_agentic_persistence(&mut prefix);
            add_preamble_guidance(&mut prefix);
        }

        SessionCacheState::hash_content(&prefix)
    }

    /// Estimate token count for the static prefix
    pub fn estimate_static_prefix_tokens(
        persona: &PersonaOverlay,
        tools: Option<&[Tool]>,
        is_code_related: bool,
    ) -> i64 {
        let mut prefix = String::new();

        prefix.push_str(persona.prompt());
        prefix.push_str("\n\n");
        add_system_environment(&mut prefix, &SYSTEM_CONTEXT);
        add_tool_context(&mut prefix, tools);

        if is_code_related {
            add_tool_usage_hints(&mut prefix);
            add_parallel_tool_guidance(&mut prefix);
            add_agentic_persistence(&mut prefix);
            add_preamble_guidance(&mut prefix);
        }

        SessionCacheState::estimate_tokens(&prefix)
    }
}
