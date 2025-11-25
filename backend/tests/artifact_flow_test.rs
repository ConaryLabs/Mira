// tests/artifact_flow_test.rs
// UPDATED: Rewritten to test artifact storage and retrieval through public API
//
// Tests artifact database operations and retrieval, without relying on internal
// engine methods. Validates storage, hashing, and the public get_artifacts API.

mod common;

use mira_backend::config::CONFIG;
use mira_backend::git::client::GitClient;
use mira_backend::git::store::GitStore;
use mira_backend::llm::provider::LlmProvider;
use mira_backend::llm::provider::OpenAiEmbeddings;
use mira_backend::llm::provider::gpt5::{Gpt5Provider, ReasoningEffort};
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::operations::Artifact;
use mira_backend::operations::engine::OperationEngine;
use mira_backend::relationship::facts_service::FactsService;
use mira_backend::relationship::service::RelationshipService;

use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;
use std::sync::Arc;

async fn setup_test_engine() -> (OperationEngine, Arc<sqlx::SqlitePool>) {
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

    // Create provider
    let gpt5 = Gpt5Provider::new(
        common::openai_api_key(),
        "gpt-5.1".to_string(),
        ReasoningEffort::Medium,
    ).expect("Should create GPT5 provider");

    // Setup services
    let sqlite_store = Arc::new(SqliteMemoryStore::new((*db).clone()));

    let multi_store = Arc::new(
        QdrantMultiStore::new(&CONFIG.qdrant_url, "test_artifacts")
            .await
            .unwrap_or_else(|_| panic!("Qdrant not available")),
    );

    let embedding_client = Arc::new(OpenAiEmbeddings::new(
        common::openai_api_key(),
        "text-embedding-3-large".to_string(),
    ));

    let llm_provider: Arc<dyn LlmProvider> = Arc::new(Gpt5Provider::new(
        common::gpt5_api_key(),
        "gpt-5-preview".to_string(),
        ReasoningEffort::Medium,
    ).expect("Should create GPT5 provider"));

    let memory_service = Arc::new(MemoryService::new(
        sqlite_store,
        multi_store.clone(),
        llm_provider.clone(),
        embedding_client.clone(),
    ));

    let facts_service = Arc::new(FactsService::new((*db).clone()));
    let relationship_service =
        Arc::new(RelationshipService::new(db.clone(), facts_service.clone()));

    let git_store = GitStore::new((*db).clone());
    let git_client = GitClient::new(PathBuf::from("./test_repos"), git_store);

    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        (*db).clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        gpt5,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
        None, // sudo_service
    );

    (engine, db)
}

/// Helper to insert test artifact directly into database
async fn insert_test_artifact(
    db: &sqlx::SqlitePool,
    operation_id: &str,
    path: &str,
    content: &str,
    language: &str,
) -> String {
    let artifact = Artifact::new(
        operation_id.to_string(),
        "code".to_string(),
        Some(path.to_string()),
        content.to_string(),
        compute_hash(content),
        Some(language.to_string()),
        None,
    );

    sqlx::query!(
        r#"
        INSERT INTO artifacts (
            id, operation_id, kind, file_path, content, content_hash,
            language, diff_from_previous, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        artifact.id,
        artifact.operation_id,
        artifact.kind,
        artifact.file_path,
        artifact.content,
        artifact.content_hash,
        artifact.language,
        artifact.diff,
        artifact.created_at,
    )
    .execute(db)
    .await
    .expect("Failed to insert test artifact");

    artifact.id
}

fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_artifact_retrieval() {
    println!("\n=== Testing Artifact Retrieval ===\n");

    let (engine, db) = setup_test_engine().await;

    // Create an operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test operation".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("+ Created operation: {}", op.id);

    // Insert test artifacts directly into database
    let code1 = r#"fn hello_world() {
    println!("Hello, world!");
}"#;

    let artifact_id1 = insert_test_artifact(&db, &op.id, "src/main.rs", code1, "rust").await;

    println!("+ Inserted first artifact: {}", artifact_id1);

    let code2 = "pub fn add(a: i32, b: i32) -> i32 { a + b }";

    let artifact_id2 = insert_test_artifact(&db, &op.id, "src/lib.rs", code2, "rust").await;

    println!("+ Inserted second artifact: {}", artifact_id2);

    // Retrieve artifacts using public API
    let artifacts = engine
        .get_artifacts_for_operation(&op.id)
        .await
        .expect("Failed to get artifacts");

    assert_eq!(artifacts.len(), 2, "Should have 2 artifacts");
    println!("+ Retrieved {} artifacts", artifacts.len());

    // Verify first artifact
    assert_eq!(artifacts[0].file_path, Some("src/main.rs".to_string()));
    assert_eq!(artifacts[0].content, code1);
    assert_eq!(artifacts[0].language, Some("rust".to_string()));
    assert!(!artifacts[0].content_hash.is_empty());
    println!("+ First artifact verified");

    // Verify second artifact
    assert_eq!(artifacts[1].file_path, Some("src/lib.rs".to_string()));
    assert_eq!(artifacts[1].content, code2);
    assert_eq!(artifacts[1].language, Some("rust".to_string()));
    assert!(!artifacts[1].content_hash.is_empty());
    println!("+ Second artifact verified");

    println!("\n[PASS] Artifact retrieval test passed!\n");
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_artifact_hash_consistency() {
    println!("\n=== Testing Artifact Hash Consistency ===\n");

    let (engine, db) = setup_test_engine().await;

    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test hashing".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Create artifacts with same content
    let content = "fn test() { println!(\"hello\"); }";

    let artifact_id1 = insert_test_artifact(&db, &op.id, "test1.rs", content, "rust").await;
    let artifact_id2 = insert_test_artifact(&db, &op.id, "test2.rs", content, "rust").await;

    println!("+ Inserted two artifacts with identical content");

    // Retrieve and verify hashes match
    let artifacts = engine
        .get_artifacts_for_operation(&op.id)
        .await
        .expect("Failed to get artifacts");

    assert_eq!(artifacts.len(), 2);
    assert_eq!(
        artifacts[0].content_hash, artifacts[1].content_hash,
        "Identical content should produce identical hashes"
    );

    println!("+ Hashes match: {}", artifacts[0].content_hash);

    // Verify hash is deterministic
    let expected_hash = compute_hash(content);
    assert_eq!(artifacts[0].content_hash, expected_hash);
    println!("+ Hash is deterministic");

    println!("\n[PASS] Hash consistency test passed!\n");
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_multiple_operations_artifact_isolation() {
    println!("\n=== Testing Artifact Isolation Across Operations ===\n");

    let (engine, db) = setup_test_engine().await;

    // Create two operations
    let op1 = engine
        .create_operation(
            "session-1".to_string(),
            "code_generation".to_string(),
            "Operation 1".to_string(),
        )
        .await
        .expect("Failed to create op1");

    let op2 = engine
        .create_operation(
            "session-2".to_string(),
            "code_generation".to_string(),
            "Operation 2".to_string(),
        )
        .await
        .expect("Failed to create op2");

    println!("+ Created two operations");

    // Add artifacts to op1
    insert_test_artifact(&db, &op1.id, "op1_file1.rs", "fn op1_func1() {}", "rust").await;
    insert_test_artifact(&db, &op1.id, "op1_file2.rs", "fn op1_func2() {}", "rust").await;

    // Add artifacts to op2
    insert_test_artifact(&db, &op2.id, "op2_file1.rs", "fn op2_func1() {}", "rust").await;
    insert_test_artifact(&db, &op2.id, "op2_file2.rs", "fn op2_func2() {}", "rust").await;
    insert_test_artifact(&db, &op2.id, "op2_file3.rs", "fn op2_func3() {}", "rust").await;

    println!("+ Added artifacts to both operations");

    // Verify op1 artifacts
    let op1_artifacts = engine
        .get_artifacts_for_operation(&op1.id)
        .await
        .expect("Failed to get op1 artifacts");

    assert_eq!(op1_artifacts.len(), 2);
    assert!(op1_artifacts.iter().all(|a| a.operation_id == op1.id));
    println!(
        "+ Operation 1 has {} isolated artifacts",
        op1_artifacts.len()
    );

    // Verify op2 artifacts
    let op2_artifacts = engine
        .get_artifacts_for_operation(&op2.id)
        .await
        .expect("Failed to get op2 artifacts");

    assert_eq!(op2_artifacts.len(), 3);
    assert!(op2_artifacts.iter().all(|a| a.operation_id == op2.id));
    println!(
        "+ Operation 2 has {} isolated artifacts",
        op2_artifacts.len()
    );

    println!("\n[PASS] Artifact isolation test passed!\n");
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_artifact_different_languages() {
    println!("\n=== Testing Artifacts with Different Languages ===\n");

    let (engine, db) = setup_test_engine().await;

    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Multi-language project".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Create artifacts in different languages
    let files = vec![
        ("src/main.rs", "fn main() {}", "rust"),
        ("index.ts", "console.log('hello');", "typescript"),
        ("app.py", "print('hello')", "python"),
        ("README.md", "# Project", "markdown"),
        ("config.json", r#"{"key": "value"}"#, "json"),
    ];

    for (path, content, lang) in &files {
        insert_test_artifact(&db, &op.id, path, content, lang).await;
    }

    println!(
        "+ Inserted {} artifacts in different languages",
        files.len()
    );

    // Retrieve and verify
    let artifacts = engine
        .get_artifacts_for_operation(&op.id)
        .await
        .expect("Failed to get artifacts");

    assert_eq!(artifacts.len(), files.len());

    // Verify each language is preserved
    for (expected_path, _, expected_lang) in &files {
        let artifact = artifacts
            .iter()
            .find(|a| a.file_path.as_deref() == Some(*expected_path))
            .expect(&format!("Missing artifact: {}", expected_path));

        assert_eq!(artifact.language.as_deref(), Some(*expected_lang));
        println!("+ Verified {}: {}", expected_path, expected_lang);
    }

    println!("\n[PASS] Multi-language artifacts test passed!\n");
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_empty_operation_no_artifacts() {
    println!("\n=== Testing Operation with No Artifacts ===\n");

    let (engine, _db) = setup_test_engine().await;

    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Empty operation".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("+ Created operation: {}", op.id);

    // Retrieve artifacts (should be empty)
    let artifacts = engine
        .get_artifacts_for_operation(&op.id)
        .await
        .expect("Failed to get artifacts");

    assert_eq!(artifacts.len(), 0, "Should have no artifacts");
    println!("+ Operation has no artifacts as expected");

    println!("\n[PASS] Empty operation test passed!\n");
}

#[tokio::test]
#[ignore = "requires Qdrant"]
async fn test_artifact_content_preservation() {
    println!("\n=== Testing Artifact Content Preservation ===\n");

    let (engine, db) = setup_test_engine().await;

    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test content preservation".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Create artifact with special characters and formatting
    // Use r###"..."### to avoid conflict with inner r#"..."#
    let complex_content = r###"
// Special characters: <>&"'
fn test() {
    let json = r#"{"key": "value"}"#;
    let unicode = "Hello world";
    println!("Line 1");
    println!("Line 2");
}
"###;

    insert_test_artifact(&db, &op.id, "complex_file.rs", complex_content, "rust").await;
    println!("+ Inserted artifact with special characters");

    // Retrieve and verify content is exactly preserved
    let artifacts = engine
        .get_artifacts_for_operation(&op.id)
        .await
        .expect("Failed to get artifacts");

    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].content, complex_content,
        "Content should be exactly preserved"
    );
    println!("+ Special characters and formatting preserved");

    println!("\n[PASS] Content preservation test passed!\n");
}
