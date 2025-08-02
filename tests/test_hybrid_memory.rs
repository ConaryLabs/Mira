// tests/test_hybrid_memory.rs
mod test_helpers;
use dotenv::dotenv;
use std::fs;
use std::path::Path;

#[tokio::test]
async fn test_hybrid_memory_integration() {
    // Load environment variables from .env file
    dotenv().ok();
    
    println!("\n🧪 HYBRID MEMORY INTEGRATION TEST\n");
    println!("📦 Initializing components...");
    
    // Use the test helper which creates an in-memory database
    let state = test_helpers::create_test_app_state().await;
    
    // Store context message
    let session_id = "test-hybrid-session";
    let context_msg = "My favorite programming language is Rust. I've been using it for 3 years.";
    
    // Save the message using the state's hybrid service
    state.hybrid_service.process_with_hybrid_memory(
        session_id,
        context_msg,
        &mira_backend::persona::PersonaOverlay::Default,
        None,
    ).await.expect("Failed to save context message");
    
    println!("✅ Context message saved");
    
    // Test recall with semantic relevance
    let query = "What programming languages do I know?";
    let response = state.hybrid_service.process_with_hybrid_memory(
        session_id,
        query,
        &mira_backend::persona::PersonaOverlay::Default,
        None,
    ).await.expect("Failed to get response");
    
    println!("🔍 Query: {}", query);
    println!("📝 Retrieved response: {}", response.output);
    
    assert!(response.output.contains("Rust") || response.output.contains("rust"), 
        "Response should mention Rust");
    
    println!("\n✅ Hybrid memory integration test PASSED!");
}

#[tokio::test]
async fn test_document_routing() {
    // Load environment variables from .env file
    dotenv().ok();
    
    println!("\n🧪 DOCUMENT ROUTING TEST\n");
    
    let state = test_helpers::create_test_app_state().await;
    
    // First, create a test project
    let project = state.project_store.create_project(
        "test-doc-project".to_string(),
        Some("Test project for document routing".to_string()),
        Some(vec!["test".to_string()]),
        None,
    ).await.expect("Failed to create test project");
    
    println!("📁 Created test project: {}", project.id);
    
    // Create a vector store for the project first
    state.vector_store_manager
        .create_project_store(&project.id)
        .await
        .expect("Failed to create vector store for project");
    
    println!("📦 Created vector store for project");
    
    // Test 1: Process a personal document (should go to personal memory)
    let personal_doc_path = Path::new("test_personal_doc.md");
    let personal_content = "Dear diary, today I learned about Rust memory management.";
    
    // Write test file
    fs::write(&personal_doc_path, personal_content).expect("Failed to write personal document");
    
    // Process the document - personal content should work without project ID
    state.document_service.process_document(
        &personal_doc_path,
        personal_content,
        None, // No project ID - should route to personal memory
    ).await.expect("Failed to process personal document");
    
    println!("✅ Personal document processed (routed to personal memory)");
    
    // Test 2: Process a technical document with project ID
    let tech_doc_path = Path::new("test_tech_doc.md");
    let tech_content = "# API Documentation\n\nThis is technical documentation for the API.";
    
    fs::write(&tech_doc_path, tech_content).expect("Failed to write tech document");
    
    // Process with project ID - should succeed now that vector store exists
    state.document_service.process_document(
        &tech_doc_path,
        tech_content,
        Some(&project.id), // With project ID
    ).await.expect("Failed to process technical document with project ID");
    
    println!("✅ Technical document processed with project ID (routed to vector store)");
    
    // Test 3: Try technical document without project ID - should fail
    let tech_doc_2_path = Path::new("test_tech_doc_2.md");
    fs::write(&tech_doc_2_path, tech_content).expect("Failed to write tech document 2");
    
    let result = state.document_service.process_document(
        &tech_doc_2_path,
        tech_content,
        None, // No project ID
    ).await;
    
    // Technical docs without project ID should error
    assert!(result.is_err(), "Technical docs should require project ID");
    println!("✅ Technical document without project ID correctly rejected");
    
    // Clean up test files
    let _ = fs::remove_file(&personal_doc_path);
    let _ = fs::remove_file(&tech_doc_path);
    let _ = fs::remove_file(&tech_doc_2_path);
    
    // Clean up test project
    state.project_store.delete_project(&project.id).await.ok();
    
    println!("\n✅ Document routing test PASSED!");
}
