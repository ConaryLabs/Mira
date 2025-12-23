//! Tool parity tests - verify Chat tools work correctly
//!
//! Run with: cargo test --test tool_parity -- --nocapture

use anyhow::Result;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use mira::chat::tools::{
    build::BuildTools, code_intel::CodeIntelTools, documents::DocumentTools,
    git_intel::GitIntelTools, index::IndexTools,
};
use mira::core::SemanticSearch;

async fn setup_db() -> Result<SqlitePool> {
    let db_path = std::env::var("MIRA_DB").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.join(".mira/mira.db").to_string_lossy().to_string())
            .unwrap_or_else(|| "/home/peter/.mira/mira.db".to_string())
    });
    let pool = SqlitePool::connect(&format!("sqlite:{}", db_path)).await?;
    Ok(pool)
}

#[tokio::test]
async fn test_chat_get_symbols() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = CodeIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "file_path": "/home/peter/Mira/src/chat/tools/mira.rs"
    });

    let result = chat_tools.get_symbols(&args).await?;
    println!("\n=== Chat get_symbols ===");
    println!("{}", result);

    // Verify it returns valid JSON with expected fields
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("file").is_some(), "Should have 'file' field");
    assert!(parsed.get("symbols").is_some(), "Should have 'symbols' field");
    assert!(parsed.get("count").is_some(), "Should have 'count' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_get_call_graph() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = CodeIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "symbol": "record_build",
        "depth": 1
    });

    let result = chat_tools.get_call_graph(&args).await?;
    println!("\n=== Chat get_call_graph ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("symbol").is_some(), "Should have 'symbol' field");
    assert!(parsed.get("called_by").is_some(), "Should have 'called_by' field");
    assert!(parsed.get("calls").is_some(), "Should have 'calls' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_get_recent_commits() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = GitIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "limit": 5
    });

    let result = chat_tools.get_recent_commits(&args).await?;
    println!("\n=== Chat get_recent_commits ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("commits").is_some(), "Should have 'commits' field");
    assert!(parsed.get("count").is_some(), "Should have 'count' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_search_commits() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = GitIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "query": "fix",
        "limit": 3
    });

    let result = chat_tools.search_commits(&args).await?;
    println!("\n=== Chat search_commits ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("query").is_some(), "Should have 'query' field");
    assert!(parsed.get("commits").is_some(), "Should have 'commits' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_index_status() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = IndexTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "action": "status"
    });

    let result = chat_tools.index(&args).await?;
    println!("\n=== Chat index status ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("symbols").is_some(), "Should have 'symbols' field");
    assert!(parsed.get("commits").is_some(), "Should have 'commits' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_build_get_errors() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);

    let chat_tools = BuildTools {
        cwd,
        db: &db_opt,
    };

    let args = json!({
        "action": "get_errors",
        "limit": 5
    });

    let result = chat_tools.build(&args).await?;
    println!("\n=== Chat build get_errors ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("errors").is_some(), "Should have 'errors' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_document_list() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = DocumentTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "action": "list",
        "limit": 5
    });

    let result = chat_tools.document(&args).await?;
    println!("\n=== Chat document list ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("documents").is_some(), "Should have 'documents' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_find_cochange_patterns() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = GitIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "file_path": "/home/peter/Mira/src/lib.rs",
        "limit": 5
    });

    let result = chat_tools.find_cochange_patterns(&args).await?;
    println!("\n=== Chat find_cochange_patterns ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("file").is_some(), "Should have 'file' field");
    assert!(parsed.get("patterns").is_some(), "Should have 'patterns' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_get_codebase_style() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = CodeIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({});

    let result = chat_tools.get_codebase_style(&args).await?;
    println!("\n=== Chat get_codebase_style ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("total_functions").is_some(), "Should have 'total_functions' field");
    assert!(parsed.get("avg_function_length").is_some(), "Should have 'avg_function_length' field");

    Ok(())
}

#[tokio::test]
async fn test_chat_get_related_files() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let chat_tools = CodeIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    let args = json!({
        "file_path": "/home/peter/Mira/src/lib.rs"
    });

    let result = chat_tools.get_related_files(&args).await?;
    println!("\n=== Chat get_related_files ===");
    println!("{}", result);

    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    assert!(parsed.get("file").is_some(), "Should have 'file' field");

    Ok(())
}

#[tokio::test]
async fn compare_semantic_search() -> Result<()> {
    let db = setup_db().await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None; // Would need Qdrant for real test

    let chat_tools = CodeIntelTools {
        cwd,
        db: &db_opt,
        semantic: &semantic,
    };

    // This will fail gracefully without semantic search configured
    let args = json!({
        "query": "code indexing and symbol extraction",
        "limit": 5
    });

    let result = chat_tools.semantic_code_search(&args).await?;
    println!("\n=== Chat semantic_code_search ===");
    println!("{}", result);
    
    Ok(())
}
