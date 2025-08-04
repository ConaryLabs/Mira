mod test_helpers;

use dotenv::dotenv;
use std::fs;
use std::path::Path;
use mira_backend::persona::PersonaOverlay;

#[tokio::test]
async fn test_hybrid_memory_integration() {
    dotenv().ok();
    println!("\n🧪 HYBRID MEMORY INTEGRATION TEST\n");
    println!("📦 Initializing app state (in-memory DB + test Qdrant collection)...");

    let state = test_helpers::create_test_app_state().await;

    // Ensure Qdrant collection exists for this test (prevents "not found" error)
    state.qdrant_store
        .ensure_collection("mira-memory")
        .await
        .expect("Failed to ensure Qdrant test collection");

    let session_id = "test-hybrid-session";
    let input = "My favorite programming language is Rust. I've been using it for 3 years.";

    // Store message using hybrid memory service (uses Responses API)
    state.hybrid_service.process_with_hybrid_memory(
        session_id,
        input,
        &PersonaOverlay::Default,
        None,
    ).await.expect("Failed to save initial context message");

    println!("✅ Context message saved to hybrid memory");

    // Semantic recall test: ask a related question
    let query = "What programming languages do I know?";
    let response = state.hybrid_service.process_with_hybrid_memory(
        session_id,
        query,
        &PersonaOverlay::Default,
        None,
    ).await.expect("Failed to get response from hybrid memory");

    println!("🔍 Query: {query}");
    println!("📝 Mira's recall response: {}", response.output);

    assert!(
        response.output.to_lowercase().contains("rust"),
        "Response should mention Rust"
    );

    println!("✅ Hybrid memory recall test PASSED!\n");
}

#[tokio::test]
async fn test_document_routing() {
    dotenv().ok();
    println!("\n🧪 DOCUMENT ROUTING TEST\n");

    let state = test_helpers::create_test_app_state().await;

    // -- SETUP: create a test project and vector store
    let project = state.project_store.create_project(
        "test-doc-project".to_string(),
        Some("Test project for document routing".to_string()),
        Some(vec!["test".to_string()]),
        None,
    ).await.expect("Failed to create test project");

    println!("📁 Created test project: {}", project.id);

    // Ensure the per-project vector store exists in Qdrant
    state.qdrant_store
        .ensure_collection(&project.id)
        .await
        .expect("Failed to ensure project Qdrant collection");

    state.vector_store_manager
        .create_project_store(&project.id)
        .await
        .expect("Failed to create project vector store");

    println!("📦 Created vector store for project");

    // --- TEST 1: Route a personal document using a real asset file
    let asset_path = Path::new("tests/assets/test_upload.txt");
    let asset_content = fs::read_to_string(&asset_path)
        .expect("Failed to read asset file for personal document test");

    // Copy to a new file to avoid any potential file lock issues
    let personal_doc_path = Path::new("test_upload_runtime_copy.txt");
    fs::write(&personal_doc_path, &asset_content).expect("Failed to copy asset for upload test");

    state.document_service.process_document(
        &personal_doc_path,
        &asset_content,
        None, // no project_id = route to personal memory
    ).await.expect("Failed to process personal document");

    println!("✅ Personal document processed (saved to personal memory)");

    // --- TEST 2: Route a technical document using the same asset, but as a project doc
    let tech_doc_path = Path::new("test_tech_upload_runtime_copy.txt");
    fs::write(&tech_doc_path, &asset_content).expect("Failed to write tech doc file");

    state.document_service.process_document(
        &tech_doc_path,
        &asset_content,
        Some(&project.id),
    ).await.expect("Failed to process technical document with project ID");

    println!("✅ Technical document processed (routed to project vector store)");

    // --- TEST 3: Route a technical doc with no project ID (should fail)
    let tech_doc2_path = Path::new("test_tech_upload_runtime_copy_2.md");
    // Here's the trick: NO personal marker, pure technical content!
    let purely_technical_content = "# API Documentation\n\nNothing personal here, just business logic and endpoints.";
    fs::write(&tech_doc2_path, &purely_technical_content).expect("Failed to write tech doc 2 file");

    let result = state.document_service.process_document(
        &tech_doc2_path,
        &purely_technical_content,
        None,
    ).await;

    assert!(
        result.is_err(),
        "Technical docs without a project ID should error"
    );
    println!("✅ Technical document without project ID correctly rejected");

    // -- CLEANUP
    let _ = fs::remove_file(&personal_doc_path);
    let _ = fs::remove_file(&tech_doc_path);
    let _ = fs::remove_file(&tech_doc2_path);
    state.project_store.delete_project(&project.id).await.ok();

    println!("✅ Document routing test PASSED!\n");
}
