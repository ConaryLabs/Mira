// tests/test_openai_connection.rs

use mira_backend::llm::OpenAIClient;
use std::env;

#[tokio::test]
async fn test_openai_api_key_and_embedding() {
    println!("\n🔍 OPENAI API TEST\n");
    
    // Load .env file
    dotenv::dotenv().ok();
    
    // Check if API key exists
    match env::var("OPENAI_API_KEY") {
        Ok(key) => {
            println!("✅ OPENAI_API_KEY found (length: {})", key.len());
            
            // Don't print the actual key, just check format
            if key.starts_with("sk-") {
                println!("✅ API key format looks correct (starts with 'sk-')");
            } else {
                println!("⚠️  API key format might be wrong (doesn't start with 'sk-')");
            }
        }
        Err(_) => {
            println!("❌ OPENAI_API_KEY not found in environment!");
            println!("   Make sure you have a .env file with OPENAI_API_KEY=sk-...");
            return;
        }
    }
    
    // Check base URL
    match env::var("OPENAI_BASE_URL") {
        Ok(url) => println!("📍 Using custom OpenAI base URL: {}", url),
        Err(_) => println!("📍 Using default OpenAI API URL"),
    }
    
    // Try to create client and test embedding
    println!("\n🧪 Testing embedding generation...");
    let client = OpenAIClient::new();
    
    match client.get_embedding("Test message").await {
        Ok(embedding) => {
            println!("✅ Embedding generated successfully!");
            println!("   Dimensions: {}", embedding.len());
            println!("   First few values: {:?}", &embedding[..5.min(embedding.len())]);
        }
        Err(e) => {
            println!("❌ Failed to generate embedding: {:?}", e);
            println!("\nPossible issues:");
            println!("1. Invalid API key");
            println!("2. Rate limiting");
            println!("3. Network issues");
            println!("4. OpenAI service issues");
        }
    }
    
    // Test a simple chat completion
    println!("\n🧪 Testing chat completion...");
    let system_prompt = "You are a helpful assistant. Respond with exactly: 'Test passed'";
    match client.chat_with_custom_prompt("Say test passed", "gpt-4.1", system_prompt).await {
        Ok(response) => {
            println!("✅ Chat completion successful!");
            println!("   Response output: {}", response.output.chars().take(50).collect::<String>());
            println!("   Mood: {}", response.mood);
        }
        Err(e) => {
            println!("❌ Failed to get chat completion: {:?}", e);
        }
    }
    
    println!("\n✅ Test complete!\n");
}
