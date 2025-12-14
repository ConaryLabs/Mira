// src/tools/semantic_helpers.rs
// Semantic search helper functions to reduce code duplication across tools

use std::collections::HashMap;
use serde_json::Value;
use super::semantic::SemanticSearch;

/// Build semantic metadata with common fields.
/// Automatically adds "type" and optionally "project_id".
pub struct MetadataBuilder {
    metadata: HashMap<String, Value>,
}

impl MetadataBuilder {
    /// Create a new metadata builder with the required type field.
    pub fn new(type_name: &str) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), Value::String(type_name.to_string()));
        Self { metadata }
    }

    /// Add a string field to the metadata.
    pub fn string(mut self, key: &str, value: impl Into<String>) -> Self {
        self.metadata.insert(key.to_string(), Value::String(value.into()));
        self
    }

    /// Add a string field only if the value is Some.
    pub fn string_opt(mut self, key: &str, value: Option<impl Into<String>>) -> Self {
        if let Some(v) = value {
            self.metadata.insert(key.to_string(), Value::String(v.into()));
        }
        self
    }

    /// Add a project_id field if present.
    pub fn project_id(mut self, project_id: Option<i64>) -> Self {
        if let Some(pid) = project_id {
            self.metadata.insert("project_id".to_string(), Value::Number(pid.into()));
        }
        self
    }

    /// Add a number field.
    pub fn number(mut self, key: &str, value: i64) -> Self {
        self.metadata.insert(key.to_string(), Value::Number(value.into()));
        self
    }

    /// Build the final metadata HashMap.
    pub fn build(self) -> HashMap<String, Value> {
        self.metadata
    }
}

/// Store content in semantic search with error logging.
/// Ensures collection exists and stores content, logging any errors.
pub async fn store_with_logging(
    semantic: &SemanticSearch,
    collection: &str,
    id: &str,
    content: &str,
    metadata: HashMap<String, Value>,
) {
    if !semantic.is_available() {
        return;
    }

    if let Err(e) = semantic.ensure_collection(collection).await {
        tracing::warn!("Failed to ensure {} collection: {}", collection, e);
    }

    if let Err(e) = semantic.store(collection, id, content, metadata).await {
        tracing::warn!("Failed to store in {}: {}", collection, e);
    }
}

/// Result from semantic search with common fields.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
}

/// Search with semantic search if available, with optional filter.
/// Returns None if semantic search is unavailable or returns no results.
pub async fn search_semantic(
    semantic: &SemanticSearch,
    collection: &str,
    query: &str,
    limit: usize,
    filter: Option<qdrant_client::qdrant::Filter>,
) -> Option<Vec<SearchResult>> {
    if !semantic.is_available() {
        return None;
    }

    match semantic.search(collection, query, limit, filter).await {
        Ok(results) if !results.is_empty() => {
            Some(results.into_iter().map(|r| SearchResult {
                content: r.content,
                score: r.score,
                metadata: r.metadata,
            }).collect())
        }
        Ok(_) => {
            tracing::debug!("No semantic results for query in {}: {}", collection, query);
            None
        }
        Err(e) => {
            tracing::warn!("Semantic search failed in {}, falling back: {}", collection, e);
            None
        }
    }
}

/// Helper to get a string value from metadata.
pub fn metadata_string(metadata: &HashMap<String, Value>, key: &str) -> Option<String> {
    metadata.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Helper to get an i64 value from metadata.
pub fn metadata_i64(metadata: &HashMap<String, Value>, key: &str) -> Option<i64> {
    metadata.get(key).and_then(|v| v.as_i64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_builder_basic() {
        let metadata = MetadataBuilder::new("test_type")
            .string("key", "value")
            .build();

        assert_eq!(metadata.get("type").unwrap(), &Value::String("test_type".to_string()));
        assert_eq!(metadata.get("key").unwrap(), &Value::String("value".to_string()));
    }

    #[test]
    fn test_metadata_builder_with_project() {
        let metadata = MetadataBuilder::new("memory")
            .string("fact_type", "preference")
            .project_id(Some(42))
            .build();

        assert_eq!(metadata.get("project_id").unwrap(), &Value::Number(42.into()));
    }

    #[test]
    fn test_metadata_builder_optional_none() {
        let metadata = MetadataBuilder::new("test")
            .string_opt("missing", None::<String>)
            .build();

        assert!(!metadata.contains_key("missing"));
    }

    #[test]
    fn test_metadata_string_helper() {
        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), Value::String("value".to_string()));

        assert_eq!(metadata_string(&metadata, "key"), Some("value".to_string()));
        assert_eq!(metadata_string(&metadata, "missing"), None);
    }
}
