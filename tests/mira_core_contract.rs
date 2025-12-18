//! Contract tests for mira-core shared functionality
//!
//! These tests ensure mira MCP and mira-core stay in sync.
//! If these fail, it means shared behavior has drifted.

use mira_core::semantic::{COLLECTION_CODE, COLLECTION_CONVERSATION, COLLECTION_DOCS};
use mira_core::semantic_helpers::MetadataBuilder;
use mira_core::{EMBEDDING_DIM, HTTP_TIMEOUT_SECS, EMBED_RETRY_ATTEMPTS};

// ============================================================================
// Semantic Constants Contract
// ============================================================================

#[test]
fn contract_collection_names() {
    // Collection names are part of the API contract - changes require migration
    assert_eq!(COLLECTION_CODE, "mira_code", "Code collection name");
    assert_eq!(COLLECTION_CONVERSATION, "mira_conversation", "Conversation collection name");
    assert_eq!(COLLECTION_DOCS, "mira_docs", "Docs collection name");
}

#[test]
fn contract_embedding_config() {
    // Embedding configuration must match Gemini API expectations
    assert_eq!(EMBEDDING_DIM, 3072, "Gemini embedding dimensions");
    assert_eq!(HTTP_TIMEOUT_SECS, 30, "HTTP timeout for API calls");
    assert_eq!(EMBED_RETRY_ATTEMPTS, 2, "Retry attempts for transient failures");
}

// ============================================================================
// MetadataBuilder Contract
// ============================================================================

#[test]
fn contract_metadata_builder_type_field() {
    // MetadataBuilder must always include "type" field
    let metadata = MetadataBuilder::new("test_type").build();
    assert!(metadata.contains_key("type"), "Must contain type field");
    assert_eq!(
        metadata.get("type").unwrap(),
        &serde_json::json!("test_type"),
        "Type field must match"
    );
}

#[test]
fn contract_metadata_builder_string() {
    let metadata = MetadataBuilder::new("test")
        .string("key", "value")
        .build();
    assert_eq!(
        metadata.get("key").unwrap(),
        &serde_json::json!("value"),
        "String field must be stored"
    );
}

#[test]
fn contract_metadata_builder_project_id() {
    let metadata = MetadataBuilder::new("test")
        .project_id(Some(42))
        .build();
    assert_eq!(
        metadata.get("project_id").unwrap(),
        &serde_json::json!(42),
        "Project ID must be stored as number"
    );
}

#[test]
fn contract_metadata_builder_optional_none() {
    // None values should not create keys
    let metadata = MetadataBuilder::new("test")
        .string_opt("missing", None::<String>)
        .project_id(None)
        .build();
    assert!(!metadata.contains_key("missing"), "None string should not create key");
    assert!(!metadata.contains_key("project_id"), "None project_id should not create key");
}

#[test]
fn contract_metadata_builder_number() {
    let metadata = MetadataBuilder::new("test")
        .number("count", 100)
        .build();
    assert_eq!(
        metadata.get("count").unwrap(),
        &serde_json::json!(100),
        "Number field must be stored"
    );
}

#[test]
fn contract_metadata_builder_bool() {
    let metadata = MetadataBuilder::new("test")
        .bool("active", true)
        .build();
    assert_eq!(
        metadata.get("active").unwrap(),
        &serde_json::json!(true),
        "Bool field must be stored"
    );
}

// ============================================================================
// Metadata Helpers Contract
// ============================================================================

#[test]
fn contract_metadata_string_helper() {
    use mira_core::semantic_helpers::metadata_string;
    use std::collections::HashMap;

    let mut metadata = HashMap::new();
    metadata.insert("key".to_string(), serde_json::json!("value"));
    metadata.insert("number".to_string(), serde_json::json!(42));

    assert_eq!(metadata_string(&metadata, "key"), Some("value".to_string()));
    assert_eq!(metadata_string(&metadata, "number"), None); // Not a string
    assert_eq!(metadata_string(&metadata, "missing"), None);
}

#[test]
fn contract_metadata_i64_helper() {
    use mira_core::semantic_helpers::metadata_i64;
    use std::collections::HashMap;

    let mut metadata = HashMap::new();
    metadata.insert("count".to_string(), serde_json::json!(42));
    metadata.insert("text".to_string(), serde_json::json!("hello"));

    assert_eq!(metadata_i64(&metadata, "count"), Some(42));
    assert_eq!(metadata_i64(&metadata, "text"), None); // Not a number
    assert_eq!(metadata_i64(&metadata, "missing"), None);
}

#[test]
fn contract_metadata_bool_helper() {
    use mira_core::semantic_helpers::metadata_bool;
    use std::collections::HashMap;

    let mut metadata = HashMap::new();
    metadata.insert("active".to_string(), serde_json::json!(true));
    metadata.insert("text".to_string(), serde_json::json!("hello"));

    assert_eq!(metadata_bool(&metadata, "active"), Some(true));
    assert_eq!(metadata_bool(&metadata, "text"), None); // Not a bool
    assert_eq!(metadata_bool(&metadata, "missing"), None);
}
