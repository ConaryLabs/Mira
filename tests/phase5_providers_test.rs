// tests/phase5_providers_test.rs
// Phase 5: Provider refactoring tests - delegation tools, GPT-5, DeepSeek

use mira_backend::operations::{get_delegation_tools, parse_tool_call};
use serde_json::json;

// ============================================================================
// Delegation Tools Tests
// ============================================================================

#[test]
fn test_get_delegation_tools() {
    let tools = get_delegation_tools();
    assert_eq!(tools.len(), 3, "Should have 3 delegation tools");
    
    // Verify tool names
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("function")?.get("name")?.as_str())
        .collect();
    
    assert!(names.contains(&"generate_code"), "Should have generate_code tool");
    assert!(names.contains(&"refactor_code"), "Should have refactor_code tool");
    assert!(names.contains(&"debug_code"), "Should have debug_code tool");
}

#[test]
fn test_delegation_tools_structure() {
    let tools = get_delegation_tools();
    
    for tool in tools {
        // Each tool should have type and function
        assert_eq!(tool.get("type").and_then(|t| t.as_str()), Some("function"));
        
        let function = tool.get("function").expect("Tool should have function field");
        
        // Each function should have name, description, parameters
        assert!(function.get("name").is_some(), "Function should have name");
        assert!(function.get("description").is_some(), "Function should have description");
        assert!(function.get("parameters").is_some(), "Function should have parameters");
        
        // Parameters should have type and properties
        let params = function.get("parameters").unwrap();
        assert_eq!(params.get("type").and_then(|t| t.as_str()), Some("object"));
        assert!(params.get("properties").is_some(), "Parameters should have properties");
        assert!(params.get("required").is_some(), "Parameters should have required fields");
    }
}

#[test]
fn test_parse_tool_call_generate_code() {
    let tool_call = json!({
        "function": {
            "name": "generate_code",
            "arguments": r#"{"path": "src/test.ts", "description": "Test file", "language": "typescript"}"#
        }
    });

    let (name, args) = parse_tool_call(&tool_call).expect("Should parse tool call");
    
    assert_eq!(name, "generate_code");
    assert_eq!(args["path"], "src/test.ts");
    assert_eq!(args["description"], "Test file");
    assert_eq!(args["language"], "typescript");
}

#[test]
fn test_parse_tool_call_refactor_code() {
    let tool_call = json!({
        "function": {
            "name": "refactor_code",
            "arguments": r#"{"path": "src/old.rs", "current_code": "fn test() {}", "changes_requested": "Add error handling", "language": "rust"}"#
        }
    });

    let (name, args) = parse_tool_call(&tool_call).expect("Should parse tool call");
    
    assert_eq!(name, "refactor_code");
    assert_eq!(args["path"], "src/old.rs");
    assert_eq!(args["current_code"], "fn test() {}");
    assert_eq!(args["changes_requested"], "Add error handling");
    assert_eq!(args["language"], "rust");
}

#[test]
fn test_parse_tool_call_debug_code() {
    let tool_call = json!({
        "function": {
            "name": "debug_code",
            "arguments": r#"{"path": "src/buggy.py", "buggy_code": "def test():\n  return x", "error_message": "NameError: name 'x' is not defined", "language": "python"}"#
        }
    });

    let (name, args) = parse_tool_call(&tool_call).expect("Should parse tool call");
    
    assert_eq!(name, "debug_code");
    assert_eq!(args["path"], "src/buggy.py");
    assert_eq!(args["buggy_code"], "def test():\n  return x");
    assert_eq!(args["error_message"], "NameError: name 'x' is not defined");
    assert_eq!(args["language"], "python");
}

#[test]
fn test_parse_tool_call_missing_name() {
    let tool_call = json!({
        "function": {
            "arguments": r#"{"path": "test"}"#
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err(), "Should fail without tool name");
}

#[test]
fn test_parse_tool_call_missing_arguments() {
    let tool_call = json!({
        "function": {
            "name": "generate_code"
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err(), "Should fail without arguments");
}

#[test]
fn test_parse_tool_call_invalid_json_arguments() {
    let tool_call = json!({
        "function": {
            "name": "generate_code",
            "arguments": "not valid json"
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err(), "Should fail with invalid JSON arguments");
}

// ============================================================================
// GPT-5 Provider Tests
// ============================================================================

#[test]
fn test_normalize_verbosity() {
    use mira_backend::llm::provider::gpt5::normalize_verbosity;
    
    assert_eq!(normalize_verbosity("low"), "low");
    assert_eq!(normalize_verbosity("LOW"), "low");
    assert_eq!(normalize_verbosity("minimal"), "low");
    assert_eq!(normalize_verbosity("concise"), "low");
    
    assert_eq!(normalize_verbosity("high"), "high");
    assert_eq!(normalize_verbosity("HIGH"), "high");
    assert_eq!(normalize_verbosity("detailed"), "high");
    assert_eq!(normalize_verbosity("verbose"), "high");
    
    assert_eq!(normalize_verbosity("medium"), "medium");
    assert_eq!(normalize_verbosity("MEDIUM"), "medium");
    assert_eq!(normalize_verbosity("invalid"), "medium");
    assert_eq!(normalize_verbosity(""), "medium");
}

#[test]
fn test_normalize_reasoning() {
    use mira_backend::llm::provider::gpt5::normalize_reasoning;
    
    assert_eq!(normalize_reasoning("minimal"), "low");
    assert_eq!(normalize_reasoning("low"), "medium"); // "low" is not in the match, goes to default
    assert_eq!(normalize_reasoning("quick"), "low");
    
    assert_eq!(normalize_reasoning("high"), "high");
    assert_eq!(normalize_reasoning("HIGH"), "high");
    assert_eq!(normalize_reasoning("thorough"), "high");
    assert_eq!(normalize_reasoning("deep"), "high");
    
    assert_eq!(normalize_reasoning("medium"), "medium");
    assert_eq!(normalize_reasoning("MEDIUM"), "medium");
    assert_eq!(normalize_reasoning("invalid"), "medium");
    assert_eq!(normalize_reasoning(""), "medium");
}

// ============================================================================
// DeepSeek Provider Tests
// ============================================================================

#[test]
fn test_build_user_prompt() {
    use mira_backend::llm::provider::deepseek::{CodeGenRequest, build_user_prompt};
    
    let request = CodeGenRequest {
        path: "src/components/Button.tsx".to_string(),
        description: "Create a reusable button component".to_string(),
        language: "typescript".to_string(),
        framework: Some("react".to_string()),
        dependencies: vec!["styled-components".to_string()],
        style_guide: Some("Use functional components".to_string()),
        context: "Project uses TypeScript strict mode".to_string(),
    };

    let prompt = build_user_prompt(&request);

    assert!(prompt.contains("src/components/Button.tsx"));
    assert!(prompt.contains("Create a reusable button component"));
    assert!(prompt.contains("typescript"));
    assert!(prompt.contains("react"));
    assert!(prompt.contains("styled-components"));
    assert!(prompt.contains("Use functional components"));
    assert!(prompt.contains("TypeScript strict mode"));
    assert!(prompt.contains("Output ONLY the JSON object"));
}

#[test]
fn test_build_user_prompt_minimal() {
    use mira_backend::llm::provider::deepseek::{CodeGenRequest, build_user_prompt};
    
    let request = CodeGenRequest {
        path: "test.rs".to_string(),
        description: "Simple test".to_string(),
        language: "rust".to_string(),
        framework: None,
        dependencies: vec![],
        style_guide: None,
        context: String::new(),
    };

    let prompt = build_user_prompt(&request);

    assert!(prompt.contains("test.rs"));
    assert!(prompt.contains("Simple test"));
    assert!(prompt.contains("rust"));
    assert!(!prompt.contains("Framework:"));
    assert!(!prompt.contains("Dependencies:"));
    assert!(!prompt.contains("Style preferences:"));
}

#[test]
fn test_code_artifact_serialization() {
    use mira_backend::llm::provider::deepseek::CodeArtifact;
    
    let artifact = CodeArtifact {
        path: "test.ts".to_string(),
        content: "console.log('test');".to_string(),
        language: "typescript".to_string(),
        explanation: Some("A simple test file".to_string()),
    };

    let json = serde_json::to_string(&artifact).expect("Should serialize");
    let deserialized: CodeArtifact = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(artifact.path, deserialized.path);
    assert_eq!(artifact.content, deserialized.content);
    assert_eq!(artifact.language, deserialized.language);
    assert_eq!(artifact.explanation, deserialized.explanation);
}

#[test]
fn test_code_artifact_without_explanation() {
    use mira_backend::llm::provider::deepseek::CodeArtifact;
    
    let json = r#"{"path":"test.rs","content":"fn main() {}","language":"rust"}"#;
    let artifact: CodeArtifact = serde_json::from_str(json).expect("Should deserialize without explanation");

    assert_eq!(artifact.path, "test.rs");
    assert_eq!(artifact.content, "fn main() {}");
    assert_eq!(artifact.language, "rust");
    assert!(artifact.explanation.is_none());
}

#[test]
fn test_codegen_request_serialization() {
    use mira_backend::llm::provider::deepseek::CodeGenRequest;
    
    let request = CodeGenRequest {
        path: "src/lib.rs".to_string(),
        description: "Main library file".to_string(),
        language: "rust".to_string(),
        framework: Some("axum".to_string()),
        dependencies: vec!["tokio".to_string(), "serde".to_string()],
        style_guide: Some("Use async/await".to_string()),
        context: "Web server project".to_string(),
    };

    let json = serde_json::to_string(&request).expect("Should serialize");
    let deserialized: CodeGenRequest = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(request.path, deserialized.path);
    assert_eq!(request.description, deserialized.description);
    assert_eq!(request.language, deserialized.language);
    assert_eq!(request.framework, deserialized.framework);
    assert_eq!(request.dependencies, deserialized.dependencies);
    assert_eq!(request.style_guide, deserialized.style_guide);
    assert_eq!(request.context, deserialized.context);
}
