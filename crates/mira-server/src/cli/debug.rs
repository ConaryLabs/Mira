// crates/mira-server/src/cli/debug.rs
// Debug commands for troubleshooting

use super::get_db_path;
use anyhow::Result;
use mira::db::pool::DatabasePool;
use mira::utils::path_to_string;
use std::path::PathBuf;
use std::sync::Arc;

/// Debug session_start output
pub async fn run_debug_session(path: Option<PathBuf>) -> Result<()> {
    let project_path = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    println!("=== Debug Session Start ===\n");
    println!("Project: {:?}\n", project_path);

    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Create a minimal MCP server context
    let server = mira::mcp::MiraServer::new(pool, None);

    // Call session_start
    let result = mira::tools::session_start(
        &server,
        path_to_string(&project_path),
        None,
        None,
    )
    .await;

    match result {
        Ok(output) => {
            println!("--- Session Start Output ({} chars) ---\n", output.len());
            println!("{}", output);
        }
        Err(e) => {
            println!("ERROR: {}", e);
        }
    }

    Ok(())
}

/// Debug cartographer module detection
pub async fn run_debug_carto(path: Option<PathBuf>) -> Result<()> {
    let project_path = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    println!("=== Cartographer Debug ===\n");
    println!("Project path: {:?}\n", project_path);

    // Test module detection
    let modules = mira::cartographer::detect_rust_modules(&project_path);
    println!("Detected {} modules:\n", modules.len());

    for m in &modules {
        println!("  {} ({})", m.id, m.path);
        if let Some(ref purpose) = m.purpose {
            println!("    Purpose: {}", purpose);
        }
    }

    // Try full map generation with database
    println!("\n--- Database Integration ---\n");
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    let project_path_str = path_to_string(&project_path);
    let (project_id, name) = {
        let path_clone = project_path_str.clone();
        pool.interact(move |conn| {
            mira::db::get_or_create_project_sync(conn, &path_clone, None)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await?
    };
    println!("Project ID: {}, Name: {:?}", project_id, name);

    match mira::cartographer::get_or_generate_map_pool(
        pool,
        project_id,
        project_path_str,
        name.unwrap_or_else(|| "unknown".to_string()),
        "rust".to_string(),
    )
    .await
    {
        Ok(map) => {
            println!(
                "\nCodebase map generated with {} modules",
                map.modules.len()
            );
            println!("\n{}", mira::cartographer::format_compact(&map));
        }
        Err(e) => {
            println!("\nError generating map: {}", e);
        }
    }

    Ok(())
}
