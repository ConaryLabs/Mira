// src/operations/tool_builder.rs
// Builder for creating OpenAI-compatible function tool schemas

use serde_json::{Value, json};

/// Builder for creating function tool schemas
pub struct ToolBuilder {
    name: String,
    description: String,
    properties: Vec<(String, Value)>,
    required: Vec<String>,
}

impl ToolBuilder {
    /// Create a new tool with name and description
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            properties: Vec::new(),
            required: Vec::new(),
        }
    }

    /// Add a property to the tool
    pub fn property(mut self, name: impl Into<String>, schema: Value, required: bool) -> Self {
        let name = name.into();
        if required {
            self.required.push(name.clone());
        }
        self.properties.push((name, schema));
        self
    }

    /// Build the final tool schema
    /// OpenAI Chat Completions format (nested function object)
    pub fn build(self) -> Value {
        let mut properties_obj = serde_json::Map::new();
        for (name, schema) in self.properties {
            properties_obj.insert(name, schema);
        }

        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": properties_obj,
                    "required": self.required
                }
            }
        })
    }
}

/// Common property schemas
pub mod properties {
    use serde_json::{Value, json};

    /// File path property
    pub fn path(description: &str) -> Value {
        json!({
            "type": "string",
            "description": description
        })
    }

    /// Programming language enum property
    pub fn language() -> Value {
        json!({
            "type": "string",
            "enum": ["typescript", "javascript", "rust", "python", "go", "java", "cpp"],
            "description": "Programming language"
        })
    }

    /// Text description property
    pub fn description(desc: &str) -> Value {
        json!({
            "type": "string",
            "description": desc
        })
    }

    /// String array property
    pub fn string_array(description: &str) -> Value {
        json!({
            "type": "array",
            "items": {"type": "string"},
            "description": description
        })
    }

    /// Boolean property with default
    pub fn boolean(description: &str, default: bool) -> Value {
        json!({
            "type": "boolean",
            "description": description,
            "default": default
        })
    }

    /// Optional string property
    pub fn optional_string(description: &str) -> Value {
        json!({
            "type": "string",
            "description": description
        })
    }
}
