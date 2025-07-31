// tests/test_hybrid_memory.rs

use mira_backend::{
    llm::OpenAIClient,
    llm::assistant::{AssistantManager, VectorStoreManager, ThreadManager},
    memory::{
        sqlite::store::SqliteMemoryStore,
        qdrant::store::QdrantMemoryStore,
        traits::MemoryStore,  // Add this import
    },
    services::{ChatService, MemoryService, ContextService, HybridMemoryService, DocumentService},
    persona::PersonaOverlay,
    project::store::ProjectStore,
    git::{GitStore, GitClient},
};
use std::sync::Arc;
use std::path::Path;
use tokio;

#[tokio::test]
async fn test_hybrid_memory_integration() {
    println!("\n🧪 HYBRID MEMORY INTEGRATION TEST\n");
    
    // Load environment
    dotenv::dotenv().ok();
    
    // 1. Initialize all components
    println!("📦 Initializing components...");
    
    // Database
    let pool = sqlx::SqlitePool::connect("sqlite://test_hybrid.db").await
        .expect("Failed to connect to test database");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool).await
        .expect("Failed to run migrations");
    
    // Memory stores
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        reqwest::Client::new(),
        "http://localhost:6333",
        "test-hybrid-memory",
    ));
    
    // LLM client
    let llm_client = Arc::new(OpenAIClient::new());
    
    // Project store
    let project_store = Arc::new(ProjectStore::new(pool.clone()));
    
    // Git stores
    let git_store = GitStore::new(pool.clone());
    let git_client = GitClient::new("./test_repos", git_store.clone());
    
    // Services
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        llm_client.clone(),
    ));
    
    let context_service = Arc::new(ContextService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));
    
    let chat_service = Arc::new(ChatService::new(llm_client.clone()));
    
    // Assistant components
    println!("🤖 Initializing OpenAI Assistant...");
    let mut assistant_manager = AssistantManager::new(llm_client.clone());
    assistant_manager.create_assistant().await
        .expect("Failed to create assistant");
    let assistant_manager = Arc::new(assistant_manager);
    
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(llm_client.clone()));
    
    // Hybrid services
    let hybrid_service = Arc::new(HybridMemoryService::new(
        chat_service.clone(),
        memory_service.clone(),
        context_service.clone(),
        assistant_manager.clone(),
        vector_store_manager.clone(),
        thread_manager.clone(),
    ));
    
    let document_service = Arc::new(DocumentService::new(
        vector_store_manager.clone(),
        memory_service.clone(),
        chat_service.clone(),
    ));
    
    // 2. Create a test project
    println!("\n📁 Creating test project...");
    let project = project_store.create_project(
        "test-hybrid-project".to_string(),
        Some("Testing hybrid memory integration".to_string()),
        Some(vec!["test".to_string()]),
        None,
    ).await.expect("Failed to create project");
    
    println!("✅ Created project: {}", project.id);
    
    // 3. Test document upload
    println!("\n📄 Testing document upload...");
    
    // Create a test document
    let test_doc_path = Path::new("test_document.md");
    let test_content = r#"# Test Project Documentation

This is a test document for the hybrid memory system.

## Technical Details
- Uses OpenAI vector stores for document search
- Integrates with personal memory in Qdrant
- Supports hybrid retrieval

## Personal Note
This project means a lot to me because it represents a breakthrough in AI memory systems.
"#;
    
    tokio::fs::write(&test_doc_path, test_content).await
        .expect("Failed to write test document");
    
    // Process the document
    document_service.process_document(
        &test_doc_path,
        test_content,
        Some(&project.id),
    ).await.expect("Failed to process document");
    
    println!("✅ Document processed and uploaded");
    
    // 4. Test hybrid memory chat
    println!("\n💬 Testing hybrid memory chat...");
    
    let test_message = "What technical details are in the project documentation?";
    let session_id = "test-session";
    
    let response = hybrid_service.process_with_hybrid_memory(
        session_id,
        test_message,
        &PersonaOverlay::Default,
        Some(&project.id),
    ).await.expect("Failed to process with hybrid memory");
    
    println!("✅ Got response from hybrid system:");
    println!("   Output: {}", response.output.chars().take(200).collect::<String>());
    println!("   Salience: {}", response.salience);
    println!("   Mood: {}", response.mood);
    
    // 5. Verify vector store was created
    println!("\n🔍 Verifying vector store...");
    
    let store_info = vector_store_manager.get_store_info(&project.id).await;
    assert!(store_info.is_some(), "Vector store should exist for project");
    
    if let Some(info) = store_info {
        println!("✅ Vector store found:");
        println!("   ID: {}", info.id);
        println!("   Name: {}", info.name);
        println!("   Files: {} uploaded", info.file_ids.len());
    }
    
    // 6. Test personal memory sync
    println!("\n🧠 Testing personal memory sync...");
    
    // Send a high-salience message
    let important_message = "This hybrid memory system is a game-changer for my work!";
    
    let important_response = hybrid_service.process_with_hybrid_memory(
        session_id,
        important_message,
        &PersonaOverlay::Default,
        Some(&project.id),
    ).await.expect("Failed to process important message");
    
    println!("✅ Processed high-salience message");
    
    // 7. Verify memories were saved
    println!("\n📊 Checking saved memories...");
    
    let recent_memories = sqlite_store.load_recent(session_id, 10).await
        .expect("Failed to load recent memories");
    
    println!("✅ Found {} memories in SQLite", recent_memories.len());
    
    for (i, memory) in recent_memories.iter().enumerate() {
        println!("   {}. [{}] {}", 
            i + 1, 
            memory.role, 
            memory.content.chars().take(50).collect::<String>()
        );
    }
    
    // Clean up
    tokio::fs::remove_file(&test_doc_path).await.ok();
    println!("\n✅ Hybrid memory integration test complete!");
}

#[tokio::test]
async fn test_document_routing() {
    println!("\n🧪 DOCUMENT ROUTING TEST\n");
    
    // Minimal setup for document routing test
    let llm_client = Arc::new(OpenAIClient::new());
    let sqlite_store = Arc::new(SqliteMemoryStore::new(
        sqlx::SqlitePool::connect(":memory:").await.unwrap()
    ));
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        reqwest::Client::new(),
        "http://localhost:6333",
        "test-routing",
    ));
    
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store,
        qdrant_store,
        llm_client.clone(),
    ));
    let chat_service = Arc::new(ChatService::new(llm_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client));
    
    let document_service = Arc::new(DocumentService::new(
        vector_store_manager,
        memory_service,
        chat_service,
    ));
    
    // Test different document types
    let test_cases = vec![
        ("diary.txt", "Dear diary, today was wonderful...", "PersonalMemory"),
        ("technical_spec.md", "# API Specification\n\n## Endpoints", "ProjectVectorStore"),
        ("personal_notes.txt", "Remember to call mom", "PersonalMemory"),
        ("code.py", "def main():\n    print('Hello')", "ProjectVectorStore"),
    ];
    
    for (filename, content, expected) in test_cases {
        println!("Testing: {} -> Expected: {}", filename, expected);
        
        // The analyze_and_route method is private, so we test through process_document
        // For this test, we'll just verify it doesn't panic
        let path = Path::new(filename);
        
        // This will fail for ProjectVectorStore without a project_id, which is expected
        let result = document_service.process_document(
            path,
            content,
            None, // No project_id
        ).await;
        
        if expected == "PersonalMemory" {
            assert!(result.is_ok(), "Personal memory documents should process without project_id");
            println!("✅ {} routed correctly", filename);
        } else {
            assert!(result.is_err(), "Project documents should fail without project_id");
            println!("✅ {} correctly requires project_id", filename);
        }
    }
    
    println!("\n✅ Document routing test complete!");
}
