// backend/tests/cache_incremental_test.rs
// Integration test for incremental context caching

use mira_backend::cache::SessionCacheState;
use mira_backend::memory::features::recall_engine::RecallContext;
use mira_backend::persona::PersonaOverlay;
use mira_backend::prompt::UnifiedPromptBuilder;

/// Test that cache-aware prompt builder produces smaller prompts on second call
/// This test doesn't need database - just tests the pure prompt building logic
#[test]
fn test_incremental_context_reduces_prompt_size() {
    // Create test context (simulates memory context)
    let recall_context = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: Some("This is a test rolling summary with some content that should be cached. User has been working on a Rust project.".to_string()),
        session_summary: Some("Session focused on implementing caching features.".to_string()),
        code_intelligence: None,
    };

    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];

    // FIRST CALL: No cache state exists yet
    let (prompt1, hashes1, cached_sections1) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        None,
        None, // No cache state
    );

    assert_eq!(cached_sections1, 0, "First call should have 0 cached sections");
    println!("First prompt length: {} chars", prompt1.len());
    println!("First call cached_sections: {}", cached_sections1);

    // Create state after first call (simulating what SessionCacheStore.upsert would do)
    let static_prefix_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(
        &persona,
        Some(&tools),
        true,
    );
    let static_prefix_tokens = UnifiedPromptBuilder::estimate_static_prefix_tokens(
        &persona,
        Some(&tools),
        true,
    );

    let mut state = SessionCacheState::new(
        "test-session".to_string(),
        static_prefix_hash.clone(),
        static_prefix_tokens,
    );
    state.update_after_call(hashes1.clone(), 0);

    // Verify cache is warm
    assert!(state.is_cache_likely_warm(), "Cache should be warm immediately after creation");

    // SECOND CALL: Same context, should use cached references
    let (prompt2, hashes2, cached_sections2) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        None,
        Some(&state), // Pass cache state
    );

    println!("Second prompt length: {} chars", prompt2.len());
    println!("Second call cached_sections: {}", cached_sections2);

    // Should have cached sections since context is identical
    assert!(
        cached_sections2 > 0,
        "Second call should have cached sections (got {})",
        cached_sections2
    );

    // Second prompt should be smaller due to cached markers
    assert!(
        prompt2.len() < prompt1.len(),
        "Second prompt ({} chars) should be smaller than first ({} chars)",
        prompt2.len(),
        prompt1.len()
    );

    let reduction = ((prompt1.len() - prompt2.len()) as f64 / prompt1.len() as f64) * 100.0;
    println!("Prompt size reduction: {:.1}%", reduction);

    // Verify hashes are consistent
    assert_eq!(
        hashes1.memory_context, hashes2.memory_context,
        "Memory context hash should be identical"
    );
    assert_eq!(
        hashes1.project_context, hashes2.project_context,
        "Project context hash should be identical"
    );

    // Verify the cached markers are in the second prompt
    assert!(
        prompt2.contains("unchanged from previous context"),
        "Second prompt should contain cached markers"
    );
}

/// Test that changed context invalidates cache
#[test]
fn test_changed_context_invalidates_cache() {
    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];

    // First context
    let recall_context1 = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: Some("First summary content".to_string()),
        session_summary: None,
        code_intelligence: None,
    };

    // Build first prompt
    let (_, hashes1, _) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context1,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        None,
        None,
    );

    // Create state from first call
    let static_prefix_hash = UnifiedPromptBuilder::calculate_static_prefix_hash(
        &persona,
        Some(&tools),
        true,
    );
    let mut state = SessionCacheState::new(
        "test-session".to_string(),
        static_prefix_hash,
        1000,
    );
    state.update_after_call(hashes1, 0);

    // CHANGED context
    let recall_context2 = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: Some("COMPLETELY DIFFERENT summary content that should not match".to_string()),
        session_summary: None,
        code_intelligence: None,
    };

    // Build second prompt with changed context
    let (prompt2, _, cached_sections) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context2,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        None,
        Some(&state),
    );

    println!("Changed context cached_sections: {}", cached_sections);

    // Memory section should NOT be cached since content changed
    // But project section could still be cached (same project)
    assert!(
        !prompt2.contains("[MEMORY: unchanged"),
        "Changed memory should not use cached marker"
    );

    // The new content should be in the prompt
    assert!(
        prompt2.contains("COMPLETELY DIFFERENT"),
        "New content should be in prompt"
    );
}

/// Test that cold cache (>5 min) doesn't use cached markers
#[test]
fn test_cold_cache_uses_full_content() {
    let persona = PersonaOverlay::Default;
    let tools: Vec<mira_backend::tools::types::Tool> = vec![];

    let recall_context = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: Some("Test content".to_string()),
        session_summary: None,
        code_intelligence: None,
    };

    // Create a "cold" cache state (last call was 10 minutes ago)
    let mut state = SessionCacheState::new(
        "cold-session".to_string(),
        "hash".to_string(),
        1000,
    );

    // Manually set last_call_at to 10 minutes ago
    state.last_call_at = chrono::Utc::now() - chrono::Duration::minutes(10);

    // Verify cache is cold
    assert!(
        !state.is_cache_likely_warm(),
        "Cache should be cold after 10 minutes"
    );

    // Build prompt with cold cache
    let (prompt, _, cached_sections) = UnifiedPromptBuilder::build_system_prompt_cached(
        &persona,
        &recall_context,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        None,
        Some(&state),
    );

    // Should not use any cached sections when cache is cold
    assert_eq!(
        cached_sections, 0,
        "Cold cache should not use cached sections"
    );

    assert!(
        !prompt.contains("unchanged from previous context"),
        "Cold cache should not use cached markers"
    );
}
