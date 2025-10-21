// tests/artifact_flow_test.rs
//
// Test the complete artifact creation and streaming flow
// Validates that artifacts are created, stored, hashed, and included in events

use mira_backend::config::CONFIG;
use mira_backend::llm::provider::OpenAiEmbeddings;
use mira_backend::llm::provider::LlmProvider;
use mira_backend::llm::provider::gpt5::Gpt5Provider;
use mira_backend::llm::provider::deepseek::DeepSeekProvider;
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::operations::engine::{OperationEngine, OperationEngineEvent};
use mira_backend::relationship::service::RelationshipService;
use mira_backend::relationship::facts_service::FactsService;

use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tokio::sync::mpsc;
use serde_json::json;

async fn setup_test_engine() -> OperationEngine {
    // Setup in-memory database
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    
    // Create providers
    let gpt5 = Gpt5Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        4000,
        "medium".to_string(),
        "medium".to_string(),
    );
    
    let deepseek = DeepSeekProvider::new("test-key".to_string());
    
    // Setup services
    let sqlite_store = Arc::new(SqliteMemoryStore::new((*db).clone()));
    
    let multi_store = Arc::new(
        QdrantMultiStore::new(&CONFIG.qdrant_url, "test_artifacts")
            .await
            .unwrap_or_else(|_| panic!("Qdrant not available"))
    );
    
    let embedding_client = Arc::new(OpenAiEmbeddings::new(
        "test-key".to_string(),
        "text-embedding-3-large".to_string(),
    ));
    
    let llm_provider: Arc<dyn LlmProvider> = Arc::new(Gpt5Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        4000,
        "medium".to_string(),
        "medium".to_string(),
    ));
    
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store,
        multi_store,
        llm_provider.clone(),
        embedding_client,
    ));
    
    let facts_service = Arc::new(FactsService::new((*db).clone()));
    let relationship_service = Arc::new(RelationshipService::new(
        db.clone(),
        facts_service.clone(),
    ));
    
    OperationEngine::new(
        db.clone(),
        gpt5,
        deepseek,
        memory_service,
        relationship_service,
    )
}

#[tokio::test]
async fn test_artifact_creation_and_retrieval() {
    println!("\n=== Testing Artifact Creation and Retrieval ===\n");
    
    let engine = setup_test_engine().await;
    let (tx, mut rx) = mpsc::channel(100);
    
    // Create and start operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Create a Rust function".to_string(),
        )
        .await
        .expect("Failed to create operation");
    
    println!("✓ Created operation: {}", op.id);
    
    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");
    
    // Drain startup events
    let _ = rx.recv().await; // Started
    let _ = rx.recv().await; // StatusChanged
    
    println!("✓ Operation started");
    
    // Create first artifact
    let code1 = r#"fn hello_world() {
    println!("Hello, world!");
}"#;
    
    let artifact_data = json!({
        "path": "src/main.rs",
        "content": code1,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data, &tx)
        .await
        .expect("Failed to create artifact");
    
    println!("✓ Created first artifact");
    
    // Check for artifact events
    let preview_event = rx.recv().await.expect("No ArtifactPreview event");
    match preview_event {
        OperationEngineEvent::ArtifactPreview {
            operation_id,
            artifact_id,
            path,
            preview,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(path, "src/main.rs");
            assert!(preview.contains("hello_world"));
            println!("✓ Received ArtifactPreview event (artifact_id: {})", artifact_id);
        }
        _ => panic!("Expected ArtifactPreview event, got: {:?}", preview_event),
    }
    
    let completed_event = rx.recv().await.expect("No ArtifactCompleted event");
    match completed_event {
        OperationEngineEvent::ArtifactCompleted {
            operation_id,
            artifact,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(artifact.file_path, Some("src/main.rs".to_string()));
            assert_eq!(artifact.language, Some("rust".to_string()));
            assert_eq!(artifact.content, code1);
            assert!(!artifact.content_hash.is_empty());
            assert!(artifact.diff.is_none()); // First artifact has no diff
            println!("✓ Received ArtifactCompleted event");
            println!("  - Hash: {}", artifact.content_hash);
        }
        _ => panic!("Expected ArtifactCompleted event, got: {:?}", completed_event),
    }
    
    // Create second artifact (should have diff)
    let code2 = r#"fn hello_world() {
    println!("Hello, world!");
    println!("Welcome to Rust!");
}"#;
    
    let artifact_data2 = json!({
        "path": "src/main.rs",
        "content": code2,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data2, &tx)
        .await
        .expect("Failed to create second artifact");
    
    println!("✓ Created second artifact");
    
    // Drain artifact events
    let _ = rx.recv().await; // ArtifactPreview
    
    let completed_event2 = rx.recv().await.expect("No second ArtifactCompleted event");
    match completed_event2 {
        OperationEngineEvent::ArtifactCompleted {
            artifact,
            ..
        } => {
            assert_eq!(artifact.file_path, Some("src/main.rs".to_string()));
            assert!(artifact.diff.is_some()); // Should have diff from first version
            println!("✓ Received second ArtifactCompleted event with diff");
        }
        _ => panic!("Expected ArtifactCompleted event"),
    }
    
    // Complete operation
    engine
        .complete_operation(&op.id, "test-session", Some("Generated code successfully".to_string()), &tx)
        .await
        .expect("Failed to complete operation");
    
    println!("✓ Completed operation");
    
    // Drain status change event
    let _ = rx.recv().await; // StatusChanged
    
    // Check final Completed event includes all artifacts
    let final_event = rx.recv().await.expect("No Completed event");
    match final_event {
        OperationEngineEvent::Completed {
            operation_id,
            result,
            artifacts,
        } => {
            assert_eq!(operation_id, op.id);
            assert!(result.is_some());
            assert_eq!(artifacts.len(), 2); // Should have both artifacts
            println!("✓ Completed event includes {} artifacts", artifacts.len());
            
            // Verify artifacts are in order
            assert_eq!(artifacts[0].kind, "code");
            assert_eq!(artifacts[1].kind, "code");
            println!("✓ Artifacts are in correct order");
        }
        _ => panic!("Expected Completed event, got: {:?}", final_event),
    }
    
    println!("\n✅ Artifact creation and retrieval test passed!\n");
}

#[tokio::test]
async fn test_artifact_hash_and_diff() {
    println!("\n=== Testing Artifact Hashing and Diffing ===\n");
    
    let engine = setup_test_engine().await;
    let (tx, mut rx) = mpsc::channel(100);
    
    // Create operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test hashing".to_string(),
        )
        .await
        .expect("Failed to create operation");
    
    engine.start_operation(&op.id, &tx).await.expect("Failed to start");
    
    // Drain startup events
    let _ = rx.recv().await;
    let _ = rx.recv().await;
    
    // Create first version
    let content1 = "fn test() { println!(\"v1\"); }";
    let artifact_data1 = json!({
        "path": "test.rs",
        "content": content1,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data1, &tx)
        .await
        .expect("Failed to create artifact");
    
    // Get the hash from the event
    let _ = rx.recv().await; // ArtifactPreview
    let event1 = rx.recv().await.expect("No event");
    let hash1 = match event1 {
        OperationEngineEvent::ArtifactCompleted { artifact, .. } => {
            assert!(artifact.diff.is_none(), "First artifact should have no diff");
            artifact.content_hash
        }
        _ => panic!("Expected ArtifactCompleted event"),
    };
    
    println!("✓ First artifact hash: {}", hash1);
    
    // Create second version with different content
    let content2 = "fn test() { println!(\"v2\"); }";
    let artifact_data2 = json!({
        "path": "test.rs",
        "content": content2,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data2, &tx)
        .await
        .expect("Failed to create artifact");
    
    let _ = rx.recv().await; // ArtifactPreview
    let event2 = rx.recv().await.expect("No event");
    match event2 {
        OperationEngineEvent::ArtifactCompleted { artifact, .. } => {
            assert!(artifact.diff.is_some(), "Second artifact should have diff");
            assert_ne!(artifact.content_hash, hash1, "Different content should have different hash");
            println!("✓ Second artifact has different hash: {}", artifact.content_hash);
            println!("✓ Diff present: {} bytes", artifact.diff.as_ref().unwrap().len());
        }
        _ => panic!("Expected ArtifactCompleted event"),
    };
    
    // Create third version with same content as first (should have same hash)
    let artifact_data3 = json!({
        "path": "test.rs",
        "content": content1,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data3, &tx)
        .await
        .expect("Failed to create artifact");
    
    let _ = rx.recv().await; // ArtifactPreview
    let event3 = rx.recv().await.expect("No event");
    match event3 {
        OperationEngineEvent::ArtifactCompleted { artifact, .. } => {
            assert_eq!(artifact.content_hash, hash1, "Same content should have same hash");
            println!("✓ Third artifact hash matches first (content deduplication works)");
        }
        _ => panic!("Expected ArtifactCompleted event"),
    };
    
    println!("\n✅ Artifact hashing and diffing test passed!\n");
}

#[tokio::test]
async fn test_multiple_artifacts_per_operation() {
    println!("\n=== Testing Multiple Artifacts Per Operation ===\n");
    
    let engine = setup_test_engine().await;
    let (tx, mut rx) = mpsc::channel(100);
    
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Generate multiple files".to_string(),
        )
        .await
        .expect("Failed to create operation");
    
    engine.start_operation(&op.id, &tx).await.expect("Failed to start");
    
    // Drain startup events
    let _ = rx.recv().await;
    let _ = rx.recv().await;
    
    // Create multiple different files
    let files = vec![
        ("src/main.rs", "fn main() {}", "rust"),
        ("src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }", "rust"),
        ("README.md", "# My Project", "markdown"),
        ("Cargo.toml", "[package]\nname = \"test\"", "toml"),
    ];
    
    for (path, content, lang) in &files {
        let artifact_data = json!({
            "path": path,
            "content": content,
            "language": lang
        });
        
        engine
            .create_artifact(&op.id, artifact_data, &tx)
            .await
            .expect("Failed to create artifact");
        
        // Drain events for each artifact
        let _ = rx.recv().await; // ArtifactPreview
        let _ = rx.recv().await; // ArtifactCompleted
    }
    
    println!("✓ Created {} artifacts", files.len());
    
    // Complete operation
    engine
        .complete_operation(&op.id, "test-session", Some("Done".to_string()), &tx)
        .await
        .expect("Failed to complete");
    
    let _ = rx.recv().await; // StatusChanged
    
    let final_event = rx.recv().await.expect("No Completed event");
    match final_event {
        OperationEngineEvent::Completed { artifacts, .. } => {
            assert_eq!(artifacts.len(), files.len());
            println!("✓ All {} artifacts included in Completed event", artifacts.len());
            
            // Verify all files are present
            for (expected_path, _, _) in &files {
                assert!(
                    artifacts.iter().any(|a| a.file_path.as_deref() == Some(*expected_path)),
                    "Missing artifact: {}",
                    expected_path
                );
            }
            println!("✓ All expected artifacts present");
        }
        _ => panic!("Expected Completed event"),
    }
    
    println!("\n✅ Multiple artifacts test passed!\n");
}

#[tokio::test]
async fn test_artifact_preview_truncation() {
    println!("\n=== Testing Artifact Preview Truncation ===\n");
    
    let engine = setup_test_engine().await;
    let (tx, mut rx) = mpsc::channel(100);
    
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Generate large file".to_string(),
        )
        .await
        .expect("Failed to create operation");
    
    engine.start_operation(&op.id, &tx).await.expect("Failed to start");
    
    // Drain startup events
    let _ = rx.recv().await;
    let _ = rx.recv().await;
    
    // Create a large artifact (over 200 chars - that's the preview limit in engine.rs)
    let mut large_content = "fn main() {\n".to_string();
    large_content.push_str(&"    println!(\"line\");\n".repeat(50));
    large_content.push_str("}");
    
    let artifact_data = json!({
        "path": "large.rs",
        "content": &large_content,
        "language": "rust"
    });
    
    engine
        .create_artifact(&op.id, artifact_data, &tx)
        .await
        .expect("Failed to create artifact");
    
    let preview_event = rx.recv().await.expect("No preview event");
    match preview_event {
        OperationEngineEvent::ArtifactPreview { preview, .. } => {
            assert!(preview.len() <= 203, "Preview should be truncated to ~200 chars (+ '...')");
            println!("✓ Preview truncated: {} chars (original: {} chars)", preview.len(), large_content.len());
        }
        _ => panic!("Expected ArtifactPreview event"),
    }
    
    let completed_event = rx.recv().await.expect("No completed event");
    match completed_event {
        OperationEngineEvent::ArtifactCompleted { artifact, .. } => {
            assert_eq!(artifact.content.len(), large_content.len());
            println!("✓ Full content preserved in ArtifactCompleted event");
        }
        _ => panic!("Expected ArtifactCompleted event"),
    }
    
    println!("\n✅ Preview truncation test passed!\n");
}
