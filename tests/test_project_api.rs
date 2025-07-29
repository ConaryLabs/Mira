// tests/test_project_api.rs

mod test_helpers;

use axum::http::{Request, StatusCode};
use axum::body::Body;
use tower::ServiceExt;
use mira_backend::project::types::{
    CreateProjectRequest, UpdateProjectRequest, 
    CreateArtifactRequest, Project, Artifact
};

/// Helper to create a test app
async fn create_test_app() -> axum::Router {
    let app_state = test_helpers::create_test_app_state().await;
    
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
    println!("\n📮 GET /projects/{}", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/projects/{}", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let fetched_project: Project = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(fetched_project.id, created_project.id);
    assert_eq!(fetched_project.name, created_project.name);
    println!("✅ Project fetched successfully");
    
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
    let project_list: mira_backend::project::types::ProjectsResponse = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(project_list.total, 1);
    assert_eq!(project_list.projects.len(), 1);
    println!("✅ Project list retrieved");
    
    // Test 4: Update project
    println!("\n📮 PUT /projects/{}", created_project.id);
    let update_request = UpdateProjectRequest {
        name: Some("Updated API Project".to_string()),
        description: Some("Updated via API test".to_string()),
        tags: Some(vec!["updated".to_string()]),
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/projects/{}", created_project.id))
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
    println!("\n📮 POST /artifacts");
    let artifact_request = CreateArtifactRequest {
        project_id: created_project.id.clone(),
        name: "test_api.rs".to_string(),
        artifact_type: mira_backend::project::types::ArtifactType::Code,
        content: Some("// API test artifact".to_string()),
    };
    
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/artifacts")
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
    println!("\n📮 GET /projects/{}/artifacts", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/projects/{}/artifacts", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let artifacts_list: mira_backend::project::types::ArtifactsResponse = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(artifacts_list.total, 1);
    println!("✅ Artifacts listed");
    
    // Test 7: Delete project
    println!("\n📮 DELETE /projects/{}", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/projects/{}", created_project.id))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    println!("✅ Project deleted");
    
    // Test 8: Verify project is gone
    println!("\n📮 GET /projects/{} (should 404)", created_project.id);
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/projects/{}", created_project.id))
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
    println!("\n📮 GET /projects/non-existent-id");
    let response = app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/projects/non-existent-id")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    println!("✅ 404 for non-existent project");
    
    // Test 2: Create artifact for non-existent project
    println!("\n📮 POST /artifacts (invalid project)");
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
                .uri("/artifacts")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&artifact_request).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    println!("✅ Error for artifact with invalid project");
}
