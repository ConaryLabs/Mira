// src/operations/tool_builder.rs
// Builder for creating OpenAI-compatible function tool schemas

use serde_json::{json, Value};

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

    /// Build the final tool schema with strict mode enabled (default)
    ///
    /// OpenAI Structured Outputs (December 2025 best practice):
    /// - `strict: true` ensures model output always conforms to schema
    /// - `additionalProperties: false` required for strict mode
    /// - Near-zero tool call parsing errors
    ///
    /// Use `build_relaxed()` if strict mode causes issues with complex schemas.
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
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": properties_obj,
                    "required": self.required,
                    "additionalProperties": false
                }
            }
        })
    }

    /// Build tool schema without strict mode
    ///
    /// Use this fallback if strict mode causes issues with:
    /// - Complex nested schemas
    /// - Optional properties with defaults
    /// - Dynamic or union types
    #[allow(dead_code)]
    pub fn build_relaxed(self) -> Value {
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

/// Common property schemas for tool parameters
pub mod properties {
    use serde_json::{json, Value};

    /// String property with description
    pub fn string(description: &str) -> Value {
        json!({
            "type": "string",
            "description": description
        })
    }

    /// File path property
    pub fn path(description: &str) -> Value {
        json!({
            "type": "string",
            "description": description
        })
    }

    /// Integer property with optional default
    pub fn integer(description: &str, default: Option<i64>) -> Value {
        let mut schema = json!({
            "type": "integer",
            "description": description
        });
        if let Some(d) = default {
            schema["default"] = json!(d);
        }
        schema
    }

    /// Number (float) property with optional default
    pub fn number(description: &str, default: Option<f64>) -> Value {
        let mut schema = json!({
            "type": "number",
            "description": description
        });
        if let Some(d) = default {
            schema["default"] = json!(d);
        }
        schema
    }

    /// Boolean property with default
    pub fn boolean(description: &str, default: bool) -> Value {
        json!({
            "type": "boolean",
            "description": description,
            "default": default
        })
    }

    /// Enum property with specific allowed values
    pub fn enum_string(description: &str, values: &[&str]) -> Value {
        json!({
            "type": "string",
            "description": description,
            "enum": values
        })
    }

    /// Programming language enum property
    pub fn language() -> Value {
        json!({
            "type": "string",
            "enum": ["typescript", "javascript", "rust", "python", "go", "java", "cpp", "c", "ruby", "php"],
            "description": "Programming language"
        })
    }

    /// Text description property (alias for string)
    pub fn description(desc: &str) -> Value {
        string(desc)
    }

    /// String array property
    pub fn string_array(description: &str) -> Value {
        json!({
            "type": "array",
            "items": {"type": "string"},
            "description": description
        })
    }

    /// Optional string property (same as string, for semantic clarity)
    pub fn optional_string(description: &str) -> Value {
        string(description)
    }

    /// URL property
    pub fn url(description: &str) -> Value {
        json!({
            "type": "string",
            "format": "uri",
            "description": description
        })
    }

    /// Date/datetime string property
    pub fn date(description: &str) -> Value {
        json!({
            "type": "string",
            "description": format!("{} (e.g., '2024-01-01', '1 week ago')", description)
        })
    }

    /// Pattern/wildcard string property
    pub fn pattern(description: &str) -> Value {
        json!({
            "type": "string",
            "description": format!("{} (supports % wildcard)", description)
        })
    }

    /// Commit hash property
    pub fn commit_hash(description: &str) -> Value {
        json!({
            "type": "string",
            "description": format!("{} (full or short hash)", description)
        })
    }

    /// Search query property
    pub fn query(description: &str) -> Value {
        json!({
            "type": "string",
            "description": description
        })
    }
}
