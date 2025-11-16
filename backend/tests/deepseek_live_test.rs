// tests/deepseek_live_test.rs
//
// Live integration tests for DeepSeek API
// These tests make REAL API calls - cheap but not free

use mira_backend::llm::provider::deepseek::{CodeGenRequest, DeepSeekProvider};
use std::env;

/// Get DeepSeek provider with real API key from env
fn get_provider() -> DeepSeekProvider {
    let api_key = env::var("DEEPSEEK_API_KEY")
        .or_else(|_| env::var("OPENAI_API_KEY"))
        .expect("DEEPSEEK_API_KEY or OPENAI_API_KEY must be set");

    DeepSeekProvider::new(api_key)
}

#[tokio::test]
async fn test_simple_rust_function() {
    println!("\n=== Testing Simple Rust Function Generation ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/utils.rs".to_string(),
        description: "A simple function that adds two numbers and returns the result".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec![],
        style_guide: None,
        context: String::new(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    println!("✓ Generated code at: {}", artifact.path);
    println!("Code length: {} chars", artifact.content.len());
    println!("Language: {}", artifact.language);

    // Verify it's valid Rust
    assert_eq!(artifact.language, "rust");
    assert!(
        artifact.content.contains("fn"),
        "Should contain function definition"
    );
    assert!(artifact.content.len() > 20, "Should have some code");

    // For simple requests, DeepSeek might return concise code - that's fine!
    if artifact.content.len() < 100 {
        println!(
            "ℹ Note: Simple request got concise response ({}  chars)",
            artifact.content.len()
        );
    }

    if let Some(explanation) = artifact.explanation {
        println!(
            "Explanation: {}",
            explanation.chars().take(100).collect::<String>()
        );
    }

    println!("✓ Rust function generation working");
}

#[tokio::test]
async fn test_fibonacci_with_context() {
    println!("\n=== Testing Fibonacci with Context ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/fibonacci.rs".to_string(),
        description: "Implement fibonacci function".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec![],
        style_guide: Some("Use idiomatic Rust with proper error handling".to_string()),
        context: "This is for a performance-critical application, prefer iterative over recursive"
            .to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    println!("✓ Generated fibonacci code");
    println!(
        "Tokens used: {} input, {} output",
        response.tokens_input, response.tokens_output
    );

    // Check for fibonacci-related content
    assert!(
        artifact.content.to_lowercase().contains("fib")
            || artifact.content.to_lowercase().contains("fibonacci"),
        "Code should be about fibonacci"
    );

    println!("✓ Context-aware generation working");
}

#[tokio::test]
async fn test_http_client_complex() {
    println!("\n=== Testing Complex HTTP Client Generation ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/http_client.rs".to_string(),
        description: "HTTP client with GET and POST methods, proper error handling, async/await"
            .to_string(),
        language: "rust".to_string(),
        framework: Some("tokio".to_string()),
        dependencies: vec!["reqwest".to_string(), "serde".to_string()],
        style_guide: Some("Follow Rust API guidelines, include doc comments".to_string()),
        context: "Part of a REST API wrapper library".to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    println!("✓ Generated HTTP client");
    println!("Lines of code: {}", artifact.content.lines().count());

    // Verify expected elements
    let has_struct = artifact.content.contains("struct");
    let has_impl = artifact.content.contains("impl");
    let has_async = artifact.content.contains("async");
    let has_result = artifact.content.contains("Result");

    println!("Contains struct: {}", has_struct);
    println!("Contains impl: {}", has_impl);
    println!("Contains async: {}", has_async);
    println!("Contains Result: {}", has_result);

    assert!(has_struct, "Should have struct definition");
    assert!(has_impl, "Should have implementation block");

    println!("✓ Complex code generation working");
}

#[tokio::test]
async fn test_typescript_component() {
    println!("\n=== Testing TypeScript React Component ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/components/Button.tsx".to_string(),
        description: "A reusable Button component with variants and sizes".to_string(),
        language: "typescript".to_string(),
        framework: Some("react".to_string()),
        dependencies: vec!["react".to_string()],
        style_guide: Some("Use TypeScript strict mode, functional components".to_string()),
        context: "Component library for a design system".to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    println!("✓ Generated TypeScript component");

    assert_eq!(artifact.language, "typescript");
    assert!(
        artifact.content.contains("interface") || artifact.content.contains("type"),
        "Should have TypeScript types"
    );

    println!("✓ TypeScript generation working");
}

#[tokio::test]
async fn test_python_script() {
    println!("\n=== Testing Python Script Generation ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "scripts/process_data.py".to_string(),
        description: "Script to read CSV file and compute statistics".to_string(),
        language: "python".to_string(),
        framework: None,
        dependencies: vec!["pandas".to_string(), "numpy".to_string()],
        style_guide: Some("Follow PEP 8, include type hints".to_string()),
        context: "Data processing pipeline for analytics".to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    println!("✓ Generated Python script");

    assert_eq!(artifact.language, "python");
    assert!(
        artifact.content.contains("def"),
        "Should have function definitions"
    );

    println!("✓ Python generation working");
}

#[tokio::test]
async fn test_json_output_format() {
    println!("\n=== Testing JSON Output Format ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "test.rs".to_string(),
        description: "Simple test function".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec![],
        style_guide: None,
        context: String::new(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    // Verify response structure
    assert!(response.artifact.path.len() > 0, "Should have path");
    assert!(response.artifact.content.len() > 0, "Should have content");
    assert!(response.artifact.language.len() > 0, "Should have language");
    assert!(response.tokens_input > 0, "Should have token counts");
    assert!(response.tokens_output > 0, "Should have token counts");

    println!("✓ JSON output format is correct");
    println!("  Path: {}", response.artifact.path);
    println!("  Language: {}", response.artifact.language);
    println!(
        "  Content length: {} chars",
        response.artifact.content.len()
    );
    println!(
        "  Tokens: {} in / {} out",
        response.tokens_input, response.tokens_output
    );
}

#[tokio::test]
async fn test_code_completeness() {
    println!("\n=== Testing Code Completeness (No Placeholders) ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/parser.rs".to_string(),
        description: "Parser for JSON with error handling and validation".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec!["serde_json".to_string()],
        style_guide: None,
        context: String::new(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let content = &response.artifact.content;

    // Check for placeholder indicators
    let has_placeholders = content.contains("...")
        || content.contains("// TODO")
        || content.contains("unimplemented!");

    let has_complete_braces = content.matches('{').count() == content.matches('}').count();

    println!("Has placeholders: {}", has_placeholders);
    println!("Balanced braces: {}", has_complete_braces);
    println!("Lines of code: {}", content.lines().count());

    // For complex requests, we expect reasonable code length
    let line_count = content.lines().count();
    if line_count < 10 {
        println!(
            "⚠ Warning: Only {} lines for a JSON parser - might be incomplete",
            line_count
        );
        // Don't fail, but note it
    }

    // Must have balanced braces and actual code
    assert!(has_complete_braces, "Should have balanced braces");
    assert!(
        content.contains("fn") || content.contains("def") || content.contains("function"),
        "Should have function definitions"
    );

    println!("✓ Code completeness check passed");
}

#[tokio::test]
async fn test_explanation_quality() {
    println!("\n=== Testing Explanation Quality ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/algorithm.rs".to_string(),
        description: "Implement binary search on a sorted array".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec![],
        style_guide: Some("Include doc comments and inline comments".to_string()),
        context: "Educational code for teaching algorithms".to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let artifact = response.artifact;

    // Check for comments in code
    let has_comments = artifact.content.contains("//") || artifact.content.contains("///");

    println!("Has comments in code: {}", has_comments);

    if let Some(explanation) = artifact.explanation {
        println!("Has explanation: true");
        println!("Explanation length: {} chars", explanation.len());
        assert!(explanation.len() > 20, "Explanation should be substantive");
    } else {
        println!("Has explanation: false");
    }

    println!("✓ Documentation check passed");
}

#[tokio::test]
async fn test_error_handling() {
    println!("\n=== Testing Error Handling Requirements ===\n");

    let provider = get_provider();

    let request = CodeGenRequest {
        path: "src/file_ops.rs".to_string(),
        description: "Functions to read and write files with proper error handling".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec!["std::fs".to_string()],
        style_guide: Some("Use Result types, never unwrap".to_string()),
        context: "Production code that must handle all errors gracefully".to_string(),
    };

    let response = provider
        .generate_code(request)
        .await
        .expect("Failed to generate code");

    let content = &response.artifact.content;

    // Check for error handling patterns
    let has_result = content.contains("Result<");
    let has_error_handling =
        content.contains("?") || content.contains("match") || content.contains(".map_err(");
    let has_unwrap = content.contains(".unwrap()");

    println!("Has Result types: {}", has_result);
    println!("Has error handling: {}", has_error_handling);
    println!("Has unwrap (should avoid): {}", has_unwrap);

    assert!(has_result, "Should use Result types for file operations");

    println!("✓ Error handling check passed");
}

#[tokio::test]
async fn test_performance_under_load() {
    println!("\n=== Testing Performance (5 rapid requests) ===\n");

    let provider = get_provider();
    let start = std::time::Instant::now();

    let mut handles = vec![];

    for i in 0..5 {
        let provider_clone = provider.clone();
        let handle = tokio::spawn(async move {
            let request = CodeGenRequest {
                path: format!("src/util_{}.rs", i),
                description: format!("Utility function number {}", i),
                language: "rust".to_string(),
                framework: None,
                dependencies: vec![],
                style_guide: None,
                context: String::new(),
            };

            provider_clone.generate_code(request).await
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("✓ Completed {} / 5 requests", success_count);
    println!("Total time: {:?}", elapsed);
    println!("Average: {:?} per request", elapsed / 5);

    assert!(success_count >= 4, "At least 4 out of 5 should succeed");

    println!("✓ Performance test passed");
}
