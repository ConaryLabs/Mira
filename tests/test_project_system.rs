// tests/test_project_system.rs

use mira_backend::project::store::ProjectStore;
use mira_backend::project::types::ArtifactType;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Helper to create a test database with migrations
async fn create_test_db() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("Failed to create in-memory database");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");
    
    pool
}

#[tokio::test]
async fn test_project_crud_operations() {
    let pool = create_test_db().await;
    let store = ProjectStore::new(pool);
    
    // Test 1: Create a project
    println!("📁 Testing project creation...");
    let project = store.create_project(
        "Test Project".to_string(),
        Some("A test project for integration testing".to_string()),
        Some(vec!["test".to_string(), "integration".to_string()]),
        Some("test-user".to_string()),
    )
    .await
    .expect("Failed to create project");
    
    assert_eq!(project.name, "Test Project");
    assert_eq!(project.tags, Some(vec!["test".to_string(), "integration".to_string()]));
    println!("✅ Project created with ID: {}", project.id);
    
    // Test 2: Get project by ID
    println!("\n🔍 Testing project retrieval...");
    let retrieved = store.get_project(&project.id)
        .await
        .expect("Failed to get project")
        .expect("Project not found");
    
    assert_eq!(retrieved.id, project.id);
    assert_eq!(retrieved.name, project.name);
    println!("✅ Project retrieved successfully");
    
    // Test 3: Update project
    println!("\n✏️ Testing project update...");
    let updated = store.update_project(
        &project.id,
        Some("Updated Test Project".to_string()),
        Some("Updated description".to_string()),
        Some(vec!["updated".to_string(), "test".to_string()]),
    )
    .await
    .expect("Failed to update project")
    .expect("Project not found for update");
    
    assert_eq!(updated.name, "Updated Test Project");
    assert_eq!(updated.description, Some("Updated description".to_string()));
    assert!(updated.updated_at > project.updated_at);
    println!("✅ Project updated successfully");
    
    // Test 4: List projects
    println!("\n📋 Testing project listing...");
    let projects = store.list_projects()
        .await
        .expect("Failed to list projects");
    
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project.id);
    println!("✅ Found {} project(s)", projects.len());
    
    // Test 5: Delete project
    println!("\n🗑️ Testing project deletion...");
    let deleted = store.delete_project(&project.id)
        .await
        .expect("Failed to delete project");
    
    assert!(deleted);
    
    // Verify deletion
    let not_found = store.get_project(&project.id)
        .await
        .expect("Failed to check deleted project");
    
    assert!(not_found.is_none());
    println!("✅ Project deleted successfully");
}

#[tokio::test]
async fn test_artifact_crud_operations() {
    let pool = create_test_db().await;
    let store = ProjectStore::new(pool);
    
    // First create a project to attach artifacts to
    let project = store.create_project(
        "Artifact Test Project".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create project");
    
    println!("📄 Testing artifact creation...");
    
    // Test 1: Create code artifact
    let code_artifact = store.create_artifact(
        project.id.clone(),
        "main.rs".to_string(),
        ArtifactType::Code,
        Some("fn main() { println!(\"Hello, world!\"); }".to_string()),
    )
    .await
    .expect("Failed to create code artifact");
    
    assert_eq!(code_artifact.name, "main.rs");
    assert_eq!(code_artifact.artifact_type, ArtifactType::Code);
    assert_eq!(code_artifact.version, 1);
    println!("✅ Code artifact created");
    
    // Test 2: Create markdown artifact
    let md_artifact = store.create_artifact(
        project.id.clone(),
        "README.md".to_string(),
        ArtifactType::Markdown,
        Some("# Test Project\n\nThis is a test.".to_string()),
    )
    .await
    .expect("Failed to create markdown artifact");
    
    println!("✅ Markdown artifact created");
    
    // Test 3: List project artifacts
    println!("\n📋 Testing artifact listing...");
    let artifacts = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to list artifacts");
    
    assert_eq!(artifacts.len(), 2);
    println!("✅ Found {} artifacts", artifacts.len());
    
    // Test 4: Update artifact
    println!("\n✏️ Testing artifact update...");
    let updated_artifact = store.update_artifact(
        &code_artifact.id,
        Some("main_updated.rs".to_string()),
        Some("fn main() { println!(\"Hello, updated world!\"); }".to_string()),
    )
    .await
    .expect("Failed to update artifact")
    .expect("Artifact not found");
    
    assert_eq!(updated_artifact.name, "main_updated.rs");
    assert_eq!(updated_artifact.version, 2); // Version should increment
    println!("✅ Artifact updated, version: {}", updated_artifact.version);
    
    // Test 5: Delete artifact
    println!("\n🗑️ Testing artifact deletion...");
    let deleted = store.delete_artifact(&md_artifact.id)
        .await
        .expect("Failed to delete artifact");
    
    assert!(deleted);
    
    // Verify only one artifact remains
    let remaining = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to list remaining artifacts");
    
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, code_artifact.id);
    println!("✅ Artifact deleted successfully");
}

#[tokio::test]
async fn test_cascade_delete() {
    let pool = create_test_db().await;
    let store = ProjectStore::new(pool);
    
    println!("🔗 Testing cascade delete...");
    
    // Create project with artifacts
    let project = store.create_project(
        "Cascade Test".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create project");
    
    // Add multiple artifacts
    for i in 1..=3 {
        store.create_artifact(
            project.id.clone(),
            format!("file{}.txt", i),
            ArtifactType::Note,
            Some(format!("Content {}", i)),
        )
        .await
        .expect("Failed to create artifact");
    }
    
    // Verify artifacts exist
    let artifacts = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to list artifacts");
    assert_eq!(artifacts.len(), 3);
    println!("✅ Created 3 artifacts");
    
    // Delete project
    store.delete_project(&project.id)
        .await
        .expect("Failed to delete project");
    
    // Verify artifacts are gone (this would fail if cascade delete isn't working)
    let orphaned = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to check for orphaned artifacts");
    
    assert_eq!(orphaned.len(), 0);
    println!("✅ Cascade delete successful - all artifacts removed");
}

#[tokio::test]
async fn test_artifact_types() {
    let pool = create_test_db().await;
    let store = ProjectStore::new(pool);
    
    println!("🎨 Testing all artifact types...");
    
    let project = store.create_project(
        "Type Test".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create project");
    
    let types = vec![
        (ArtifactType::Code, "test.rs"),
        (ArtifactType::Image, "test.png"),
        (ArtifactType::Log, "test.log"),
        (ArtifactType::Note, "test.txt"),
        (ArtifactType::Markdown, "test.md"),
    ];
    
    for (artifact_type, name) in types {
        let artifact = store.create_artifact(
            project.id.clone(),
            name.to_string(),
            artifact_type,
            Some("test content".to_string()),
        )
        .await
        .expect(&format!("Failed to create {:?} artifact", artifact_type));
        
        assert_eq!(artifact.artifact_type, artifact_type);
        println!("✅ Created {:?} artifact", artifact_type);
    }
    
    let all_artifacts = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to list all artifact types");
    
    assert_eq!(all_artifacts.len(), 5);
    println!("✅ All artifact types working correctly");
}

#[tokio::test]
async fn test_project_tags_serialization() {
    let pool = create_test_db().await;
    let store = ProjectStore::new(pool);
    
    println!("🏷️ Testing tag serialization...");
    
    // Create project with complex tags
    let tags = vec![
        "rust".to_string(),
        "async".to_string(),
        "test-driven".to_string(),
        "phase-1".to_string(),
    ];
    
    let project = store.create_project(
        "Tag Test".to_string(),
        None,
        Some(tags.clone()),
        None,
    )
    .await
    .expect("Failed to create project with tags");
    
    // Retrieve and verify tags
    let retrieved = store.get_project(&project.id)
        .await
        .expect("Failed to get project")
        .expect("Project not found");
    
    assert_eq!(retrieved.tags, Some(tags));
    println!("✅ Tags properly serialized and deserialized");
}

#[tokio::test]
async fn test_concurrent_operations() {
    let pool = create_test_db().await;
    let store = Arc::new(ProjectStore::new(pool));
    
    println!("⚡ Testing concurrent operations...");
    
    // Create a project first
    let project = store.create_project(
        "Concurrent Test".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create project");
    
    // Spawn multiple tasks creating artifacts concurrently
    let mut handles = vec![];
    
    for i in 0..5 {
        let store_clone = store.clone();
        let project_id = project.id.clone();
        
        let handle = tokio::spawn(async move {
            store_clone.create_artifact(
                project_id,
                format!("concurrent_{}.txt", i),
                ArtifactType::Note,
                Some(format!("Concurrent content {}", i)),
            )
            .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        handle.await
            .expect("Task panicked")
            .expect("Failed to create artifact");
    }
    
    // Verify all artifacts were created
    let artifacts = store.list_project_artifacts(&project.id)
        .await
        .expect("Failed to list artifacts");
    
    assert_eq!(artifacts.len(), 5);
    println!("✅ Concurrent operations successful");
}
