// tests/git_operations_test.rs
// Comprehensive Git Operations Tests
//
// Tests:
// 1. Repository attachment and cloning
// 2. Pull/push operations
// 3. Branch management (list, switch, create)
// 4. Commit history and diffs
// 5. File tree building from repos
// 6. Import status state machine (Pending → Cloned → Imported)
// 7. Code sync after file changes
// 8. File operations (read, write, restore)

use mira_backend::git::{
    client::GitClient,
    store::GitStore,
    types::{GitRepoAttachment, GitImportStatus},
};
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::llm::provider::OpenAiEmbeddings;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use std::path::PathBuf;
use std::fs;
use git2::Repository;
use tempfile::TempDir;

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

async fn setup_git_client(pool: Arc<sqlx::SqlitePool>, with_code_intel: bool) -> (GitClient, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let git_dir = temp_dir.path().join("repos");
    fs::create_dir_all(&git_dir).expect("Failed to create git dir");
    
    let store = GitStore::new((*pool).clone());
    
    let client = if with_code_intel {
        let embedding_client = Arc::new(OpenAiEmbeddings::new(
            "test-key".to_string(),
            "text-embedding-3-large".to_string(),
        ));
        
        let multi_store = Arc::new(
            QdrantMultiStore::new("http://localhost:6333", "test_git")
                .await
                .expect("Failed to create Qdrant store")
        );
        
        let code_intel = CodeIntelligenceService::new(
            pool.clone(),
            multi_store,
            embedding_client,
        );
        
        GitClient::with_code_intelligence(git_dir, store, code_intel)
    } else {
        GitClient::new(git_dir, store)
    };
    
    (client, temp_dir)
}

/// Create a test git repository with some commits
fn create_test_repo(path: &PathBuf) -> Repository {
    let repo = Repository::init(path).expect("Failed to init repo");
    
    // Configure git
    let mut config = repo.config().expect("Failed to get config");
    config.set_str("user.name", "Test User").expect("Failed to set name");
    config.set_str("user.email", "test@example.com").expect("Failed to set email");
    
    // Create initial commit
    let tree_id = {
        let mut index = repo.index().expect("Failed to get index");
        
        // Add a file
        let file_path = path.join("README.md");
        fs::write(&file_path, "# Test Repository\n").expect("Failed to write file");
        
        index.add_path(std::path::Path::new("README.md")).expect("Failed to add file");
        index.write().expect("Failed to write index");
        index.write_tree().expect("Failed to write tree")
    };
    
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let sig = repo.signature().expect("Failed to create signature");
    
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Initial commit",
        &tree,
        &[]
    ).expect("Failed to create commit");
    
    // Add another commit
    let second_tree_id = {
        let file_path = path.join("src");
        fs::create_dir_all(&file_path).expect("Failed to create src dir");
        
        let code_file = file_path.join("main.rs");
        fs::write(&code_file, "fn main() {\n    println!(\"Hello, world!\");\n}\n")
            .expect("Failed to write code file");
        
        let mut index = repo.index().expect("Failed to get index");
        index.add_path(std::path::Path::new("src/main.rs")).expect("Failed to add file");
        index.write().expect("Failed to write index");
        index.write_tree().expect("Failed to write tree")
    };
    
    let second_tree = repo.find_tree(second_tree_id).expect("Failed to find tree");
    let parent = repo.head().expect("Failed to get HEAD")
        .peel_to_commit().expect("Failed to get parent commit");
    
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Add main.rs",
        &second_tree,
        &[&parent]
    ).expect("Failed to create second commit");
    
    repo
}

// ============================================================================
// TEST 1: Repository Attachment
// ============================================================================

#[tokio::test]
async fn test_repository_attachment() {
    println!("\n=== Testing Repository Attachment ===\n");
    
    let pool = setup_test_db().await;
    let (client, _temp) = setup_git_client(pool.clone(), false).await;
    
    let project_id = "test-project-001";
    let repo_url = "https://github.com/test/repo.git";
    
    println!("[1] Attaching repository to project");
    let attachment = client.attach_repo(project_id, repo_url).await
        .expect("Failed to attach repo");
    
    assert_eq!(attachment.project_id, project_id);
    assert_eq!(attachment.repo_url, repo_url);
    assert_eq!(attachment.import_status, GitImportStatus::Pending);
    assert!(attachment.last_imported_at.is_none());
    
    println!("[2] Retrieving attachment from database");
    let retrieved = client.store.get_attachment(&attachment.id).await
        .expect("Failed to get attachment")
        .expect("Attachment not found");
    
    assert_eq!(retrieved.id, attachment.id);
    assert_eq!(retrieved.project_id, project_id);
    
    println!("[3] Listing project attachments");
    let attachments = client.store.list_project_attachments(project_id).await
        .expect("Failed to list attachments");
    
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, attachment.id);
    
    println!("✓ Repository attachment working correctly");
}

// ============================================================================
// TEST 2: Import Status State Machine
// ============================================================================

#[tokio::test]
async fn test_import_status_transitions() {
    println!("\n=== Testing Import Status State Machine ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    // Create a real test repository
    let source_repo_path = temp.path().join("source");
    fs::create_dir_all(&source_repo_path).expect("Failed to create source dir");
    let _source_repo = create_test_repo(&source_repo_path);
    
    let project_id = "test-project-002";
    
    println!("[1] Initial state: Pending");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_repo_path.display())
    ).await.expect("Failed to attach repo");
    
    assert_eq!(attachment.import_status, GitImportStatus::Pending);
    
    println!("[2] Transition to Cloned");
    client.clone_repo(&attachment).await
        .expect("Failed to clone repo");
    
    let cloned_status = client.store.get_attachment(&attachment.id).await
        .expect("Failed to get attachment")
        .expect("Attachment not found");
    
    assert_eq!(cloned_status.import_status, GitImportStatus::Cloned);
    
    println!("[3] Verify repository was actually cloned");
    let cloned_path = PathBuf::from(&attachment.local_path);
    assert!(cloned_path.exists());
    assert!(cloned_path.join(".git").exists());
    assert!(cloned_path.join("README.md").exists());
    
    println!("[4] Transition to Imported");
    client.import_codebase(&attachment).await
        .expect("Failed to import codebase");
    
    let imported_status = client.store.get_attachment(&attachment.id).await
        .expect("Failed to get attachment")
        .expect("Attachment not found");
    
    assert_eq!(imported_status.import_status, GitImportStatus::Imported);
    assert!(imported_status.last_imported_at.is_some());
    
    println!("✓ Import status state machine working correctly");
}

// ============================================================================
// TEST 3: File Tree Building
// ============================================================================

#[tokio::test]
async fn test_file_tree_building() {
    println!("\n=== Testing File Tree Building ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    // Create test repository with structure
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    create_test_repo(&source_path);
    
    let project_id = "test-project-003";
    
    println!("[1] Cloning repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await
        .expect("Failed to clone");
    
    println!("[2] Building file tree");
    let tree = client.get_file_tree(&attachment)
        .expect("Failed to get file tree");
    
    assert!(!tree.is_empty());
    
    println!("[3] Verifying tree structure");
    let has_readme = tree.iter().any(|node| node.name == "README.md");
    let has_src = tree.iter().any(|node| node.name == "src");
    
    assert!(has_readme, "Should have README.md");
    assert!(has_src, "Should have src directory");
    
    // Find src directory and check it has main.rs
    if let Some(src_node) = tree.iter().find(|n| n.name == "src") {
        assert!(!src_node.children.is_empty(), "src should have children");
        let has_main = src_node.children.iter().any(|c| c.name == "main.rs");
        assert!(has_main, "src should contain main.rs");
    }
    
    println!("✓ File tree building working correctly");
}

// ============================================================================
// TEST 4: File Read/Write Operations
// ============================================================================

#[tokio::test]
async fn test_file_operations() {
    println!("\n=== Testing File Operations ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    create_test_repo(&source_path);
    
    let project_id = "test-project-004";
    
    println!("[1] Setting up repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    
    println!("[2] Reading file content");
    let content = client.get_file_content(&attachment, "README.md")
        .expect("Failed to read README");
    
    assert!(content.contains("Test Repository"));
    
    let main_content = client.get_file_content(&attachment, "src/main.rs")
        .expect("Failed to read main.rs");
    
    assert!(main_content.contains("fn main"));
    assert!(main_content.contains("Hello, world"));
    
    println!("[3] Writing new file content");
    let new_content = "fn main() {\n    println!(\"Modified!\");\n}\n";
    client.update_file_content(&attachment, "src/main.rs", new_content, None)
        .expect("Failed to update file");
    
    let updated = client.get_file_content(&attachment, "src/main.rs")
        .expect("Failed to read updated file");
    
    assert!(updated.contains("Modified!"));
    assert!(!updated.contains("Hello, world"));
    
    println!("[4] Creating new file");
    client.update_file_content(&attachment, "src/lib.rs", "pub fn test() {}\n", None)
        .expect("Failed to create new file");
    
    let lib_content = client.get_file_content(&attachment, "src/lib.rs")
        .expect("Failed to read new file");
    
    assert!(lib_content.contains("pub fn test"));
    
    println!("✓ File operations working correctly");
}

// ============================================================================
// TEST 5: Branch Management
// ============================================================================

#[tokio::test]
async fn test_branch_operations() {
    println!("\n=== Testing Branch Operations ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    let repo = create_test_repo(&source_path);
    
    // Create a feature branch
    let head = repo.head().expect("Failed to get HEAD");
    let commit = head.peel_to_commit().expect("Failed to get commit");
    
    repo.branch("feature-test", &commit, false)
        .expect("Failed to create branch");
    
    let project_id = "test-project-005";
    
    println!("[1] Setting up repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    
    println!("[2] Listing branches");
    let branches = client.get_branches(&attachment)
        .expect("Failed to get branches");
    
    assert!(branches.len() >= 2); // main and feature-test
    
    let branch_names: Vec<_> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(branch_names.contains(&"main") || branch_names.contains(&"master"));
    assert!(branch_names.contains(&"feature-test"));
    
    // Find current branch
    let current = branches.iter().find(|b| b.is_head)
        .expect("Should have current branch");
    
    println!("   Current branch: {}", current.name);
    
    println!("[3] Switching branches");
    client.switch_branch(&attachment, "feature-test")
        .expect("Failed to switch branch");
    
    let updated_branches = client.get_branches(&attachment)
        .expect("Failed to get branches after switch");
    
    let new_current = updated_branches.iter().find(|b| b.is_head)
        .expect("Should have current branch");
    
    assert_eq!(new_current.name, "feature-test");
    
    println!("✓ Branch operations working correctly");
}

// ============================================================================
// TEST 6: Commit History
// ============================================================================

#[tokio::test]
async fn test_commit_history() {
    println!("\n=== Testing Commit History ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    create_test_repo(&source_path);
    
    let project_id = "test-project-006";
    
    println!("[1] Setting up repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    
    println!("[2] Retrieving commit history");
    let commits = client.get_commits(&attachment, 10)
        .expect("Failed to get commits");
    
    assert_eq!(commits.len(), 2); // We created 2 commits
    
    println!("[3] Verifying commit details");
    let latest = &commits[0];
    assert_eq!(latest.message.trim(), "Add main.rs");
    assert_eq!(latest.author_name, "Test User");
    assert_eq!(latest.author_email, "test@example.com");
    assert_eq!(latest.parent_ids.len(), 1); // Second commit has one parent
    
    let initial = &commits[1];
    assert_eq!(initial.message.trim(), "Initial commit");
    assert_eq!(initial.parent_ids.len(), 0); // Initial commit has no parents
    
    println!("[4] Testing commit limit");
    let limited = client.get_commits(&attachment, 1)
        .expect("Failed to get limited commits");
    
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].message.trim(), "Add main.rs");
    
    println!("✓ Commit history working correctly");
}

// ============================================================================
// TEST 7: Diff Operations
// ============================================================================

#[tokio::test]
async fn test_diff_operations() {
    println!("\n=== Testing Diff Operations ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    create_test_repo(&source_path);
    
    let project_id = "test-project-007";
    
    println!("[1] Setting up repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    
    println!("[2] Getting diff for latest commit");
    let commits = client.get_commits(&attachment, 1)
        .expect("Failed to get commits");
    
    let diff = client.get_diff(&attachment, &commits[0].id)
        .expect("Failed to get diff");
    
    assert!(!diff.files_changed.is_empty());
    assert!(diff.additions > 0);
    
    println!("[3] Verifying changed files");
    let changed_files: Vec<_> = diff.files_changed.iter()
        .map(|f| f.path.as_str())
        .collect();
    
    assert!(changed_files.contains(&"src/main.rs"));
    
    println!("✓ Diff operations working correctly");
}

// ============================================================================
// TEST 8: Pull Operations
// ============================================================================

#[tokio::test]
async fn test_pull_operations() {
    println!("\n=== Testing Pull Operations ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    // Create source repo
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    let source_repo = create_test_repo(&source_path);
    
    let project_id = "test-project-008";
    
    println!("[1] Cloning repository");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    
    println!("[2] Making changes in source repository");
    let new_file = source_path.join("CHANGELOG.md");
    fs::write(&new_file, "# Changelog\n\n## v1.0.0\n- Initial release\n")
        .expect("Failed to write changelog");
    
    let mut index = source_repo.index().expect("Failed to get index");
    index.add_path(std::path::Path::new("CHANGELOG.md"))
        .expect("Failed to add changelog");
    index.write().expect("Failed to write index");
    
    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = source_repo.find_tree(tree_id).expect("Failed to find tree");
    let sig = source_repo.signature().expect("Failed to create signature");
    let parent = source_repo.head().expect("Failed to get HEAD")
        .peel_to_commit().expect("Failed to get parent");
    
    source_repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Add changelog",
        &tree,
        &[&parent]
    ).expect("Failed to commit");
    
    println!("[3] Pulling changes to cloned repository");
    client.pull_changes(&attachment).await
        .expect("Failed to pull changes");
    
    println!("[4] Verifying pulled changes");
    let changelog = client.get_file_content(&attachment, "CHANGELOG.md")
        .expect("Failed to read changelog");
    
    assert!(changelog.contains("Changelog"));
    assert!(changelog.contains("v1.0.0"));
    
    let updated_attachment = client.store.get_attachment(&attachment.id).await
        .expect("Failed to get attachment")
        .expect("Attachment not found");
    
    assert!(updated_attachment.last_sync_at.is_some());
    
    println!("✓ Pull operations working correctly");
}

// ============================================================================
// TEST 9: Code Intelligence Integration
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant
async fn test_code_intelligence_integration() {
    println!("\n=== Testing Code Intelligence Integration ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), true).await;
    
    assert!(client.has_code_intelligence());
    
    let source_path = temp.path().join("source");
    fs::create_dir_all(&source_path).expect("Failed to create source");
    create_test_repo(&source_path);
    
    let project_id = "test-project-009";
    
    println!("[1] Importing codebase with code intelligence");
    let attachment = client.attach_repo(
        project_id,
        &format!("file://{}", source_path.display())
    ).await.expect("Failed to attach");
    
    client.clone_repo(&attachment).await.expect("Failed to clone");
    client.import_codebase(&attachment).await
        .expect("Failed to import");
    
    println!("[2] Verifying files were analyzed");
    // Code intelligence should have analyzed main.rs
    // This would be verified by checking repository_files table
    
    let files: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, file_path FROM repository_files WHERE attachment_id = ?"
    )
    .bind(&attachment.id)
    .fetch_all(&*pool)
    .await
    .expect("Failed to query files");
    
    assert!(!files.is_empty(), "Should have analyzed files");
    
    let has_main = files.iter().any(|(_, path)| path.contains("main.rs"));
    assert!(has_main, "Should have analyzed main.rs");
    
    println!("✓ Code intelligence integration working");
}

// ============================================================================
// TEST 10: Error Handling
// ============================================================================

#[tokio::test]
async fn test_error_handling() {
    println!("\n=== Testing Error Handling ===\n");
    
    let pool = setup_test_db().await;
    let (client, _temp) = setup_git_client(pool.clone(), false).await;
    
    println!("[1] Attempting to clone invalid repository");
    let attachment = client.attach_repo("test-project", "https://invalid-url-that-doesnt-exist.com/repo.git")
        .await.expect("Failed to attach");
    
    let clone_result = client.clone_repo(&attachment).await;
    assert!(clone_result.is_err(), "Should fail to clone invalid repo");
    
    println!("[2] Attempting to read non-existent file");
    // Create a valid attachment but try to read non-existent file
    let temp_dir = TempDir::new().expect("Failed to create temp");
    let test_path = temp_dir.path().join("test-repo");
    fs::create_dir_all(&test_path).expect("Failed to create dir");
    create_test_repo(&test_path);
    
    let valid_attachment = client.attach_repo("test-project-2", &format!("file://{}", test_path.display()))
        .await.expect("Failed to attach");
    
    client.clone_repo(&valid_attachment).await.expect("Failed to clone");
    
    let read_result = client.get_file_content(&valid_attachment, "nonexistent.txt");
    assert!(read_result.is_err(), "Should fail to read non-existent file");
    
    println!("[3] Attempting to switch to non-existent branch");
    let switch_result = client.switch_branch(&valid_attachment, "nonexistent-branch");
    assert!(switch_result.is_err(), "Should fail to switch to non-existent branch");
    
    println!("✓ Error handling working correctly");
}

// ============================================================================
// TEST 11: Multiple Attachments per Project
// ============================================================================

#[tokio::test]
async fn test_multiple_attachments() {
    println!("\n=== Testing Multiple Attachments per Project ===\n");
    
    let pool = setup_test_db().await;
    let (client, temp) = setup_git_client(pool.clone(), false).await;
    
    let project_id = "test-project-multi";
    
    // Create two test repositories
    let repo1_path = temp.path().join("repo1");
    let repo2_path = temp.path().join("repo2");
    fs::create_dir_all(&repo1_path).expect("Failed to create repo1");
    fs::create_dir_all(&repo2_path).expect("Failed to create repo2");
    create_test_repo(&repo1_path);
    create_test_repo(&repo2_path);
    
    println!("[1] Attaching first repository");
    let attach1 = client.attach_repo(
        project_id,
        &format!("file://{}", repo1_path.display())
    ).await.expect("Failed to attach repo1");
    
    println!("[2] Attaching second repository");
    let attach2 = client.attach_repo(
        project_id,
        &format!("file://{}", repo2_path.display())
    ).await.expect("Failed to attach repo2");
    
    assert_ne!(attach1.id, attach2.id);
    
    println!("[3] Listing all attachments");
    let attachments = client.store.list_project_attachments(project_id).await
        .expect("Failed to list attachments");
    
    assert_eq!(attachments.len(), 2);
    
    let ids: Vec<_> = attachments.iter().map(|a| &a.id).collect();
    assert!(ids.contains(&&attach1.id));
    assert!(ids.contains(&&attach2.id));
    
    println!("[4] Cloning both repositories");
    client.clone_repo(&attach1).await.expect("Failed to clone repo1");
    client.clone_repo(&attach2).await.expect("Failed to clone repo2");
    
    assert!(PathBuf::from(&attach1.local_path).exists());
    assert!(PathBuf::from(&attach2.local_path).exists());
    
    println!("✓ Multiple attachments working correctly");
}
