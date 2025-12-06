// tests/git_operations_test.rs
// Git Operations Integration Tests

use mira_backend::config::CONFIG;
use mira_backend::git::{client::GitClient, store::GitStore, types::GitImportStatus};
mod common;

use mira_backend::llm::provider::OpenAIEmbeddings;
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;

use git2::{Oid, Repository, Signature};
use sqlx::sqlite::SqlitePoolOptions;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

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

async fn setup_code_intelligence(pool: Arc<sqlx::SqlitePool>) -> Arc<CodeIntelligenceService> {
    let multi_store = Arc::new(
        QdrantMultiStore::new(&CONFIG.qdrant_url, "test_git_ops")
            .await
            .unwrap_or_else(|_| panic!("Qdrant not available")),
    );

    let embedding_client = Arc::new(OpenAIEmbeddings::new(
        common::openai_api_key(),
    ));

    Arc::new(CodeIntelligenceService::new(
        (*pool).clone(),
        multi_store,
        embedding_client,
    ))
}

async fn create_test_project(pool: &sqlx::SqlitePool, project_id: &str) {
    let now = chrono::Utc::now().timestamp();
    let path = format!("/tmp/test_project_{}", project_id);
    sqlx::query!(
        "INSERT INTO projects (id, name, path, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        project_id,
        project_id,
        path,
        now,
        now
    )
    .execute(pool)
    .await
    .expect("Failed to create test project");
}

// Helper to create test git repo - returns commit_id
fn create_test_repo(path: &std::path::Path) -> Oid {
    let repo = Repository::init(path).expect("Failed to init repo");

    // Create README
    let readme_path = path.join("README.md");
    fs::write(&readme_path, "# Test Repository\n\nThis is a test.\n")
        .expect("Failed to write README");

    // Create src/main.rs
    let src_dir = path.join("src");
    fs::create_dir_all(&src_dir).expect("Failed to create src dir");
    let main_path = src_dir.join("main.rs");
    fs::write(
        &main_path,
        "fn main() {\n    println!(\"Hello, world!\");\n}\n",
    )
    .expect("Failed to write main.rs");

    // Stage files
    let mut index = repo.index().expect("Failed to get index");
    index
        .add_path(std::path::Path::new("README.md"))
        .expect("Failed to add README");
    index
        .add_path(std::path::Path::new("src/main.rs"))
        .expect("Failed to add main.rs");
    index.write().expect("Failed to write index");

    // Commit
    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let sig = Signature::now("Test User", "test@example.com").expect("Failed to create signature");

    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .expect("Failed to create commit");

    commit_id
}

#[tokio::test]

async fn test_git_attach_and_clone() {
    println!("\n=== Testing Git Attach and Clone ===\n");

    let pool = setup_test_db().await;
    let _code_intel = setup_code_intelligence(pool.clone()).await;

    // Create project first (required for foreign key)
    let project_id = "test-project";
    create_test_project(&pool, project_id).await;

    let git_store = GitStore::new((*pool).clone());
    let git_client = GitClient::new(PathBuf::from("./test_repos"), git_store);

    // Create test repo
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source_path = temp_dir.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source dir");
    let _commit_id = create_test_repo(&source_path);

    println!("[1] Attaching repository");
    let attachment = git_client
        .attach_repo(project_id, &format!("file://{}", source_path.display()))
        .await
        .expect("Failed to attach repo");

    assert_eq!(attachment.project_id, project_id);
    assert_eq!(attachment.import_status, GitImportStatus::Pending);

    println!("[2] Cloning repository");
    git_client
        .clone_repo(&attachment)
        .await
        .expect("Failed to clone repo");

    // Verify status changed to Cloned
    let updated = git_client
        .store
        .get_attachment(&attachment.id)
        .await
        .expect("Failed to get attachment")
        .expect("Attachment not found");

    assert_eq!(updated.import_status, GitImportStatus::Cloned);

    println!("✓ Repository attached and cloned successfully");
}

#[tokio::test]

async fn test_git_import_codebase() {
    println!("\n=== Testing Git Import Codebase ===\n");

    let pool = setup_test_db().await;
    let _code_intel = setup_code_intelligence(pool.clone()).await;

    // Create project first (required for foreign key)
    let project_id = "test-project-import";
    create_test_project(&pool, project_id).await;

    let git_store = GitStore::new((*pool).clone());
    let git_client = GitClient::new(PathBuf::from("./test_repos"), git_store);

    // Create test repo
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source_path = temp_dir.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source dir");
    create_test_repo(&source_path);

    println!("[1] Attaching and cloning repository");
    let attachment = git_client
        .attach_repo(project_id, &format!("file://{}", source_path.display()))
        .await
        .expect("Failed to attach repo");

    git_client
        .clone_repo(&attachment)
        .await
        .expect("Failed to clone repo");

    println!("[2] Importing codebase");
    git_client
        .import_codebase(&attachment)
        .await
        .expect("Failed to import codebase");

    // Verify status changed to Imported
    let updated = git_client
        .store
        .get_attachment(&attachment.id)
        .await
        .expect("Failed to get attachment")
        .expect("Attachment not found");

    assert_eq!(updated.import_status, GitImportStatus::Imported);
    assert!(updated.last_imported_at.is_some());

    println!("✓ Codebase imported successfully");
}

#[tokio::test]

async fn test_git_file_operations() {
    println!("\n=== Testing Git File Operations ===\n");

    let pool = setup_test_db().await;
    let _code_intel = setup_code_intelligence(pool.clone()).await;

    // Create project first (required for foreign key)
    let project_id = "test-project-files";
    create_test_project(&pool, project_id).await;

    let git_store = GitStore::new((*pool).clone());
    let git_client = GitClient::new(PathBuf::from("./test_repos"), git_store);

    // Create test repo
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source_path = temp_dir.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source dir");
    create_test_repo(&source_path);

    println!("[1] Setting up repository");
    let attachment = git_client
        .attach_repo(project_id, &format!("file://{}", source_path.display()))
        .await
        .expect("Failed to attach repo");

    git_client
        .clone_repo(&attachment)
        .await
        .expect("Failed to clone repo");

    println!("[2] Reading file content");
    let content = git_client
        .get_file_content(&attachment, "README.md")
        .expect("Failed to read README");

    assert!(content.contains("Test Repository"));

    let main_content = git_client
        .get_file_content(&attachment, "src/main.rs")
        .expect("Failed to read main.rs");

    assert!(main_content.contains("fn main"));
    assert!(main_content.contains("Hello, world"));

    println!("[3] Getting file tree");
    let tree = git_client
        .get_file_tree(&attachment)
        .expect("Failed to get file tree");

    assert!(!tree.is_empty());
    let has_readme = tree.iter().any(|node| node.name == "README.md");
    let has_src = tree.iter().any(|node| node.name == "src");

    assert!(has_readme, "Should have README.md");
    assert!(has_src, "Should have src directory");

    println!("✓ File operations working correctly");
}
