// tests/message_pipeline_flow_test.rs
// Tests message analysis pipeline - tags, topics, salience, error detection
// FIXME: All tests require GPT 5.1 response format work

use mira_backend::llm::provider::{LlmProvider, gpt5::{Gpt5Provider, ReasoningEffort}};
use mira_backend::memory::features::message_pipeline::MessagePipeline;
use std::sync::Arc;

// ============================================================================
// TEST 1: Basic Message Analysis Flow
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_message_analysis_flow() {
    let pipeline = setup_pipeline().await;

    let test_content = "Here's a bug fix for the authentication handler in Rust. The issue was with token validation.";
    let role = "user";

    // Execute analysis
    let result = pipeline
        .analyze_message(test_content, role, None)
        .await
        .expect("Analysis should succeed");

    // Verify analysis structure
    assert!(
        result.should_embed,
        "Code-related message should be embedded"
    );
    assert!(!result.analysis.topics.is_empty(), "Should extract topics");
    assert!(result.analysis.salience > 0.0, "Should have salience score");
    assert!(
        result.analysis.salience <= 1.0,
        "Salience should be normalized"
    );

    // Verify code detection
    assert!(result.analysis.is_code, "Should detect code content");
    assert!(
        result.analysis.programming_lang.is_some(),
        "Should detect language"
    );

    // Check that embedding routing was decided
    assert!(
        result.analysis.routing.should_embed,
        "Should route for embedding"
    );
    assert!(
        !result.analysis.routing.embedding_heads.is_empty(),
        "Should have routing heads"
    );

    println!("✓ Message analysis flow working");
    println!("  Topics: {:?}", result.analysis.topics);
    println!("  Salience: {}", result.analysis.salience);
    println!("  Language: {:?}", result.analysis.programming_lang);
}

// ============================================================================
// TEST 2: Low-Value Message Filtering
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_low_value_message_skips_embedding() {
    let pipeline = setup_pipeline().await;

    let trivial_messages = vec!["ok", "thanks", "got it", "👍", "k"];

    for test_content in trivial_messages {
        let result = pipeline
            .analyze_message(test_content, "user", None)
            .await
            .expect("Analysis should succeed");

        // Trivial messages should not be embedded
        assert!(
            !result.should_embed,
            "Trivial message '{}' shouldn't be embedded",
            test_content
        );
        assert!(
            result.analysis.salience < 0.5,
            "Trivial message '{}' should have low salience",
            test_content
        );
    }

    println!("✓ Low-value message filtering working");
}

// ============================================================================
// TEST 3: High-Value Message Detection
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_high_value_messages_are_embedded() {
    let pipeline = setup_pipeline().await;

    let high_value_messages = vec![
        "Let's implement OAuth2 authentication with PKCE flow",
        "The database migration failed because of foreign key constraints",
        "Here's the complete implementation of the binary search tree",
        "I need to optimize the SQL query performance - it's taking 5 seconds",
    ];

    for test_content in high_value_messages {
        let result = pipeline
            .analyze_message(test_content, "user", None)
            .await
            .expect("Analysis should succeed");

        assert!(
            result.should_embed,
            "High-value message should be embedded: {}",
            test_content
        );
        assert!(
            result.analysis.salience > 0.5,
            "High-value message should have high salience: {}",
            test_content
        );
    }

    println!("✓ High-value message detection working");
}

// ============================================================================
// TEST 4: Error Detection in Messages
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_error_detection_in_messages() {
    let pipeline = setup_pipeline().await;

    let error_messages = vec![
        (
            "Error: panic at 'index out of bounds' in src/main.rs:42",
            Some("src/main.rs"),
        ),
        (
            "TypeError: Cannot read property 'id' of undefined at app.js:156",
            Some("app.js"),
        ),
        (
            "Segmentation fault (core dumped) in parser.c line 89",
            Some("parser.c"),
        ),
        (
            "NullPointerException in AuthService.java:234",
            Some("AuthService.java"),
        ),
    ];

    for (test_content, expected_file) in error_messages {
        let result = pipeline
            .analyze_message(test_content, "assistant", None)
            .await
            .expect("Analysis should succeed");

        // Should detect error
        assert!(
            result.analysis.contains_error,
            "Should detect error in: {}",
            test_content
        );

        // Should classify error type
        assert!(
            result.analysis.error_type.is_some(),
            "Should classify error type in: {}",
            test_content
        );

        // Should extract file if present
        if let Some(expected) = expected_file {
            assert!(
                result.analysis.error_file.is_some(),
                "Should extract error file from: {}",
                test_content
            );
            let extracted_file = result.analysis.error_file.as_ref().unwrap();
            assert!(
                extracted_file.contains(expected),
                "Expected file '{}', got '{}'",
                expected,
                extracted_file
            );
        }

        // Errors are high-value
        assert!(
            result.should_embed,
            "Error messages should be embedded: {}",
            test_content
        );
    }

    println!("✓ Error detection working");
}

// ============================================================================
// TEST 5: Topic Extraction
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_topic_extraction() {
    let pipeline = setup_pipeline().await;

    let message_with_topics = "I'm working on implementing a Redis cache layer with connection pooling and automatic failover for our microservices architecture";

    let result = pipeline
        .analyze_message(message_with_topics, "user", None)
        .await
        .expect("Analysis should succeed");

    // Should extract multiple topics
    assert!(!result.analysis.topics.is_empty(), "Should extract topics");
    assert!(
        result.analysis.topics.len() >= 2,
        "Should extract multiple topics"
    );

    // Topics should be lowercase and relevant
    let topics_str = result.analysis.topics.join(" ").to_lowercase();

    // Check for key technical terms (flexible matching)
    let has_redis = topics_str.contains("redis") || topics_str.contains("cache");
    let has_architecture = topics_str.contains("architecture")
        || topics_str.contains("microservice")
        || topics_str.contains("service");

    assert!(
        has_redis || has_architecture,
        "Should extract relevant topics. Got: {:?}",
        result.analysis.topics
    );

    println!("✓ Topic extraction working");
    println!("  Extracted topics: {:?}", result.analysis.topics);
}

// ============================================================================
// TEST 6: Programming Language Detection
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_programming_language_detection() {
    let pipeline = setup_pipeline().await;

    let language_tests = vec![
        (
            "Here's a Python function using async/await",
            vec!["python", "py"],
        ),
        (
            "This Rust function uses Result<T, E> for error handling",
            vec!["rust", "rs"],
        ),
        (
            "JavaScript arrow function: const add = (a, b) => a + b",
            vec!["javascript", "js"],
        ),
        (
            "SQL query: SELECT * FROM users WHERE active = true",
            vec!["sql"],
        ),
    ];

    for (message, expected_langs) in language_tests {
        let result = pipeline
            .analyze_message(message, "user", None)
            .await
            .expect("Analysis should succeed");

        if result.analysis.is_code {
            let detected = result
                .analysis
                .programming_lang
                .as_ref()
                .expect("Should detect language for code content");

            let detected_lower = detected.to_lowercase();
            let matched = expected_langs
                .iter()
                .any(|lang| detected_lower.contains(lang));

            assert!(
                matched,
                "Expected one of {:?}, got '{}' for: {}",
                expected_langs, detected, message
            );
        }
    }

    println!("✓ Programming language detection working");
}

// ============================================================================
// TEST 7: Mood and Intent Analysis
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_mood_and_intent_analysis() {
    let pipeline = setup_pipeline().await;

    let test_content = "I'm really frustrated with this bug. It's been blocking me for hours.";

    let result = pipeline
        .analyze_message(test_content, "user", None)
        .await
        .expect("Analysis should succeed");

    // Should extract mood
    assert!(result.analysis.mood.is_some(), "Should detect mood");

    // Should detect intent
    assert!(result.analysis.intent.is_some(), "Should detect intent");

    // Intensity should be set for emotional content
    assert!(
        result.analysis.intensity.is_some(),
        "Should have intensity score"
    );

    println!("✓ Mood and intent analysis working");
    println!("  Mood: {:?}", result.analysis.mood);
    println!("  Intent: {:?}", result.analysis.intent);
}

// ============================================================================
// TEST 8: Salience Scoring Consistency
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_salience_scoring_consistency() {
    let pipeline = setup_pipeline().await;

    // High salience content
    let high_salience =
        "Critical security vulnerability found in authentication system. Immediate patch required.";
    let high_result = pipeline
        .analyze_message(high_salience, "user", None)
        .await
        .expect("Should analyze high salience message");

    // Low salience content
    let low_salience = "ok thanks";
    let low_result = pipeline
        .analyze_message(low_salience, "user", None)
        .await
        .expect("Should analyze low salience message");

    // Verify relative salience
    assert!(
        high_result.analysis.salience > low_result.analysis.salience,
        "High-value content should have higher salience. High: {}, Low: {}",
        high_result.analysis.salience,
        low_result.analysis.salience
    );

    assert!(
        high_result.analysis.salience > 0.7,
        "Critical content should have high salience score"
    );
    assert!(
        low_result.analysis.salience < 0.3,
        "Trivial content should have low salience score"
    );

    println!("✓ Salience scoring consistency verified");
    println!("  High salience: {}", high_result.analysis.salience);
    println!("  Low salience: {}", low_result.analysis.salience);
}

// ============================================================================
// TEST 9: Analysis Version Tracking
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_analysis_version_tracking() {
    let pipeline = setup_pipeline().await;

    let result = pipeline
        .analyze_message("test message", "user", None)
        .await
        .expect("Analysis should succeed");

    // Should have analysis version for tracking
    assert!(
        !result.analysis.analysis_version.is_empty(),
        "Should have analysis version"
    );

    println!("✓ Analysis version tracking working");
    println!("  Version: {}", result.analysis.analysis_version);
}

// ============================================================================
// TEST 10: Context-Enhanced Analysis
// ============================================================================

#[tokio::test]
#[ignore = "requires GPT 5.1 response format work"]
async fn test_context_enhanced_analysis() {
    let pipeline = setup_pipeline().await;

    let content = "Let's use that approach";
    let context =
        Some("We were discussing implementing a retry mechanism with exponential backoff");

    let result = pipeline
        .analyze_message(content, "user", context)
        .await
        .expect("Analysis should succeed");

    // With context, salience should be higher than without
    let result_no_context = pipeline
        .analyze_message(content, "user", None)
        .await
        .expect("Analysis should succeed");

    // Context should improve understanding
    assert!(
        result.analysis.salience >= result_no_context.analysis.salience,
        "Context should maintain or improve salience"
    );

    println!("✓ Context-enhanced analysis working");
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn setup_pipeline() -> MessagePipeline {
    // Create a real LLM provider (will use API)
    let llm_provider = create_llm_provider();

    MessagePipeline::new(llm_provider)
}

fn create_llm_provider() -> Arc<dyn LlmProvider> {
    // Load .env file
    let _ = dotenv::dotenv();
    // Get API key from environment
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for tests");

    Arc::new(Gpt5Provider::new(
        api_key,
        "gpt-5.1".to_string(),
        ReasoningEffort::Medium,
    ).expect("Should create GPT5 provider"))
}

// ============================================================================
// Test Configuration
// ============================================================================

// Run tests with: cargo test --test message_pipeline_flow_test -- --nocapture
// Requires: OPENAI_API_KEY environment variable set
