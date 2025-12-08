// backend/tests/cache_ab_comparison_test.rs
// Comprehensive A/B comparison test for incremental context caching
//
// This test provides hard numbers on:
// - Prompt size reduction (bytes and estimated tokens)
// - Per-section savings
// - Different caching scenarios

use chrono::Utc;
use mira_backend::cache::SessionCacheState;
use mira_backend::memory::core::types::MemoryEntry;
use mira_backend::memory::features::recall_engine::RecallContext;
use mira_backend::persona::PersonaOverlay;
use mira_backend::prompt::UnifiedPromptBuilder;

/// Create a MemoryEntry with all fields populated
fn create_memory_entry(
    id: i64,
    session_id: &str,
    role: &str,
    content: &str,
    salience: f32,
    summary: Option<&str>,
    tags: Option<Vec<String>>,
    error_file: Option<&str>,
) -> MemoryEntry {
    MemoryEntry {
        id: Some(id),
        session_id: session_id.to_string(),
        response_id: None,
        parent_id: None,
        role: role.to_string(),
        content: content.to_string(),
        timestamp: Utc::now(),
        tags,
        mood: None,
        intensity: None,
        salience: Some(salience),
        original_salience: Some(salience),
        intent: None,
        topics: None,
        summary: summary.map(|s| s.to_string()),
        relationship_impact: None,
        contains_code: Some(role == "system"),
        language: Some("en".to_string()),
        programming_lang: Some("rust".to_string()),
        analyzed_at: Some(Utc::now()),
        analysis_version: Some("1.0".to_string()),
        routed_to_heads: None,
        last_recalled: None,
        recall_count: Some(0),
        contains_error: None,
        error_type: None,
        error_severity: None,
        error_file: error_file.map(|s| s.to_string()),
        model_version: None,
        prompt_tokens: None,
        completion_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
        latency_ms: None,
        generation_time_ms: None,
        finish_reason: None,
        tool_calls: None,
        temperature: None,
        max_tokens: None,
        embedding: None,
        embedding_heads: None,
        qdrant_point_ids: None,
    }
}

/// Realistic memory context with substantial content
fn create_realistic_recall_context() -> RecallContext {
    RecallContext {
        recent: vec![],
        semantic: vec![
            create_memory_entry(
                1,
                "test",
                "assistant",
                "The user prefers functional programming patterns and immutable data structures. They use Rust edition 2024 and prefer async/await over threads. Code style emphasizes explicit error handling with Result types.",
                0.8,
                Some("User coding preferences"),
                None,
                None,
            ),
            create_memory_entry(
                2,
                "test",
                "assistant",
                "Previous discussion about implementing a caching layer for LLM responses. User wants to optimize for both cost and latency. Decided on two-level approach: application cache + OpenAI prompt caching.",
                0.7,
                Some("Caching discussion"),
                None,
                None,
            ),
        ],
        rolling_summary: Some(r#"
## Recent Session Summary

The user has been working on implementing an LLM caching optimization system for their Mira project. Key points:

1. **Architecture Decision**: Chose a two-level caching approach:
   - Application-level cache (SQLite) for exact request matches
   - OpenAI prompt caching for prefix-based caching (90% discount)

2. **Implementation Progress**:
   - Created SessionCacheState struct to track what was sent
   - Implemented incremental context algorithm
   - Added cache-aware prompt builder

3. **Technical Details**:
   - Using SHA-256 hashes to detect content changes
   - 5-minute TTL window for cache warmth
   - Static prefix (~1200 tokens) optimized for OpenAI caching

4. **Testing Requirements**:
   - Need A/B comparison with hard numbers
   - Want to verify actual token/cost savings
"#.to_string()),
        session_summary: Some(r#"
Long-running session focused on LLM optimization. User is building Mira, an AI coding assistant with Rust backend. Current milestone is implementing comprehensive caching to reduce API costs and improve latency. User values thorough testing and measurable results.
"#.to_string()),
        code_intelligence: None,
    }
}

/// Realistic code context entries
fn create_code_context() -> Vec<MemoryEntry> {
    vec![
        create_memory_entry(
            100,
            "test",
            "system",
            r#"
File: src/cache/session_state.rs
pub struct SessionCacheState {
    pub session_id: String,
    pub static_prefix_hash: String,
    pub last_call_at: DateTime<Utc>,
    pub context_hashes: ContextHashes,
    pub static_prefix_tokens: i64,
    pub last_reported_cached_tokens: i64,
    pub total_requests: i64,
    pub total_cached_tokens: i64,
}

impl SessionCacheState {
    pub fn new(session_id: String, static_prefix_hash: String, static_prefix_tokens: i64) -> Self {
        Self {
            session_id,
            static_prefix_hash,
            last_call_at: Utc::now(),
            context_hashes: ContextHashes::default(),
            static_prefix_tokens,
            last_reported_cached_tokens: 0,
            total_requests: 0,
            total_cached_tokens: 0,
        }
    }

    pub fn is_cache_likely_warm(&self) -> bool {
        let elapsed = Utc::now().signed_duration_since(self.last_call_at);
        elapsed.num_seconds() < DEFAULT_CACHE_WARM_WINDOW_SECS
    }

    pub fn update_after_call(&mut self, context_hashes: ContextHashes, cached_tokens: i64) {
        self.last_call_at = Utc::now();
        self.context_hashes = context_hashes;
        self.last_reported_cached_tokens = cached_tokens;
        self.total_requests += 1;
        self.total_cached_tokens += cached_tokens;
    }
}
"#,
            0.9,
            None,
            Some(vec!["file:src/cache/session_state.rs".to_string()]),
            Some("src/cache/session_state.rs"),
        ),
        create_memory_entry(
            101,
            "test",
            "system",
            r#"
File: src/prompt/builders.rs
impl UnifiedPromptBuilder {
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

        // STATIC SECTION (cacheable prefix)
        prompt.push_str(persona.prompt());
        add_system_environment(&mut prompt, &SYSTEM_CONTEXT);
        add_tool_context(&mut prompt, tools);

        // DYNAMIC SECTION (cache-aware)
        prompt.push_str("--- CONTEXT ---\n\n");
        add_current_time(&mut prompt);

        // Each section checks cache state for incremental updates
        if add_cached_context_section(&mut prompt, "project", ..., cache_state, &mut new_hashes) {
            cached_sections += 1;
        }
        // ... similar for memory, code_intelligence, file sections

        (prompt, new_hashes, cached_sections)
    }
}
"#,
            0.85,
            None,
            Some(vec!["file:src/prompt/builders.rs".to_string()]),
            Some("src/prompt/builders.rs"),
        ),
        create_memory_entry(
            102,
            "test",
            "system",
            r#"
File: src/operations/engine/orchestration.rs
async fn run_operation_inner(&self, ...) -> Result<()> {
    // Load memory context
    let recall_context = self.context_builder
        .load_memory_context(session_id, user_content, project_id)
        .await?;

    // Build cache-aware prompt
    let cache_result = self.context_builder
        .build_system_prompt_cached(
            session_id,
            &recall_context,
            code_context.as_ref(),
            file_tree.as_ref(),
            project_id,
        )
        .await;

    let system_prompt = cache_result.prompt;

    // Execute LLM call
    let result = self.execute_with_llm(..., system_prompt, ...).await;

    // Update cache state after call
    self.context_builder.update_cache_state(
        session_id,
        cache_result.static_prefix_hash,
        cache_result.static_prefix_tokens,
        cache_result.context_hashes,
        0, // cached_tokens tracked in orchestrator
    ).await;

    result
}
"#,
            0.8,
            None,
            Some(vec!["file:src/operations/engine/orchestration.rs".to_string()]),
            Some("src/operations/engine/orchestration.rs"),
        ),
    ]
}

/// Results from a single prompt build
#[derive(Debug)]
struct PromptMetrics {
    total_bytes: usize,
    estimated_tokens: i64,
    cached_sections: usize,
    has_project_marker: bool,
    has_memory_marker: bool,
    has_code_intel_marker: bool,
    has_file_marker: bool,
}

fn analyze_prompt(prompt: &str, cached_sections: usize) -> PromptMetrics {
    PromptMetrics {
        total_bytes: prompt.len(),
        estimated_tokens: SessionCacheState::estimate_tokens(prompt),
        cached_sections,
        has_project_marker: prompt.contains("[PROJECT: unchanged"),
        has_memory_marker: prompt.contains("[MEMORY: unchanged"),
        has_code_intel_marker: prompt.contains("[CODE_INTELLIGENCE: unchanged"),
        has_file_marker: prompt.contains("[FILE: unchanged"),
    }
}

fn print_comparison(label: &str, before: &PromptMetrics, after: &PromptMetrics) {
    let byte_saved = before.total_bytes as i64 - after.total_bytes as i64;
    let byte_pct = (byte_saved as f64 / before.total_bytes as f64) * 100.0;
    let token_saved = before.estimated_tokens - after.estimated_tokens;
    let token_pct = (token_saved as f64 / before.estimated_tokens as f64) * 100.0;

    println!("\n{}", "=".repeat(70));
    println!("{}", label);
    println!("{}", "=".repeat(70));
    println!("                          BEFORE          AFTER         SAVINGS");
    println!("{}", "-".repeat(70));
    println!("Bytes:              {:>10}      {:>10}    {:>10} ({:.1}%)",
        before.total_bytes, after.total_bytes, byte_saved, byte_pct);
    println!("Est. Tokens:        {:>10}      {:>10}    {:>10} ({:.1}%)",
        before.estimated_tokens, after.estimated_tokens, token_saved, token_pct);
    println!("Cached Sections:    {:>10}      {:>10}",
        before.cached_sections, after.cached_sections);
    println!("{}", "-".repeat(70));
    println!("Section Markers in AFTER:");
    println!("  [PROJECT: unchanged]           = {}", after.has_project_marker);
    println!("  [MEMORY: unchanged]            = {}", after.has_memory_marker);
    println!("  [CODE_INTELLIGENCE: unchanged] = {}", after.has_code_intel_marker);
    println!("  [FILE: unchanged]              = {}", after.has_file_marker);
}

/// SCENARIO 1: Full context comparison (all sections populated)
#[test]
fn scenario_1_full_context_identical_requests() {
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  SCENARIO 1: Full Context - Identical Back-to-Back Requests          ║");
    println!("║  Simulates: User asks follow-up question within 5 minutes            ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];
    let recall_context = create_realistic_recall_context();
    let code_context = create_code_context();

    // FIRST REQUEST (no cache)
    let (prompt1, hashes1, cached1) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        None, // No cache state
    );
    let metrics1 = analyze_prompt(&prompt1, cached1);

    // Create cache state from first request
    let static_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(&persona, Some(&tools), true);
    let static_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(&persona, Some(&tools), true);
    let mut state = SessionCacheState::new("test".to_string(), static_hash, static_tokens);
    state.update_after_call(hashes1, 0);

    // SECOND REQUEST (with warm cache, identical context)
    let (prompt2, _, cached2) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        Some(&state),
    );
    let metrics2 = analyze_prompt(&prompt2, cached2);

    print_comparison("Scenario 1: Identical requests with full context", &metrics1, &metrics2);

    // Assertions
    assert!(metrics2.total_bytes < metrics1.total_bytes, "Second prompt should be smaller");
    assert!(metrics2.cached_sections > 0, "Should have cached sections");
}

/// SCENARIO 2: Only memory context changes
#[test]
fn scenario_2_memory_changes_other_sections_cached() {
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  SCENARIO 2: Memory Changes, Other Sections Cached                   ║");
    println!("║  Simulates: User continues conversation (new memories added)         ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];
    let recall_context1 = create_realistic_recall_context();
    let code_context = create_code_context();

    // FIRST REQUEST
    let (prompt1, hashes1, cached1) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context1,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        None,
    );
    let metrics1 = analyze_prompt(&prompt1, cached1);

    // Create cache state
    let static_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(&persona, Some(&tools), true);
    let static_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(&persona, Some(&tools), true);
    let mut state = SessionCacheState::new("test".to_string(), static_hash, static_tokens);
    state.update_after_call(hashes1, 0);

    // SECOND REQUEST with changed memory (new rolling summary)
    let mut recall_context2 = create_realistic_recall_context();
    recall_context2.rolling_summary = Some(r#"
## Updated Session Summary

NEW CONTENT: User completed the caching implementation and is now testing it.
The A/B tests show significant savings. Next steps involve production deployment.
This is completely different content that will cause the memory hash to change.
"#.to_string());

    let (prompt2, _, cached2) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context2,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        Some(&state),
    );
    let metrics2 = analyze_prompt(&prompt2, cached2);

    print_comparison("Scenario 2: Memory changed, other sections cached", &metrics1, &metrics2);

    // Memory should NOT be cached (content changed)
    // But project and code_intelligence should be cached
    assert!(!metrics2.has_memory_marker, "Memory should NOT use cached marker (content changed)");
    assert!(metrics2.cached_sections > 0, "Should have some cached sections (project, code_intel)");
}

/// SCENARIO 3: Cold cache (simulating >5 minute gap)
#[test]
fn scenario_3_cold_cache_no_savings() {
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  SCENARIO 3: Cold Cache (>5 minute gap)                              ║");
    println!("║  Simulates: User returns after lunch break                           ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];
    let recall_context = create_realistic_recall_context();
    let code_context = create_code_context();

    // FIRST REQUEST
    let (prompt1, hashes1, cached1) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        None,
    );
    let metrics1 = analyze_prompt(&prompt1, cached1);

    // Create COLD cache state (10 minutes ago)
    let static_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(&persona, Some(&tools), true);
    let static_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(&persona, Some(&tools), true);
    let mut state = SessionCacheState::new("test".to_string(), static_hash, static_tokens);
    state.update_after_call(hashes1, 0);
    state.last_call_at = chrono::Utc::now() - chrono::Duration::minutes(10);

    assert!(!state.is_cache_likely_warm(), "Cache should be cold");

    // SECOND REQUEST with cold cache
    let (prompt2, _, cached2) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        Some(&state),
    );
    let metrics2 = analyze_prompt(&prompt2, cached2);

    print_comparison("Scenario 3: Cold cache - no incremental savings", &metrics1, &metrics2);

    // Should have NO savings with cold cache
    assert_eq!(metrics2.cached_sections, 0, "Cold cache should not use cached sections");
    assert_eq!(metrics1.total_bytes, metrics2.total_bytes, "Prompts should be identical size");
}

/// SCENARIO 4: Rapid-fire requests (simulating autocomplete or streaming)
#[test]
fn scenario_4_rapid_fire_requests() {
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  SCENARIO 4: Rapid-Fire Requests (5 requests in sequence)            ║");
    println!("║  Simulates: Multiple tool calls in single operation                  ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];
    let recall_context = create_realistic_recall_context();
    let code_context = create_code_context();

    let mut total_bytes_without_cache = 0;
    let mut total_bytes_with_cache = 0;
    let mut total_tokens_without_cache = 0;
    let mut total_tokens_with_cache = 0;
    let mut state: Option<SessionCacheState> = None;

    println!("\nRequest-by-request breakdown:");
    println!("{}", "-".repeat(70));
    println!("{:>8} {:>10} {:>10} {:>8} {:>15}", "Request", "Bytes", "Tokens", "Cached", "Savings");
    println!("{}", "-".repeat(70));

    for i in 1..=5 {
        let (prompt, hashes, cached) = UnifiedPromptBuilder::build_system_prompt_cached(
            &persona,
            &recall_context,
            Some(&tools),
            None,
            Some("mira-project"),
            Some(&code_context),
            None,
            state.as_ref(),
        );

        let bytes = prompt.len();
        let tokens = SessionCacheState::estimate_tokens(&prompt);

        if i == 1 {
            total_bytes_without_cache = bytes * 5; // What it would be without caching
            total_tokens_without_cache = tokens * 5;
        }
        total_bytes_with_cache += bytes;
        total_tokens_with_cache += tokens;

        let savings = if i == 1 {
            "N/A (baseline)".to_string()
        } else {
            let first_bytes = total_bytes_without_cache / 5;
            format!("{} bytes", first_bytes as i64 - bytes as i64)
        };

        println!("{:>8} {:>10} {:>10} {:>8} {:>15}",
            i, bytes, tokens, cached, savings);

        // Update state for next request
        if state.is_none() {
            let static_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(&persona, Some(&tools), true);
            let static_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(&persona, Some(&tools), true);
            state = Some(SessionCacheState::new("test".to_string(), static_hash, static_tokens));
        }
        if let Some(ref mut s) = state {
            s.update_after_call(hashes, 0);
        }
    }

    let bytes_saved = total_bytes_without_cache - total_bytes_with_cache;
    let bytes_pct = (bytes_saved as f64 / total_bytes_without_cache as f64) * 100.0;
    let tokens_saved = total_tokens_without_cache - total_tokens_with_cache;
    let tokens_pct = (tokens_saved as f64 / total_tokens_without_cache as f64) * 100.0;

    println!("{}", "-".repeat(70));
    println!("\nTOTALS FOR 5 REQUESTS:");
    println!("                          WITHOUT CACHE    WITH CACHE       SAVED");
    println!("{}", "-".repeat(70));
    println!("Total Bytes:        {:>15} {:>15} {:>10} ({:.1}%)",
        total_bytes_without_cache, total_bytes_with_cache, bytes_saved, bytes_pct);
    println!("Total Tokens:       {:>15} {:>15} {:>10} ({:.1}%)",
        total_tokens_without_cache, total_tokens_with_cache, tokens_saved, tokens_pct);
}

/// SCENARIO 5: Cost estimation
#[test]
fn scenario_5_cost_estimation() {
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  SCENARIO 5: Cost Estimation (GPT-5.1 Pricing)                       ║");
    println!("║  Input: $1.25/1M tokens, Cached: $0.125/1M (90% discount)            ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];
    let recall_context = create_realistic_recall_context();
    let code_context = create_code_context();

    // Pricing constants (GPT-5.1)
    const INPUT_PRICE_PER_M: f64 = 1.25;
    const CACHED_PRICE_PER_M: f64 = 0.125; // 90% discount

    // FIRST REQUEST
    let (prompt1, hashes1, _) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        None,
    );
    let tokens1 = SessionCacheState::estimate_tokens(&prompt1);

    // Create cache state
    let static_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(&persona, Some(&tools), true);
    let static_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(&persona, Some(&tools), true);
    let mut state = SessionCacheState::new("test".to_string(), static_hash, static_tokens);
    state.update_after_call(hashes1, 0);

    // SECOND REQUEST (cached)
    let (prompt2, _, _) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("mira-project"),
        Some(&code_context),
        None,
        Some(&state),
    );
    let tokens2 = SessionCacheState::estimate_tokens(&prompt2);

    // Calculate costs
    let cost_without_cache = (tokens1 as f64 / 1_000_000.0) * INPUT_PRICE_PER_M;
    let cost_with_incremental = (tokens2 as f64 / 1_000_000.0) * INPUT_PRICE_PER_M;

    // With OpenAI prompt caching on static prefix
    let dynamic_tokens = tokens1 - static_tokens;
    let cost_with_openai_cache =
        (static_tokens as f64 / 1_000_000.0) * CACHED_PRICE_PER_M +
        (dynamic_tokens as f64 / 1_000_000.0) * INPUT_PRICE_PER_M;

    // Combined: incremental context + OpenAI caching
    let dynamic_tokens_reduced = tokens2 - static_tokens;
    let cost_combined =
        (static_tokens as f64 / 1_000_000.0) * CACHED_PRICE_PER_M +
        (dynamic_tokens_reduced.max(0) as f64 / 1_000_000.0) * INPUT_PRICE_PER_M;

    println!("\nToken Counts:");
    println!("  First request (no cache):  {:>6} tokens", tokens1);
    println!("  Second request (cached):   {:>6} tokens", tokens2);
    println!("  Token reduction:           {:>6} tokens ({:.1}%)",
        tokens1 - tokens2,
        ((tokens1 - tokens2) as f64 / tokens1 as f64) * 100.0);
    println!();
    println!("  Static prefix (OpenAI cacheable): {:>6} tokens", static_tokens);
    println!("  Dynamic content (first):          {:>6} tokens", dynamic_tokens);
    println!("  Dynamic content (cached):         {:>6} tokens", dynamic_tokens_reduced);
    println!();
    println!("Cost per Request (input tokens only):");
    println!("{}", "-".repeat(70));
    println!("  A) No caching at all:              ${:.6} / request", cost_without_cache);
    println!("  B) OpenAI cache only:              ${:.6} / request", cost_with_openai_cache);
    println!("  C) Incremental context only:       ${:.6} / request", cost_with_incremental);
    println!("  D) BOTH optimizations:             ${:.6} / request", cost_combined);
    println!("{}", "-".repeat(70));

    let savings_vs_none = ((cost_without_cache - cost_combined) / cost_without_cache) * 100.0;
    let savings_vs_openai = ((cost_with_openai_cache - cost_combined) / cost_with_openai_cache) * 100.0;

    println!();
    println!("Savings Analysis:");
    println!("  D vs A (both vs none):     {:.1}% reduction", savings_vs_none);
    println!("  D vs B (added value of incremental): {:.1}% additional reduction", savings_vs_openai);
    println!();
    println!("Projected Monthly Savings (10,000 requests):");
    println!("{}", "-".repeat(70));
    let monthly_a = cost_without_cache * 10_000.0;
    let monthly_b = cost_with_openai_cache * 10_000.0;
    let monthly_d = cost_combined * 10_000.0;
    println!("  Without any caching:       ${:>8.2}", monthly_a);
    println!("  OpenAI cache only:         ${:>8.2}  (saves ${:.2})", monthly_b, monthly_a - monthly_b);
    println!("  Both optimizations:        ${:>8.2}  (saves ${:.2})", monthly_d, monthly_a - monthly_d);
    println!("{}", "-".repeat(70));
    println!("  TOTAL MONTHLY SAVINGS:     ${:>8.2}", monthly_a - monthly_d);
}

/// Summary test that runs all scenarios
#[test]
fn run_all_scenarios_summary() {
    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║         INCREMENTAL CONTEXT CACHING - A/B TEST SUMMARY               ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Run individual scenario tests for detailed breakdowns:");
    println!("  cargo test scenario_1 -- --nocapture");
    println!("  cargo test scenario_2 -- --nocapture");
    println!("  cargo test scenario_3 -- --nocapture");
    println!("  cargo test scenario_4 -- --nocapture");
    println!("  cargo test scenario_5 -- --nocapture");
    println!();
    println!("Or run all with:");
    println!("  cargo test --test cache_ab_comparison_test -- --nocapture");
}
