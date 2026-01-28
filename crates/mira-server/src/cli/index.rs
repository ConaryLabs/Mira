// crates/mira-server/src/cli/index.rs
// Project indexing command

use super::clients::get_embeddings_with_pool;
use super::get_db_path;
use anyhow::Result;
use mira::db::pool::DatabasePool;
use mira::http::create_shared_client;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// Run the index command to index a project
pub async fn run_index(path: Option<PathBuf>, no_embed: bool, _quiet: bool) -> Result<()> {
    let path =
        path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    // Create shared HTTP client
    let http_client = create_shared_client();

    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    let embeddings = if no_embed {
        None
    } else {
        get_embeddings_with_pool(Some(pool.clone()), http_client)
    };

    // Get or create project
    let path_str = path.to_string_lossy().to_string();
    let project_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());
    let (project_id, _project_name) = pool
        .interact(move |conn| {
            mira::db::get_or_create_project_sync(conn, &path_str, project_name.as_deref())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await?;

    // Set project ID for usage tracking
    if let Some(ref emb) = embeddings {
        emb.set_project_id(Some(project_id)).await;
    }

    let stats = mira::indexer::index_project(&path, pool, embeddings, Some(project_id)).await?;

    println!(
        "Indexed {} files, {} symbols, {} code chunks",
        stats.files, stats.symbols, stats.chunks
    );

    Ok(())
}
