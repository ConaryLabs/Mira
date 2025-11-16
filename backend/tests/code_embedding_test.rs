// tests/code_embedding_test.rs
// Tests for code element embedding and semantic search

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;

use mira_backend::llm::embeddings::EmbeddingHead;
use mira_backend::llm::provider::OpenAiEmbeddings;
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;

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

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS code_elements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id INTEGER NOT NULL,
            language TEXT NOT NULL,
            element_type TEXT NOT NULL,
            name TEXT NOT NULL,
            full_path TEXT NOT NULL,
            visibility TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            content TEXT NOT NULL,
            signature_hash TEXT,
            complexity_score INTEGER DEFAULT 0,
            is_test BOOLEAN DEFAULT FALSE,
            is_async BOOLEAN DEFAULT FALSE,
            documentation TEXT,
            metadata TEXT,
            created_at INTEGER,
            analyzed_at INTEGER,
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

fn setup_embedding_client() -> Arc<OpenAiEmbeddings> {
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
    Arc::new(OpenAiEmbeddings::new(
        api_key,
        "text-embedding-3-large".to_string(),
    ))
}

async fn setup_qdrant() -> Arc<QdrantMultiStore> {
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_code_embedding")
            .await
            .expect("Failed to connect to Qdrant"),
    )
}

// ============================================================================
// TEST 1: Parse and Embed Code Elements
// ============================================================================

#[tokio::test]
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
// ============================================================================

#[tokio::test]
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

    code_intelligence
        .embed_code_elements(file_id, project_id)
        .await?;

    println!("[1] Setup complete - code embedded");

    // Step 2: Search for "authentication"
    let query = "user authentication and token verification";
    let query_embedding = embedding_client.embed(query).await?;

    let results = multi_store
        .search(
            EmbeddingHead::Code,
            &format!("code:{}", project_id),
            &query_embedding,
            5,
        )
        .await?;

    println!(
        "[2] Search for 'authentication' returned {} results",
        results.len()
    );
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
// ============================================================================

#[tokio::test]
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
// ============================================================================

#[tokio::test]
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
