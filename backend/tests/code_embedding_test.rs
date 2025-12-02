// tests/code_embedding_test.rs
// Tests for code element embedding and semantic search

use anyhow::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;

use mira_backend::llm::embeddings::EmbeddingHead;
use mira_backend::llm::provider::GeminiEmbeddings;
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;

fn init_env() {
    let _ = dotenv::dotenv();
}

const TEST_RUST_CODE: &str = r#"
pub fn authenticate_user(token: &str) -> Result<User, AuthError> {
    let decoded = verify_token(token)?;
    User::from_claims(decoded)
}

pub struct AuthConfig {
    pub secret_key: String,
    pub token_expiry: u64,
}

impl AuthConfig {
    pub fn new(secret: String) -> Self {
        Self {
            secret_key: secret,
            token_expiry: 3600,
        }
    }
}
"#;

const TEST_RUST_CODE_MODIFIED: &str = r#"
pub async fn authenticate_user(token: &str) -> Result<User, AuthError> {
    let decoded = verify_token_async(token).await?;
    User::from_claims_async(decoded).await
}

pub struct AuthConfig {
    pub secret_key: String,
    pub token_expiry: u64,
    pub refresh_enabled: bool,
}

impl AuthConfig {
    pub fn new(secret: String) -> Self {
        Self {
            secret_key: secret,
            token_expiry: 7200,
            refresh_enabled: true,
        }
    }
}
"#;

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:").await.unwrap();

    // Create minimal schema
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS language_configs (
            language TEXT PRIMARY KEY,
            file_extensions TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO language_configs (language, file_extensions) VALUES ('rust', '.rs')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS git_repo_attachments (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            repo_url TEXT,
            local_path TEXT,
            import_status TEXT DEFAULT 'complete',
            last_synced INTEGER
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS repository_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            attachment_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            language TEXT NOT NULL,
            last_indexed INTEGER NOT NULL,
            ast_analyzed BOOLEAN DEFAULT FALSE,
            element_count INTEGER DEFAULT 0,
            complexity_score INTEGER DEFAULT 0,
            last_analyzed INTEGER,
            line_count INTEGER,
            UNIQUE(attachment_id, file_path)
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    // Match production schema from migrations/20251125000002_code_intelligence.sql
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS code_elements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT,
            file_id INTEGER,
            file_path TEXT,
            language TEXT,
            name TEXT NOT NULL,
            full_path TEXT,
            element_type TEXT NOT NULL,
            visibility TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            line_start INTEGER,
            line_end INTEGER,
            content TEXT,
            signature TEXT,
            content_hash TEXT,
            signature_hash TEXT,
            parent_id INTEGER,
            complexity_score REAL,
            is_test BOOLEAN DEFAULT FALSE,
            is_async BOOLEAN DEFAULT FALSE,
            documentation TEXT,
            metadata TEXT,
            analyzed_at INTEGER,
            created_at INTEGER,
            updated_at INTEGER,
            UNIQUE(file_id, name, start_line)
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS code_quality_issues (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            element_id INTEGER NOT NULL REFERENCES code_elements(id) ON DELETE CASCADE,
            issue_type TEXT NOT NULL,
            severity TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            suggested_fix TEXT,
            fix_confidence REAL DEFAULT 0.0,
            is_auto_fixable BOOLEAN DEFAULT FALSE,
            detected_at INTEGER
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS external_dependencies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            element_id INTEGER NOT NULL REFERENCES code_elements(id) ON DELETE CASCADE,
            import_path TEXT NOT NULL,
            imported_symbols TEXT,
            dependency_type TEXT NOT NULL,
            created_at INTEGER
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    pool
}

fn setup_embedding_client() -> Arc<GeminiEmbeddings> {
    init_env();
    let api_key = std::env::var("GOOGLE_API_KEY").unwrap_or_else(|_| "test-key".to_string());
    Arc::new(GeminiEmbeddings::new(
        api_key,
        "gemini-embedding-001".to_string(),
    ))
}

async fn setup_qdrant() -> Arc<QdrantMultiStore> {
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_code_embedding")
            .await
            .expect("Failed to connect to Qdrant"),
    )
}

// ============================================================================
// TEST 1: Parse and Embed Code Elements
// NOTE: Requires real Gemini API + Qdrant
// ============================================================================

#[tokio::test]
#[ignore = "integration test - requires Gemini API + Qdrant"]
async fn test_parse_and_embed_code_elements() -> Result<()> {
    println!("\n=== Testing Code Element Parsing and Embedding ===\n");

    let pool = setup_test_db().await;
    let embedding_client = setup_embedding_client();
    let multi_store = setup_qdrant().await;

    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        pool.clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    // Step 1: Create test attachment and file
    let attachment_id = "test-attachment-1";
    let project_id = "test-project";

    sqlx::query(
        "INSERT INTO git_repo_attachments (id, project_id, repo_url, local_path, last_synced) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(attachment_id)
    .bind(project_id)
    .bind("https://test.git")
    .bind("/tmp/test")
    .bind(chrono::Utc::now().timestamp())
    .execute(&pool)
    .await?;

    let file_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed) VALUES (?, ?, ?, ?, ?) RETURNING id"
    )
    .bind(attachment_id)
    .bind("src/auth.rs")
    .bind("hash123")
    .bind("rust")
    .bind(chrono::Utc::now().timestamp())
    .fetch_one(&pool)
    .await?;

    println!("[1] Created test file with id: {}", file_id);

    // Step 2: Parse the code
    let result = code_intelligence
        .analyze_and_store_with_project(file_id, TEST_RUST_CODE, "src/auth.rs", "rust", project_id)
        .await?;

    println!("[2] Parsed code: {} elements found", result.elements_count);
    assert!(
        result.elements_count >= 2,
        "Should find at least 2 elements (function, struct)"
    );

    // Debug: Test embedding client directly
    let test_embed = embedding_client.embed("test embedding").await;
    match &test_embed {
        Ok(v) => println!("[2a] Embedding client works, vector dim: {}", v.len()),
        Err(e) => println!("[2a] Embedding client FAILED: {}", e),
    }

    // Debug: Check what's in code_elements table
    let rows = sqlx::query("SELECT id, name, element_type, content FROM code_elements WHERE file_id = ?")
        .bind(file_id)
        .fetch_all(&pool)
        .await?;
    println!("[2b] DB has {} code_elements for file_id {}", rows.len(), file_id);
    for row in &rows {
        let id: i64 = row.get("id");
        let name: String = row.get("name");
        let element_type: String = row.get("element_type");
        let content: Option<String> = row.try_get("content").ok();
        println!("[2c] Element {}: {} '{}' content_len={}", id, element_type, name, content.as_ref().map(|c| c.len()).unwrap_or(0));
    }

    // Debug: Test Qdrant save directly
    let test_embedding = embedding_client.embed("test code element").await?;
    println!("[2d] test_embedding len: {}", test_embedding.len());
    let test_entry = mira_backend::memory::core::types::MemoryEntry {
        id: Some(999),
        session_id: "code:test-project".to_string(),
        role: "code".to_string(),
        content: "test content".to_string(),
        timestamp: Utc::now(),
        embedding: Some(test_embedding),
        contains_code: Some(true),
        programming_lang: Some("rust".to_string()),
        tags: Some(vec!["test".to_string()]),
        response_id: None,
        parent_id: None,
        mood: None,
        intensity: None,
        salience: None,
        original_salience: None,
        intent: None,
        topics: None,
        summary: None,
        relationship_impact: None,
        language: None,
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
        embedding_heads: None,
        qdrant_point_ids: None,
    };
    match multi_store.save(EmbeddingHead::Code, &test_entry).await {
        Ok(point_id) => println!("[2d] Qdrant save OK, point_id: {}", point_id),
        Err(e) => println!("[2d] Qdrant save FAILED: {:?}", e),
    }

    // Step 3: Embed the code elements
    let embedded_count = code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    println!("[3] Embedded {} code elements", embedded_count);
    assert_eq!(
        embedded_count, result.elements_count,
        "Should embed all parsed elements"
    );

    // Step 4: Verify elements are in database
    let db_elements =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM code_elements WHERE file_id = ?")
            .bind(file_id)
            .fetch_one(&pool)
            .await?;

    println!("[4] Database contains {} elements", db_elements);
    assert_eq!(db_elements as usize, result.elements_count);

    println!("\n✓ Parse and embed test passed\n");
    Ok(())
}

// ============================================================================
// TEST 2: Semantic Search for Code Elements
// NOTE: Requires real OpenAI API + Qdrant, complex integration test
// ============================================================================

#[tokio::test]
#[ignore = "integration test - requires Gemini API + Qdrant"]
async fn test_semantic_search_code_elements() -> Result<()> {
    println!("\n=== Testing Semantic Search for Code Elements ===\n");

    let pool = setup_test_db().await;
    let embedding_client = setup_embedding_client();
    let multi_store = setup_qdrant().await;

    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        pool.clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    // Setup: Create and embed code
    let attachment_id = "test-attachment-2";
    let project_id = "test-project";

    sqlx::query(
        "INSERT INTO git_repo_attachments (id, project_id, repo_url, local_path, last_synced) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(attachment_id)
    .bind(project_id)
    .bind("https://test.git")
    .bind("/tmp/test")
    .bind(chrono::Utc::now().timestamp())
    .execute(&pool)
    .await?;

    let file_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed) VALUES (?, ?, ?, ?, ?) RETURNING id"
    )
    .bind(attachment_id)
    .bind("src/auth.rs")
    .bind("hash456")
    .bind("rust")
    .bind(chrono::Utc::now().timestamp())
    .fetch_one(&pool)
    .await?;

    code_intelligence
        .analyze_and_store_with_project(file_id, TEST_RUST_CODE, "src/auth.rs", "rust", project_id)
        .await?;

    // Debug: Check how many elements were stored
    let element_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM code_elements WHERE file_id = ?")
        .bind(file_id)
        .fetch_one(&pool)
        .await?;
    println!("[0a] Stored {} code elements in DB for file_id {}", element_count, file_id);

    let embed_count = code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    println!("[1a] Embedded {} code elements", embed_count);

    // Wait for Qdrant to index (2 seconds for safety)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    println!("[1] Setup complete - code embedded");

    // Step 2: Search for "authentication"
    let query = "user authentication and token verification";
    let query_embedding = embedding_client.embed(query).await?;

    let session_filter = format!("code:{}", project_id);
    println!("[1b] Searching with session_id filter: '{}'", session_filter);

    let results = multi_store
        .search(
            EmbeddingHead::Code,
            &session_filter,
            &query_embedding,
            5,
        )
        .await?;

    println!(
        "[2] Search for 'authentication' returned {} results",
        results.len()
    );

    // Debug: Also try search with session_id using search_all
    let all_results = multi_store.search_all(&session_filter, &query_embedding, 10).await?;
    println!("[2a] search_all with '{}' returned {} head groups", session_filter, all_results.len());
    for (head, entries) in &all_results {
        println!("[2b] search_all {} head: {} results", head.as_str(), entries.len());
    }

    // Debug: Check collection info
    println!("[2c] multi_store enabled heads: {:?}", multi_store.get_enabled_heads());
    assert!(
        !results.is_empty(),
        "Should find code elements related to authentication"
    );

    // Verify results contain authentication function
    let has_auth_function = results
        .iter()
        .any(|r| r.content.contains("authenticate_user"));

    println!(
        "[3] Results contain authenticate_user function: {}",
        has_auth_function
    );
    assert!(has_auth_function, "Should find authenticate_user function");

    println!("\n✓ Semantic search test passed\n");
    Ok(())
}

// ============================================================================
// TEST 3: Invalidation on File Change
// NOTE: Requires real OpenAI API + Qdrant, complex integration test
// ============================================================================

#[tokio::test]
#[ignore = "integration test - requires Gemini API + Qdrant"]
async fn test_invalidation_on_file_change() -> Result<()> {
    println!("\n=== Testing Invalidation on File Change ===\n");

    let pool = setup_test_db().await;
    let embedding_client = setup_embedding_client();
    let multi_store = setup_qdrant().await;

    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        pool.clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    // Setup: Create and embed original code
    let attachment_id = "test-attachment-3";
    let project_id = "test-project";

    sqlx::query(
        "INSERT INTO git_repo_attachments (id, project_id, repo_url, local_path, last_synced) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(attachment_id)
    .bind(project_id)
    .bind("https://test.git")
    .bind("/tmp/test")
    .bind(chrono::Utc::now().timestamp())
    .execute(&pool)
    .await?;

    let file_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed) VALUES (?, ?, ?, ?, ?) RETURNING id"
    )
    .bind(attachment_id)
    .bind("src/auth.rs")
    .bind("hash789")
    .bind("rust")
    .bind(chrono::Utc::now().timestamp())
    .fetch_one(&pool)
    .await?;

    // Parse and embed original code
    code_intelligence
        .analyze_and_store_with_project(file_id, TEST_RUST_CODE, "src/auth.rs", "rust", project_id)
        .await?;

    let original_count = code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    println!("[1] Original code: {} elements embedded", original_count);

    // Step 2: Invalidate old embeddings
    let invalidated = code_intelligence.invalidate_file(file_id).await?;
    println!("[2] Invalidated {} embeddings", invalidated);
    assert_eq!(
        invalidated, original_count as u64,
        "Should invalidate all embeddings"
    );

    // Step 3: Delete old elements and re-parse with modified code
    sqlx::query("DELETE FROM code_elements WHERE file_id = ?")
        .bind(file_id)
        .execute(&pool)
        .await?;

    code_intelligence
        .analyze_and_store_with_project(
            file_id,
            TEST_RUST_CODE_MODIFIED,
            "src/auth.rs",
            "rust",
            project_id,
        )
        .await?;

    let new_count = code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    // Wait for Qdrant to index (2 seconds for safety)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    println!("[3] Modified code: {} elements embedded", new_count);

    // Step 4: Verify search returns updated code
    let query = "async authentication";
    let query_embedding = embedding_client.embed(query).await?;

    let results = multi_store
        .search(
            EmbeddingHead::Code,
            &format!("code:{}", project_id),
            &query_embedding,
            5,
        )
        .await?;

    let has_async = results
        .iter()
        .any(|r| r.content.contains("async fn authenticate_user"));

    println!("[4] Search finds async version: {}", has_async);
    assert!(has_async, "Should find updated async function");

    println!("\n✓ Invalidation test passed\n");
    Ok(())
}

// ============================================================================
// TEST 4: Struct and Function Search
// NOTE: Requires real OpenAI API + Qdrant, complex integration test
// ============================================================================

#[tokio::test]
#[ignore = "integration test - requires Gemini API + Qdrant"]
async fn test_search_different_element_types() -> Result<()> {
    println!("\n=== Testing Search for Structs and Functions ===\n");

    let pool = setup_test_db().await;
    let embedding_client = setup_embedding_client();
    let multi_store = setup_qdrant().await;

    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        pool.clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    // Setup
    let attachment_id = "test-attachment-4";
    let project_id = "test-project";

    sqlx::query(
        "INSERT INTO git_repo_attachments (id, project_id, repo_url, local_path, last_synced) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(attachment_id)
    .bind(project_id)
    .bind("https://test.git")
    .bind("/tmp/test")
    .bind(chrono::Utc::now().timestamp())
    .execute(&pool)
    .await?;

    let file_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed) VALUES (?, ?, ?, ?, ?) RETURNING id"
    )
    .bind(attachment_id)
    .bind("src/auth.rs")
    .bind("hash999")
    .bind("rust")
    .bind(chrono::Utc::now().timestamp())
    .fetch_one(&pool)
    .await?;

    code_intelligence
        .analyze_and_store_with_project(file_id, TEST_RUST_CODE, "src/auth.rs", "rust", project_id)
        .await?;

    code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    // Wait for Qdrant to index (2 seconds for safety)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    println!("[1] Code embedded");

    // Search for struct
    let struct_query = embedding_client
        .embed("configuration structure for authentication")
        .await?;
    let struct_results = multi_store
        .search(
            EmbeddingHead::Code,
            &format!("code:{}", project_id),
            &struct_query,
            5,
        )
        .await?;

    let has_struct = struct_results
        .iter()
        .any(|r| r.content.contains("struct AuthConfig"));
    println!("[2] Found AuthConfig struct: {}", has_struct);
    assert!(has_struct, "Should find struct");

    // Search for function
    let fn_query = embedding_client
        .embed("user authentication token verification")
        .await?;
    let fn_results = multi_store
        .search(
            EmbeddingHead::Code,
            &format!("code:{}", project_id),
            &fn_query,
            5,
        )
        .await?;

    let has_function = fn_results
        .iter()
        .any(|r| r.content.contains("fn authenticate_user"));
    println!("[3] Found authenticate_user function: {}", has_function);
    assert!(has_function, "Should find function");

    println!("\n✓ Struct and function search test passed\n");
    Ok(())
}
