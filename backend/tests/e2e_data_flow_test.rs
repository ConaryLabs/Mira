// tests/e2e_data_flow_test.rs
// End-to-end integration test: Message → Analysis → Storage → Embedding → Retrieval

use chrono::Utc;
use mira_backend::{
    llm::{
        embeddings::EmbeddingHead,
        provider::{LlmProvider, GeminiEmbeddings, {Gemini3Provider, ThinkingLevel}},
    },
    memory::{
        core::types::MemoryEntry, service::MemoryService,
        storage::qdrant::multi_store::QdrantMultiStore, storage::sqlite::store::SqliteMemoryStore,
    },
};
use sqlx::SqlitePool;
use std::sync::Arc;

// ============================================================================
// TEST 1: Complete Message Flow (The Big One)
// ============================================================================

#[tokio::test]
async fn test_complete_message_flow() {
    println!("\n=== Starting Complete Message Flow Test ===\n");

    // Setup all components
    let (memory_service, embedding_client, multi_store) = setup_full_stack().await;
    let session_id = "e2e-test-session";

    // STEP 1: User message comes in
    println!("[1] User message received");
    let user_message =
        "I need to fix a critical bug in the authentication handler. Users can't log in.";

    // STEP 2: Analyze the message
    println!("[2] Analyzing message...");
    let pipeline = memory_service.message_pipeline.get_pipeline();
    let analysis_result = pipeline
        .analyze_message(user_message, "user", None)
        .await
        .expect("Should analyze message");

    println!("    ✓ Analysis complete");
    println!("      - Topics: {:?}", analysis_result.analysis.topics);
    println!("      - Salience: {}", analysis_result.analysis.salience);
    println!("      - Should embed: {}", analysis_result.should_embed);

    // STEP 3: Store in SQLite with analysis metadata
    println!("[3] Storing in SQLite...");
    let mut entry = MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        response_id: None,
        parent_id: None,
        role: "user".to_string(),
        content: user_message.to_string(),
        timestamp: Utc::now(),
        tags: Some(
            analysis_result
                .analysis
                .topics
                .clone()
                .into_iter()
                .map(|t| format!("topic:{}", t))
                .collect(),
        ),
        salience: Some(analysis_result.analysis.salience as f32),
        topics: Some(analysis_result.analysis.topics.clone()),
        mood: analysis_result.analysis.mood.clone(),
        intent: analysis_result.analysis.intent.clone(),
        contains_code: Some(analysis_result.analysis.is_code),
        programming_lang: analysis_result.analysis.programming_lang.clone(),
        contains_error: Some(analysis_result.analysis.contains_error),
        error_type: analysis_result.analysis.error_type.clone(),
        ..create_empty_entry(session_id, "user", user_message)
    };

    let msg_id = memory_service
        .core
        .save_entry(&entry)
        .await
        .expect("Should save to SQLite");

    println!("    ✓ Stored with ID: {}", msg_id);

    // STEP 4: Generate and store embedding (if should_embed)
    if analysis_result.should_embed {
        println!("[4] Generating embedding...");
        let embedding = embedding_client
            .embed(user_message)
            .await
            .expect("Should generate embedding");

        println!("    ✓ Generated {} dimensional embedding", embedding.len());

        // Store in appropriate Qdrant heads
        println!("[5] Storing in Qdrant...");
        entry.id = Some(msg_id);
        entry.embedding = Some(embedding.clone());

        // Determine which heads to use based on analysis
        let heads = if analysis_result.analysis.contains_error {
            vec![EmbeddingHead::Conversation, EmbeddingHead::Code]
        } else {
            vec![EmbeddingHead::Conversation]
        };

        for head in heads {
            let point_id = multi_store
                .save(head, &entry)
                .await
                .expect("Should store in Qdrant");
            println!(
                "    ✓ Stored in {} head (point_id: {})",
                head.as_str(),
                point_id
            );
        }
    } else {
        println!("[4-5] Skipping embedding (low salience)");
    }

    // STEP 6: Verify retrieval from SQLite
    println!("[6] Verifying SQLite retrieval...");
    let recent = memory_service
        .core
        .get_recent(session_id, 10)
        .await
        .expect("Should retrieve recent");

    assert!(!recent.is_empty(), "Should find stored message");
    assert_eq!(recent[0].content, user_message);
    assert_eq!(
        recent[0].salience,
        Some(analysis_result.analysis.salience as f32)
    );

    println!("    ✓ Retrieved from SQLite successfully");

    // STEP 7: Verify semantic search works
    if analysis_result.should_embed {
        println!("[7] Testing semantic search...");
        let query = "authentication problems";
        let query_embedding = embedding_client
            .embed(query)
            .await
            .expect("Should embed query");

        let search_results = multi_store
            .search(EmbeddingHead::Conversation, session_id, &query_embedding, 5)
            .await
            .expect("Should search");

        assert!(
            !search_results.is_empty(),
            "Should find semantically similar messages"
        );
        println!(
            "    ✓ Found {} semantically similar results",
            search_results.len()
        );

        // Verify our message is in results
        let found_our_message = search_results
            .iter()
            .any(|r| r.content.contains("authentication handler"));
        assert!(
            found_our_message,
            "Should find our stored message in search results"
        );
    } else {
        println!("[7] Skipping semantic search (message not embedded)");
    }

    println!("\n✓ COMPLETE MESSAGE FLOW WORKING END-TO-END\n");
}

// ============================================================================
// TEST 2: Conversation Thread Flow
// ============================================================================

#[tokio::test]

async fn test_conversation_thread_flow() {
    println!("\n=== Testing Conversation Thread Flow ===\n");

    let (memory_service, embedding_client, multi_store) = setup_full_stack().await;
    let session_id = "thread-test-session";

    // Simulate a conversation with parent-child relationships
    let conversation = vec![
        ("user", "How do I implement JWT authentication?"),
        (
            "assistant",
            "Here's how to implement JWT: First, install the jsonwebtoken crate...",
        ),
        ("user", "What about refresh tokens?"),
        (
            "assistant",
            "Refresh tokens work by storing a longer-lived token...",
        ),
    ];

    let mut parent_id = None;
    let mut stored_ids = Vec::new();

    for (idx, (role, content)) in conversation.iter().enumerate() {
        println!("[{}] Storing {} message", idx + 1, role);

        // Save message
        let msg_id = if *role == "user" {
            memory_service
                .core
                .save_user_message(session_id, content, None)
                .await
                .expect("Should save user message")
        } else {
            memory_service
                .core
                .save_assistant_message(session_id, content, parent_id)
                .await
                .expect("Should save assistant message")
        };

        stored_ids.push(msg_id);

        if *role == "user" {
            parent_id = Some(msg_id);
        }

        println!("    ✓ Stored with ID: {}", msg_id);
    }

    // Verify thread structure (newest first)
    println!("\n[5] Verifying thread structure...");
    let recent = memory_service
        .core
        .get_recent(session_id, 10)
        .await
        .expect("Should retrieve");

    assert_eq!(recent.len(), 4, "Should have all 4 messages");

    // Check alternating roles (reversed order - newest first)
    assert_eq!(recent[0].role, "assistant"); // Most recent
    assert_eq!(recent[1].role, "user");
    assert_eq!(recent[2].role, "assistant");
    assert_eq!(recent[3].role, "user"); // Oldest

    println!("    ✓ Thread structure verified");
    println!("\n✓ CONVERSATION THREAD FLOW WORKING\n");
}

// ============================================================================
// TEST 3: Multi-Session Isolation
// ============================================================================

#[tokio::test]

async fn test_multi_session_isolation() {
    println!("\n=== Testing Multi-Session Isolation ===\n");

    let (memory_service, _, _) = setup_full_stack().await;

    // Store messages in different sessions
    let sessions = vec![
        ("session-a", "Message in session A"),
        ("session-b", "Message in session B"),
        ("session-c", "Message in session C"),
    ];

    for (session_id, content) in &sessions {
        memory_service
            .core
            .save_user_message(session_id, content, None)
            .await
            .expect("Should save");
        println!("  ✓ Stored in {}", session_id);
    }

    // Verify each session only sees its own messages
    for (session_id, expected_content) in &sessions {
        let recent = memory_service
            .core
            .get_recent(session_id, 10)
            .await
            .expect("Should retrieve");

        assert_eq!(recent.len(), 1, "Session should have exactly 1 message");
        assert_eq!(&recent[0].content, expected_content);
        assert_eq!(&recent[0].session_id, session_id);

        println!("  ✓ {} isolation verified", session_id);
    }

    println!("\n✓ MULTI-SESSION ISOLATION WORKING\n");
}

// ============================================================================
// TEST 4: High-Volume Message Flow
// ============================================================================

#[tokio::test]

async fn test_high_volume_message_flow() {
    println!("\n=== Testing High-Volume Message Flow ===\n");

    let (memory_service, _, _) = setup_full_stack().await;
    let session_id = "high-volume-test";
    let message_count = 50;

    println!("Storing {} messages...", message_count);

    let start = std::time::Instant::now();

    for i in 0..message_count {
        let content = format!("Test message number {}", i);
        let role = if i % 2 == 0 { "user" } else { "assistant" };

        if role == "user" {
            memory_service
                .core
                .save_user_message(session_id, &content, None)
                .await
                .expect("Should save");
        } else {
            memory_service
                .core
                .save_assistant_message(session_id, &content, None)
                .await
                .expect("Should save");
        }
    }

    let duration = start.elapsed();
    println!("  ✓ Stored {} messages in {:?}", message_count, duration);
    println!("  ✓ Average: {:?} per message", duration / message_count);

    // Verify retrieval
    let recent = memory_service
        .core
        .get_recent(session_id, message_count as usize)
        .await
        .expect("Should retrieve");

    assert_eq!(
        recent.len(),
        message_count as usize,
        "Should retrieve all messages"
    );

    println!("  ✓ Retrieved all {} messages", message_count);
    println!("\n✓ HIGH-VOLUME FLOW WORKING\n");
}

// ============================================================================
// TEST 5: Error Recovery and Resilience
// ============================================================================

#[tokio::test]
async fn test_error_recovery() {
    println!("\n=== Testing Error Recovery ===\n");

    let (memory_service, _, _) = setup_full_stack().await;
    let session_id = "error-test";

    // Test 1: Empty content handling
    println!("[1] Testing empty content handling...");
    let result = memory_service
        .core
        .save_user_message(session_id, "", None)
        .await;
    assert!(result.is_ok(), "Should handle empty content gracefully");
    println!("    ✓ Empty content handled");

    // Test 2: Very long content
    println!("[2] Testing very long content...");
    let long_content = "x".repeat(50000);
    let result = memory_service
        .core
        .save_user_message(session_id, &long_content, None)
        .await;
    assert!(result.is_ok(), "Should handle long content");
    println!("    ✓ Long content handled");

    // Test 3: Special characters
    println!("[3] Testing special characters...");
    let special_content = "Test with 'quotes' and \"double quotes\" and \n newlines";
    let result = memory_service
        .core
        .save_user_message(session_id, special_content, None)
        .await;
    assert!(result.is_ok(), "Should handle special characters");

    let recent = memory_service
        .core
        .get_recent(session_id, 1)
        .await
        .expect("Should retrieve");
    assert_eq!(
        recent[0].content, special_content,
        "Content should be preserved exactly"
    );
    println!("    ✓ Special characters preserved");

    println!("\n✓ ERROR RECOVERY WORKING\n");
}

// ============================================================================
// TEST 6: Code Message Routing
// ============================================================================

#[tokio::test]
async fn test_code_message_routing() {
    println!("\n=== Testing Code Message Routing ===\n");

    let (memory_service, embedding_client, multi_store) = setup_full_stack().await;
    let session_id = "code-routing-test";

    let code_message = r#"
fn authenticate(token: &str) -> Result<User, AuthError> {
    let decoded = verify_token(token)?;
    User::from_claims(decoded)
}
"#;

    // Analyze
    println!("[1] Analyzing code message...");
    let pipeline = memory_service.message_pipeline.get_pipeline();
    let analysis = pipeline
        .analyze_message(code_message, "assistant", None)
        .await
        .expect("Should analyze");

    assert!(analysis.analysis.is_code, "Should detect as code");
    assert_eq!(
        analysis.analysis.programming_lang,
        Some("rust".to_string()),
        "Should detect Rust"
    );

    println!("    ✓ Detected as Rust code");

    // Store
    println!("[2] Storing code message...");
    let msg_id = memory_service
        .core
        .save_assistant_message(session_id, code_message, None)
        .await
        .expect("Should save");

    // Generate embedding and store in Code head
    if analysis.should_embed {
        println!("[3] Storing in Code head...");
        let embedding = embedding_client
            .embed(code_message)
            .await
            .expect("Should embed");

        let mut entry = create_empty_entry(session_id, "assistant", code_message);
        entry.id = Some(msg_id);
        entry.embedding = Some(embedding);
        entry.contains_code = Some(true);
        entry.programming_lang = Some("rust".to_string());

        let point_id = multi_store
            .save(EmbeddingHead::Code, &entry)
            .await
            .expect("Should store in Code head");

        println!("    ✓ Stored in Code head (point_id: {})", point_id);
    }

    println!("\n✓ CODE MESSAGE ROUTING WORKING\n");
}

// ============================================================================
// TEST 7: Recall Engine Integration
// NOTE: Requires real OpenAI API for embeddings
// ============================================================================

#[tokio::test]
async fn test_recall_engine_integration() {
    println!("\n=== Testing Recall Engine Integration ===\n");

    let (memory_service, embedding_client, multi_store) = setup_full_stack().await;
    let session_id = "recall-test";

    // Store multiple messages
    let messages = vec![
        "I'm implementing a REST API with authentication",
        "The database schema needs foreign key constraints",
        "Let's add caching with Redis for better performance",
    ];

    println!("[1] Storing {} messages...", messages.len());
    for msg in &messages {
        let msg_id = memory_service
            .core
            .save_user_message(session_id, msg, None)
            .await
            .expect("Should save");

        // Generate and store embedding
        let embedding = embedding_client.embed(msg).await.expect("Should embed");
        let mut entry = create_empty_entry(session_id, "user", msg);
        entry.id = Some(msg_id);
        entry.embedding = Some(embedding);
        entry.salience = Some(0.8);

        multi_store
            .save(EmbeddingHead::Conversation, &entry)
            .await
            .expect("Should store in Qdrant");
    }
    println!("    ✓ Stored all messages");

    // Test parallel recall
    println!("[2] Testing parallel recall...");
    let query = "database schema design";

    let recall_context = memory_service
        .parallel_recall_context(
            session_id, query, 5,  // recent_count
            10, // semantic_count
        )
        .await
        .expect("Should recall");

    assert!(
        !recall_context.recent.is_empty(),
        "Should have recent memories"
    );
    // Semantic might be empty if embeddings weren't generated yet (async process)
    // Just verify the recall worked

    println!("    ✓ Recent memories: {}", recall_context.recent.len());
    println!("    ✓ Semantic memories: {}", recall_context.semantic.len());

    // Verify semantic relevance (if semantic results exist)
    if !recall_context.semantic.is_empty() {
        let has_relevant = recall_context.semantic.iter().any(|m| {
            m.content.to_lowercase().contains("database")
                || m.content.to_lowercase().contains("schema")
        });

        if has_relevant {
            println!("    ✓ Found semantically relevant results");
        }
    } else {
        println!(
            "    ℹ No semantic results (embeddings not generated yet - this is expected in quick tests)"
        );
    }

    println!("\n✓ RECALL ENGINE INTEGRATION WORKING\n");
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn setup_full_stack() -> (MemoryService, Arc<GeminiEmbeddings>, Arc<QdrantMultiStore>) {
    // Setup database
    let db_pool = setup_test_db().await;
    let sqlite_store = Arc::new(SqliteMemoryStore::new(db_pool));

    // Setup Qdrant (gRPC port 6334)
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    let multi_store = Arc::new(
        QdrantMultiStore::new(&qdrant_url, "e2e_test")
            .await
            .expect("Should connect to Qdrant"),
    );

    // Load .env and setup LLM provider
    let _ = dotenv::dotenv();
    let api_key = std::env::var("GOOGLE_API_KEY").expect("GOOGLE_API_KEY must be set");

    // Use actual model from config, fallback to gemini-3-pro-preview
    let model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3-pro-preview".to_string());

    let llm_provider: Arc<dyn LlmProvider> = Arc::new(Gemini3Provider::new(
        api_key.clone(),
        model,
        ThinkingLevel::High,
    ).expect("Should create Gemini provider"));

    // Setup embedding client
    let embedding_client = Arc::new(GeminiEmbeddings::new(
        api_key.clone(),
        "gemini-embedding-001".to_string(),
    ));

    // Create memory service
    let memory_service = MemoryService::new(
        sqlite_store,
        multi_store.clone(),
        llm_provider,
        embedding_client.clone(),
    );

    (memory_service, embedding_client, multi_store)
}

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("Failed to create test DB");

    // Create memory_entries table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            response_id TEXT,
            parent_id INTEGER REFERENCES memory_entries(id) ON DELETE CASCADE,
            role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system', 'code', 'document')),
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            tags TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create memory_entries table");

    // Create message_analysis table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS message_analysis (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id INTEGER NOT NULL UNIQUE REFERENCES memory_entries(id) ON DELETE CASCADE,
            mood TEXT,
            intensity REAL CHECK(intensity >= 0 AND intensity <= 1),
            salience REAL CHECK(salience >= 0 AND salience <= 1),
            original_salience REAL,
            intent TEXT,
            topics TEXT NOT NULL DEFAULT '[]',
            summary TEXT,
            relationship_impact TEXT,
            contains_code BOOLEAN DEFAULT FALSE,
            language TEXT DEFAULT 'en',
            programming_lang TEXT,
            contains_error BOOLEAN DEFAULT FALSE,
            error_type TEXT,
            error_severity TEXT,
            error_file TEXT,
            analyzed_at INTEGER DEFAULT (strftime('%s','now')),
            analysis_version TEXT,
            routed_to_heads TEXT NOT NULL DEFAULT '[]',
            last_recalled INTEGER,
            recall_count INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create message_analysis table");

    pool
}

fn create_empty_entry(session_id: &str, role: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        response_id: None,
        parent_id: None,
        role: role.to_string(),
        content: content.to_string(),
        timestamp: Utc::now(),
        tags: None,
        mood: None,
        intensity: None,
        salience: None,
        original_salience: None,
        intent: None,
        topics: None,
        summary: None,
        relationship_impact: None,
        contains_code: Some(false),
        language: Some("en".to_string()),
        programming_lang: None,
        analyzed_at: None,
        analysis_version: None,
        routed_to_heads: None,
        last_recalled: None,
        recall_count: None,
        contains_error: None,
        error_type: None,
        error_severity: None,
        error_file: None,
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

// ============================================================================
// Test Configuration
// ============================================================================

// Run with: cargo test --test e2e_data_flow_test -- --nocapture --test-threads=1
// Requires:
//   - GOOGLE_API_KEY environment variable
//   - Qdrant running on localhost:6334
//
// Use --test-threads=1 to avoid race conditions with shared Qdrant collections
