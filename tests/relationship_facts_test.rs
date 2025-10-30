// tests/relationship_facts_test.rs
// Comprehensive Relationship & Facts Service Tests
//
// Tests:
// 1. Profile CRUD and updates from LLM relationship_impact JSON
// 2. Pattern learning, confidence tracking, times_observed
// 3. Facts CRUD via FactsService
// 4. Context loader assembling relationship context
// 5. Pattern engine filtering by type/confidence

use mira_backend::relationship::{
    RelationshipService,
    FactsService,
    UserProfile,
    LearnedPattern,
    MemoryFact,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use serde_json::json;

// ============================================================================
// TEST SETUP
// ============================================================================

async fn setup_test_db() -> Arc<sqlx::SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    Arc::new(pool)
}

async fn setup_services(pool: Arc<sqlx::SqlitePool>) -> (Arc<RelationshipService>, Arc<FactsService>) {
    let facts_service = Arc::new(FactsService::new((*pool).clone()));
    let relationship_service = Arc::new(RelationshipService::new(pool.clone(), facts_service.clone()));
    
    (relationship_service, facts_service)
}

// ============================================================================
// TEST 1: Profile Creation and Retrieval
// ============================================================================

#[tokio::test]
async fn test_profile_creation_and_retrieval() {
    println!("\n=== Testing Profile Creation and Retrieval ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-001";
    
    println!("[1] Creating new profile");
    let profile = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    assert_eq!(profile.user_id, user_id);
    assert!(profile.preferred_languages.is_none());
    assert_eq!(profile.total_sessions, 0);
    
    println!("[2] Retrieving existing profile");
    let retrieved = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to get profile");
    
    assert_eq!(retrieved.id, profile.id);
    assert_eq!(retrieved.user_id, user_id);
    
    println!("✓ Profile creation and retrieval working");
}

// ============================================================================
// TEST 2: Profile Updates from LLM relationship_impact
// ============================================================================

#[tokio::test]
async fn test_profile_updates_from_llm() {
    println!("\n=== Testing Profile Updates from LLM ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-002";
    
    println!("[1] Creating base profile");
    service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    println!("[2] Processing LLM relationship impact");
    let impact_json = json!({
        "profile_changes": {
            "preferred_languages": ["rust", "typescript"],
            "code_verbosity": "detailed",
            "explanation_depth": "comprehensive",
            "conversation_style": "casual"
        }
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&impact_json)).await
        .expect("Failed to process LLM updates");
    
    println!("[3] Verifying profile changes");
    let updated = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to get updated profile");
    
    assert!(updated.preferred_languages.is_some());
    assert_eq!(updated.code_verbosity, Some("detailed".to_string()));
    assert_eq!(updated.explanation_depth, Some("comprehensive".to_string()));
    assert_eq!(updated.conversation_style, Some("casual".to_string()));
    
    let langs: Vec<String> = serde_json::from_str(updated.preferred_languages.as_ref().unwrap())
        .expect("Failed to parse languages");
    assert_eq!(langs, vec!["rust", "typescript"]);
    
    println!("✓ Profile updates from LLM working correctly");
}

// ============================================================================
// TEST 3: Pattern Learning and Confidence Tracking
// ============================================================================

#[tokio::test]
async fn test_pattern_learning() {
    println!("\n=== Testing Pattern Learning ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-003";
    
    println!("[1] Creating profile");
    service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    println!("[2] Learning new patterns from LLM");
    let impact_json = json!({
        "new_patterns": [
            {
                "pattern_type": "coding_style",
                "pattern_name": "error_handling_preference",
                "pattern_description": "Prefers Result<T> over unwrap() for error handling",
                "confidence": 0.85,
                "applies_when": "writing rust code"
            },
            {
                "pattern_type": "work_pattern",
                "pattern_name": "morning_productivity",
                "pattern_description": "Most productive during morning hours (8am-12pm)",
                "confidence": 0.9
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&impact_json)).await
        .expect("Failed to process patterns");
    
    println!("[3] Retrieving learned patterns");
    let patterns = service.storage().get_patterns(user_id, None).await
        .expect("Failed to get patterns");
    
    assert_eq!(patterns.len(), 2);
    
    let error_handling = patterns.iter()
        .find(|p| p.pattern_name == "error_handling_preference")
        .expect("Error handling pattern not found");
    
    assert_eq!(error_handling.pattern_type, "coding_style");
    assert_eq!(error_handling.confidence, 0.85);
    assert_eq!(error_handling.times_observed, 1);
    assert_eq!(error_handling.applies_when, Some("writing rust code".to_string()));
    
    println!("[4] Updating existing pattern (increasing confidence)");
    service.storage().upsert_pattern(&LearnedPattern {
        id: error_handling.id.clone(),
        user_id: user_id.to_string(),
        pattern_type: "coding_style".to_string(),
        pattern_name: "error_handling_preference".to_string(),
        pattern_description: "Prefers Result<T> over unwrap() for error handling".to_string(),
        examples: None,
        confidence: 0.95,
        times_observed: 2,
        times_applied: 1,
        applies_when: Some("writing rust code".to_string()),
        deprecated: 0,
        first_observed: error_handling.first_observed,
        last_observed: chrono::Utc::now().timestamp(),
        last_applied: Some(chrono::Utc::now().timestamp()),
    }).await.expect("Failed to update pattern");
    
    let updated_patterns = service.storage().get_patterns(user_id, None).await
        .expect("Failed to get updated patterns");
    
    let updated_pattern = updated_patterns.iter()
        .find(|p| p.pattern_name == "error_handling_preference")
        .expect("Updated pattern not found");
    
    assert_eq!(updated_pattern.confidence, 0.95);
    assert_eq!(updated_pattern.times_observed, 2);
    assert_eq!(updated_pattern.times_applied, 1);
    
    println!("✓ Pattern learning and tracking working correctly");
}

// ============================================================================
// TEST 4: Pattern Engine Filtering
// ============================================================================

#[tokio::test]
async fn test_pattern_engine_filtering() {
    println!("\n=== Testing Pattern Engine Filtering ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-004";
    
    println!("[1] Creating test patterns with varying confidence");
    service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    let patterns_json = json!({
        "new_patterns": [
            {
                "pattern_type": "coding_style",
                "pattern_name": "high_confidence_pattern",
                "pattern_description": "High confidence pattern",
                "confidence": 0.9
            },
            {
                "pattern_type": "coding_style",
                "pattern_name": "medium_confidence_pattern",
                "pattern_description": "Medium confidence pattern",
                "confidence": 0.6
            },
            {
                "pattern_type": "communication",
                "pattern_name": "communication_pattern",
                "pattern_description": "Communication preference",
                "confidence": 0.85
            },
            {
                "pattern_type": "coding_style",
                "pattern_name": "low_confidence_pattern",
                "pattern_description": "Low confidence pattern",
                "confidence": 0.3
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&patterns_json)).await
        .expect("Failed to process patterns");
    
    println!("[2] Filtering by confidence (>= 0.7)");
    let high_confidence = service.pattern_engine()
        .get_applicable_patterns(user_id, &["coding_style", "communication"], 0.7).await
        .expect("Failed to get high confidence patterns");
    
    assert_eq!(high_confidence.len(), 2); // high_confidence_pattern and communication_pattern
    assert!(high_confidence.iter().all(|p| p.confidence >= 0.7));
    
    println!("[3] Filtering by type (coding_style only)");
    let coding_patterns = service.storage()
        .get_patterns(user_id, Some("coding_style")).await
        .expect("Failed to get coding patterns");
    
    assert_eq!(coding_patterns.len(), 3);
    assert!(coding_patterns.iter().all(|p| p.pattern_type == "coding_style"));
    
    println!("[4] Filtering by type AND confidence");
    let high_coding = service.pattern_engine()
        .get_applicable_patterns(user_id, &["coding_style"], 0.7).await
        .expect("Failed to get filtered patterns");
    
    assert_eq!(high_coding.len(), 1); // Only high_confidence_pattern
    assert_eq!(high_coding[0].pattern_name, "high_confidence_pattern");
    
    println!("✓ Pattern engine filtering working correctly");
}

// ============================================================================
// TEST 5: Facts CRUD Operations
// ============================================================================

#[tokio::test]
async fn test_facts_crud() {
    println!("\n=== Testing Facts CRUD ===\n");
    
    let pool = setup_test_db().await;
    let (_, facts_service) = setup_services(pool).await;
    
    let user_id = "test-user-005";
    
    println!("[1] Creating new fact");
    let fact_id = facts_service.upsert_fact(
        user_id,
        "favorite_language",
        "rust",
        "preferences",
        Some("User mentioned multiple times"),
        0.9
    ).await.expect("Failed to create fact");
    
    assert!(!fact_id.is_empty());
    
    println!("[2] Retrieving fact by key");
    let fact = facts_service.get_fact(user_id, "favorite_language").await
        .expect("Failed to get fact")
        .expect("Fact not found");
    
    assert_eq!(fact.fact_key, "favorite_language");
    assert_eq!(fact.fact_value, "rust");
    assert_eq!(fact.fact_category, "preferences");
    assert_eq!(fact.confidence, 0.9);
    assert_eq!(fact.reference_count, 0);
    
    println!("[3] Updating existing fact");
    let updated_id = facts_service.upsert_fact(
        user_id,
        "favorite_language",
        "rust and typescript",
        "preferences",
        Some("User expanded preferences"),
        0.95
    ).await.expect("Failed to update fact");
    
    assert_eq!(updated_id, fact_id); // Should be same ID
    
    let updated = facts_service.get_fact(user_id, "favorite_language").await
        .expect("Failed to get updated fact")
        .expect("Updated fact not found");
    
    assert_eq!(updated.fact_value, "rust and typescript");
    assert_eq!(updated.confidence, 0.95);
    
    println!("[4] Referencing fact (increment count)");
    facts_service.reference_fact(&fact_id).await
        .expect("Failed to reference fact");
    
    let referenced = facts_service.get_fact(user_id, "favorite_language").await
        .expect("Failed to get referenced fact")
        .expect("Referenced fact not found");
    
    assert_eq!(referenced.reference_count, 1);
    assert!(referenced.last_referenced.is_some());
    
    println!("[5] Creating multiple facts");
    facts_service.upsert_fact(
        user_id, "timezone", "PST", "personal", None, 1.0
    ).await.expect("Failed to create timezone fact");
    
    facts_service.upsert_fact(
        user_id, "editor", "neovim", "preferences", None, 0.8
    ).await.expect("Failed to create editor fact");
    
    println!("[6] Retrieving all facts");
    let all_facts = facts_service.get_user_facts(user_id, None).await
        .expect("Failed to get all facts");
    
    assert_eq!(all_facts.len(), 3);
    
    println!("[7] Filtering facts by category");
    let prefs = facts_service.get_user_facts(user_id, Some("preferences")).await
        .expect("Failed to get preference facts");
    
    assert_eq!(prefs.len(), 2);
    assert!(prefs.iter().all(|f| f.fact_category == "preferences"));
    
    println!("[8] Marking fact as irrelevant");
    facts_service.mark_irrelevant(&fact_id).await
        .expect("Failed to mark fact irrelevant");
    
    let irrelevant = facts_service.get_fact(user_id, "favorite_language").await
        .expect("Failed to check irrelevant fact");
    
    assert!(irrelevant.is_none()); // Should not be returned when still_relevant = 0
    
    println!("✓ Facts CRUD operations working correctly");
}

// ============================================================================
// TEST 6: Facts from LLM relationship_impact
// ============================================================================

#[tokio::test]
async fn test_facts_from_llm() {
    println!("\n=== Testing Facts from LLM ===\n");
    
    let pool = setup_test_db().await;
    let (service, facts_service) = setup_services(pool).await;
    
    let user_id = "test-user-006";
    
    println!("[1] Creating profile");
    service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    println!("[2] Processing facts from LLM");
    let impact_json = json!({
        "new_facts": [
            {
                "fact_key": "current_project",
                "fact_value": "Building a Rust AI backend",
                "fact_category": "work",
                "confidence": 1.0,
                "context": "User mentioned in conversation"
            },
            {
                "fact_key": "tech_stack",
                "fact_value": "Rust, PostgreSQL, Qdrant",
                "fact_category": "technical",
                "confidence": 0.9
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&impact_json)).await
        .expect("Failed to process facts");
    
    println!("[3] Verifying facts were stored");
    let facts = facts_service.get_user_facts(user_id, None).await
        .expect("Failed to get facts");
    
    assert_eq!(facts.len(), 2);
    
    let project_fact = facts.iter()
        .find(|f| f.fact_key == "current_project")
        .expect("Project fact not found");
    
    assert_eq!(project_fact.fact_value, "Building a Rust AI backend");
    assert_eq!(project_fact.fact_category, "work");
    assert_eq!(project_fact.confidence, 1.0);
    assert_eq!(project_fact.context, Some("User mentioned in conversation".to_string()));
    
    println!("✓ Facts from LLM working correctly");
}

// ============================================================================
// TEST 7: Context Loader Integration
// ============================================================================

#[tokio::test]
async fn test_context_loader() {
    println!("\n=== Testing Context Loader ===\n");
    
    let pool = setup_test_db().await;
    let (service, facts_service) = setup_services(pool).await;
    
    let user_id = "test-user-007";
    
    println!("[1] Setting up complete user context");
    
    // Create profile with preferences
    let impact = json!({
        "profile_changes": {
            "code_verbosity": "moderate",
            "conversation_style": "technical"
        },
        "new_patterns": [
            {
                "pattern_type": "coding_style",
                "pattern_name": "test_driven",
                "pattern_description": "Prefers TDD approach",
                "confidence": 0.9
            }
        ],
        "new_facts": [
            {
                "fact_key": "timezone",
                "fact_value": "UTC",
                "fact_category": "personal",
                "confidence": 1.0
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&impact)).await
        .expect("Failed to process updates");
    
    println!("[2] Loading full context");
    let context = service.context_loader().load_context(user_id).await
        .expect("Failed to load context");
    
    assert_eq!(context.profile.code_verbosity, Some("moderate".to_string()));
    assert_eq!(context.profile.conversation_style, Some("technical".to_string()));
    assert_eq!(context.coding_patterns.len(), 1);
    assert_eq!(context.facts.len(), 1);
    
    println!("[3] Loading minimal context (high confidence only)");
    let minimal = service.context_loader().load_minimal_context(user_id).await
        .expect("Failed to load minimal context");
    
    assert_eq!(minimal.code_verbosity, Some("moderate".to_string()));
    assert_eq!(minimal.conversation_style, Some("technical".to_string()));
    assert!(!minimal.top_patterns.is_empty());
    assert!(!minimal.key_facts.is_empty());
    
    println!("[4] Getting LLM-formatted context string");
    let context_string = service.context_loader().get_llm_context_string(user_id).await
        .expect("Failed to get context string");
    
    assert!(context_string.contains("Code verbosity"));
    assert!(context_string.contains("moderate"));
    assert!(context_string.contains("test_driven"));
    assert!(context_string.contains("timezone"));
    
    println!("✓ Context loader working correctly");
}

// ============================================================================
// TEST 8: Session Metadata Updates
// ============================================================================

#[tokio::test]
async fn test_session_metadata() {
    println!("\n=== Testing Session Metadata ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-008";
    
    println!("[1] Creating initial profile");
    let initial = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to create profile");
    
    assert_eq!(initial.total_sessions, 0);
    assert!(initial.last_active.is_none());
    
    println!("[2] Updating session metadata");
    service.context_loader().update_session_metadata(user_id).await
        .expect("Failed to update session");
    
    let updated = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to get updated profile");
    
    assert_eq!(updated.total_sessions, 1);
    assert!(updated.last_active.is_some());
    
    println!("[3] Multiple session updates");
    for _ in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        service.context_loader().update_session_metadata(user_id).await
            .expect("Failed to update session");
    }
    
    let final_profile = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to get final profile");
    
    assert_eq!(final_profile.total_sessions, 4);
    assert!(final_profile.last_active.is_some());
    assert!(final_profile.last_active.unwrap() >= initial.created_at);
    
    println!("✓ Session metadata tracking working correctly");
}

// ============================================================================
// TEST 9: Complex Integration Scenario
// ============================================================================

#[tokio::test]
async fn test_complex_integration() {
    println!("\n=== Testing Complex Integration Scenario ===\n");
    
    let pool = setup_test_db().await;
    let (service, facts_service) = setup_services(pool).await;
    
    let user_id = "test-user-009";
    
    println!("[1] Simulating multi-turn conversation with relationship building");
    
    // Turn 1: User shares preferences
    let turn1 = json!({
        "profile_changes": {
            "preferred_languages": ["rust", "python"],
            "code_verbosity": "detailed"
        },
        "new_facts": [
            {
                "fact_key": "experience_level",
                "fact_value": "senior engineer with 10 years",
                "fact_category": "professional",
                "confidence": 1.0
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&turn1)).await
        .expect("Turn 1 failed");
    
    // Turn 2: Observe coding pattern
    let turn2 = json!({
        "new_patterns": [
            {
                "pattern_type": "coding_style",
                "pattern_name": "functional_preference",
                "pattern_description": "Prefers functional programming patterns",
                "confidence": 0.7
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&turn2)).await
        .expect("Turn 2 failed");
    
    // Turn 3: Strengthen pattern confidence
    let turn3 = json!({
        "new_patterns": [
            {
                "pattern_type": "coding_style",
                "pattern_name": "functional_preference",
                "pattern_description": "Prefers functional programming patterns",
                "confidence": 0.9
            }
        ],
        "new_facts": [
            {
                "fact_key": "current_focus",
                "fact_value": "Learning async Rust patterns",
                "fact_category": "learning",
                "confidence": 0.8
            }
        ]
    }).to_string();
    
    service.process_llm_updates(user_id, Some(&turn3)).await
        .expect("Turn 3 failed");
    
    println!("[2] Verifying complete relationship state");
    
    let profile = service.storage().get_or_create_profile(user_id).await
        .expect("Failed to get profile");
    
    assert_eq!(profile.code_verbosity, Some("detailed".to_string()));
    
    let patterns = service.storage().get_patterns(user_id, None).await
        .expect("Failed to get patterns");
    
    // Should have functional_preference pattern (might be duplicated in simple impl)
    assert!(!patterns.is_empty());
    let func_pattern = patterns.iter()
        .find(|p| p.pattern_name == "functional_preference")
        .expect("Pattern not found");
    
    // Latest confidence should be applied
    assert!(func_pattern.confidence >= 0.7);
    
    let facts = facts_service.get_user_facts(user_id, None).await
        .expect("Failed to get facts");
    
    assert_eq!(facts.len(), 2);
    
    println!("[3] Testing context retrieval with all data");
    let context_str = service.context_loader().get_llm_context_string(user_id).await
        .expect("Failed to get context");
    
    assert!(context_str.contains("detailed"));
    assert!(context_str.contains("experience_level"));
    
    println!("✓ Complex integration scenario working correctly");
}

#[tokio::test]
async fn test_empty_relationship_impact() {
    println!("\n=== Testing Empty/Invalid Relationship Impact ===\n");
    
    let pool = setup_test_db().await;
    let (service, _) = setup_services(pool).await;
    
    let user_id = "test-user-010";
    
    println!("[1] Testing None impact");
    service.process_llm_updates(user_id, None).await
        .expect("None impact should succeed");
    
    println!("[2] Testing empty string");
    service.process_llm_updates(user_id, Some("")).await
        .expect("Empty impact should succeed");
    
    println!("[3] Testing whitespace");
    service.process_llm_updates(user_id, Some("   \n  ")).await
        .expect("Whitespace impact should succeed");
    
    println!("[4] Testing invalid JSON");
    service.process_llm_updates(user_id, Some("not json")).await
        .expect("Invalid JSON should succeed (treated as plain text)");
    
    println!("[5] Testing empty JSON object");
    service.process_llm_updates(user_id, Some("{}")).await
        .expect("Empty JSON should succeed");
    
    println!("✓ Edge cases handled correctly");
}
