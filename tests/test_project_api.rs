// tests/test_project_api.rs

use axum::http::{Request, StatusCode};
use axum::body::Body;
use tower::ServiceExt;
use mira_backend::project::types::{
    CreateProjectRequest, UpdateProjectRequest, 
    CreateArtifactRequest, Project, Artifact
};

/// Helper to create a test app
async fn create_test_app() -> axum::Router {
    // Load environment variables
    dotenv::dotenv().ok();
    
    // Create in-memory database
    let pool = sqlx::SqlitePool::connect(":memory:")
        .await
        .expect("Failed to create test database");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");
    
    // Create app state
    let sqlite_store = std::sync::Arc::new(
        mira_backend::memory::sqlite::store::SqliteMemoryStore::new(pool.clone())
    );
    let project_store = std::sync::Arc::new(
        mira_backend::project::store::ProjectStore::new(pool)
    );
    
    // Mock stores for testing (we don't need real Qdrant for project tests)
    let qdrant_store = std::sync::Arc::new(
        mira_backend::memory::qdrant::store::QdrantMemoryStore::new(
            reqwest::Client::new(),
            "http://localhost:6333".to_string(),
            "test".to_string(),
        )
    );
    let llm_client = std::sync::Arc::new(
        mira_backend::llm::OpenAIClient::new()
    );
    
    let app_state = std::sync::Arc::new(mira_backend::handlers::AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
        project_store,
    });
    
    // Build the app with project routes
    axum::Router::new()
        .merge(mira_backend::project::project_router())
        .with_state(app_state)
}

#[tokio::test]
async fn test_project_api_endpoints() {
    let app = create_test_app().await;
    
    println!("🌐 Testing Project REST API...");
    
    // Test 1: Create project via API
    println!("\n📮 POST /projects");
    let create_request = CreateProjectRequest {
        name: "API Test Project".to_string(),
        description: Some("Created via API".to_string()),
        tags: Some(vec!["api".to_string(), "test".to_string()]),
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/projects")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&create_request).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::CREATED);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created_project: Project = serde_json::from_slice(&body).unwrap();
    
    println!("✅ Project created: {}", created_project.id);
    
    // Test 2: Get project by ID
    println!("\n📮 GET /project/{}", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/project/{}", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    println!("✅ Project retrieved");
    
    // Test 3: List all projects
    println!("\n📮 GET /projects");
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/projects")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(list_response["total"], 1);
    assert_eq!(list_response["projects"][0]["id"], created_project.id);
    println!("✅ Projects listed");
    
    // Test 4: Update project
    println!("\n📮 PUT /project/{}", created_project.id);
    let update_request = UpdateProjectRequest {
        name: Some("Updated API Project".to_string()),
        description: Some("Updated via API test".to_string()),
        tags: None,
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/project/{}", created_project.id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&update_request).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated_project: Project = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(updated_project.name, "Updated API Project");
    println!("✅ Project updated");
    
    // Test 5: Create artifact
    println!("\n📮 POST /artifact");
    let artifact_request = CreateArtifactRequest {
        project_id: created_project.id.clone(),
        name: "test.md".to_string(),
        artifact_type: mira_backend::project::types::ArtifactType::Markdown,
        content: Some("# Test Artifact\n\nCreated via API".to_string()),
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/artifact")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&artifact_request).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::CREATED);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created_artifact: Artifact = serde_json::from_slice(&body).unwrap();
    
    println!("✅ Artifact created: {}", created_artifact.id);
    
    // Test 6: List project artifacts
    println!("\n📮 GET /project/{}/artifacts", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/project/{}/artifacts", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let artifacts_response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(artifacts_response["total"], 1);
    println!("✅ Artifacts listed");
    
    // Test 7: Delete project (should cascade delete artifacts)
    println!("\n📮 DELETE /project/{}", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/project/{}", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    println!("✅ Project deleted");
    
    // Test 8: Verify project is gone
    println!("\n📮 GET /project/{} (should 404)", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/project/{}", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    println!("✅ Project not found (as expected)");
}

#[tokio::test]
async fn test_invalid_requests() {
    let app = create_test_app().await;
    
    println!("🚫 Testing error handling...");
    
    // Test 1: Get non-existent project
    println!("\n📮 GET /project/non-existent-id");
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/project/non-existent-id")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    println!("✅ 404 for non-existent project");
    
    // Test 2: Create artifact for non-existent project
    println!("\n📮 POST /artifact (invalid project)");
    let artifact_request = CreateArtifactRequest {
        project_id: "non-existent-project".to_string(),
        name: "test.txt".to_string(),
        artifact_type: mira_backend::project::types::ArtifactType::Note,
        content: Some("This should fail".to_string()),
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/artifact")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&artifact_request).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    println!("✅ Error for artifact with invalid project");
}
