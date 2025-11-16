// tests/tool_builder_test.rs
// Unit tests for ToolBuilder utility

use mira_backend::operations::tool_builder::{ToolBuilder, properties};
use serde_json::json;

#[test]
fn test_tool_builder_creates_valid_schema() {
    let tool = ToolBuilder::new("test_tool", "A test tool")
        .property("name", properties::description("A name"), true)
        .property(
            "optional",
            properties::optional_string("Optional field"),
            false,
        )
        .build();

    // Verify structure
    assert_eq!(tool["type"], "function");
    assert!(tool["function"].is_object());

    let function = &tool["function"];
    assert_eq!(function["name"], "test_tool");
    assert_eq!(function["description"], "A test tool");

    // Verify parameters
    let params = &function["parameters"];
    assert_eq!(params["type"], "object");
    assert!(params["properties"].is_object());
    assert!(params["required"].is_array());

    // Verify required field
    let required = params["required"].as_array().unwrap();
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "name");

    // Verify properties
    let props = &params["properties"];
    assert!(props["name"].is_object());
    assert!(props["optional"].is_object());
}

#[test]
fn test_properties_path() {
    let prop = properties::path("File path");

    assert_eq!(prop["type"], "string");
    assert_eq!(prop["description"], "File path");
}

#[test]
fn test_properties_language() {
    let prop = properties::language();

    assert_eq!(prop["type"], "string");
    assert_eq!(prop["description"], "Programming language");
    assert!(prop["enum"].is_array());

    let langs = prop["enum"].as_array().unwrap();
    assert!(langs.contains(&json!("typescript")));
    assert!(langs.contains(&json!("rust")));
    assert!(langs.contains(&json!("python")));
}

#[test]
fn test_properties_description() {
    let prop = properties::description("A description");

    assert_eq!(prop["type"], "string");
    assert_eq!(prop["description"], "A description");
}

#[test]
fn test_properties_string_array() {
    let prop = properties::string_array("List of items");

    assert_eq!(prop["type"], "array");
    assert_eq!(prop["description"], "List of items");
    assert!(prop["items"].is_object());
    assert_eq!(prop["items"]["type"], "string");
}

#[test]
fn test_properties_boolean_with_default() {
    let prop = properties::boolean("Enable feature", true);

    assert_eq!(prop["type"], "boolean");
    assert_eq!(prop["description"], "Enable feature");
    assert_eq!(prop["default"], true);

    let prop_false = properties::boolean("Disable feature", false);
    assert_eq!(prop_false["default"], false);
}

#[test]
fn test_properties_optional_string() {
    let prop = properties::optional_string("Optional text");

    assert_eq!(prop["type"], "string");
    assert_eq!(prop["description"], "Optional text");
}

#[test]
fn test_builder_multiple_required_fields() {
    let tool = ToolBuilder::new("multi_required", "Test")
        .property("field1", json!({"type": "string"}), true)
        .property("field2", json!({"type": "number"}), true)
        .property("field3", json!({"type": "boolean"}), true)
        .build();

    let required = tool["function"]["parameters"]["required"]
        .as_array()
        .unwrap();
    assert_eq!(required.len(), 3);
    assert!(required.contains(&json!("field1")));
    assert!(required.contains(&json!("field2")));
    assert!(required.contains(&json!("field3")));
}

#[test]
fn test_builder_optional_fields_not_in_required() {
    let tool = ToolBuilder::new("optional_test", "Test")
        .property("required1", json!({"type": "string"}), true)
        .property("optional1", json!({"type": "string"}), false)
        .property("optional2", json!({"type": "string"}), false)
        .build();

    let required = tool["function"]["parameters"]["required"]
        .as_array()
        .unwrap();
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "required1");

    // Verify optional fields exist in properties
    let props = &tool["function"]["parameters"]["properties"];
    assert!(props["optional1"].is_object());
    assert!(props["optional2"].is_object());
}

#[test]
fn test_builder_no_required_fields() {
    let tool = ToolBuilder::new("all_optional", "Test")
        .property("opt1", json!({"type": "string"}), false)
        .property("opt2", json!({"type": "number"}), false)
        .build();

    let required = tool["function"]["parameters"]["required"]
        .as_array()
        .unwrap();
    assert_eq!(required.len(), 0);
}

#[test]
fn test_builder_no_properties() {
    let tool = ToolBuilder::new("no_props", "Test with no properties").build();

    assert_eq!(tool["function"]["name"], "no_props");
    assert_eq!(tool["function"]["description"], "Test with no properties");

    let props = &tool["function"]["parameters"]["properties"];
    assert!(props.as_object().unwrap().is_empty());

    let required = tool["function"]["parameters"]["required"]
        .as_array()
        .unwrap();
    assert_eq!(required.len(), 0);
}

#[test]
fn test_builder_fluent_api_chaining() {
    // Test that the builder pattern allows method chaining
    let tool = ToolBuilder::new("chained", "Test")
        .property("a", json!({"type": "string"}), true)
        .property("b", json!({"type": "number"}), false)
        .property("c", json!({"type": "boolean"}), true)
        .property("d", json!({"type": "array"}), false)
        .build();

    assert_eq!(tool["function"]["name"], "chained");
    assert_eq!(
        tool["function"]["parameters"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        tool["function"]["parameters"]["required"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn test_delegation_tool_schemas_valid() {
    // Test that actual delegation tools build correctly
    use mira_backend::operations::delegation_tools::get_delegation_tools;

    let tools = get_delegation_tools();

    assert!(tools.len() >= 3, "Should have at least 3 delegation tools");

    for tool in tools {
        // Verify basic structure
        assert_eq!(tool["type"], "function");
        assert!(tool["function"].is_object());

        let func = &tool["function"];
        assert!(func["name"].is_string());
        assert!(func["description"].is_string());

        // Verify parameters structure
        let params = &func["parameters"];
        assert_eq!(params["type"], "object");
        assert!(params["properties"].is_object());
        assert!(params["required"].is_array());
    }
}

#[test]
fn test_tool_call_parsing() {
    use mira_backend::operations::delegation_tools::parse_tool_call;

    let tool_call = json!({
        "function": {
            "name": "generate_code",
            "arguments": r#"{"path": "test.ts", "language": "typescript"}"#
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_ok());

    let (name, args) = result.unwrap();
    assert_eq!(name, "generate_code");
    assert_eq!(args["path"], "test.ts");
    assert_eq!(args["language"], "typescript");
}

#[test]
fn test_tool_call_parsing_missing_name() {
    use mira_backend::operations::delegation_tools::parse_tool_call;

    let tool_call = json!({
        "function": {
            "arguments": r#"{"test": "value"}"#
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err());
}

#[test]
fn test_tool_call_parsing_missing_arguments() {
    use mira_backend::operations::delegation_tools::parse_tool_call;

    let tool_call = json!({
        "function": {
            "name": "test_tool"
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err());
}

#[test]
fn test_tool_call_parsing_invalid_json() {
    use mira_backend::operations::delegation_tools::parse_tool_call;

    let tool_call = json!({
        "function": {
            "name": "test_tool",
            "arguments": "invalid json {"
        }
    });

    let result = parse_tool_call(&tool_call);
    assert!(result.is_err());
}
