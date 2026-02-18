// crates/mira-server/src/cli/index.rs
// Project indexing command

use super::clients::get_embeddings_with_pool;
use super::get_db_path;
use anyhow::Result;
use mira::db::pool::DatabasePool;
use mira::http::create_shared_client;
use mira::utils::path_to_string;
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

    // Mira uses two separate databases:
    // - mira.db       (main)  — projects, sessions, goals, memories
    // - mira-code.db  (code)  — code_symbols, imports, code_chunks, vec_code
    //
    // The CLI index command needs both:
    //   1. Main DB to look up / create the project record (project_id lives there)
    //   2. Code DB for the actual symbol/chunk/embedding writes
    let db_path = get_db_path();
    let main_pool = Arc::new(DatabasePool::open(&db_path).await?);
    let code_db_path = db_path.with_file_name("mira-code.db");
    let code_pool = Arc::new(DatabasePool::open_code_db(&code_db_path).await?);

    let embeddings = if no_embed {
        None
    } else {
        get_embeddings_with_pool(Some(main_pool.clone()), http_client)
    };

    // Get or create project (stored in the main DB)
    let path_str = path_to_string(&path);
    let project_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());
    let (project_id, _project_name) = main_pool
        .interact(move |conn| {
            mira::db::get_or_create_project_sync(conn, &path_str, project_name.as_deref())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await?;

    // Alias code_pool as pool so the rest of the function is unchanged
    let pool = code_pool;

    // Ensure vec_code has correct dimensions for this embedding provider.
    // run_code_migrations() creates vec_code with a hardcoded dimension; this call
    // detects a mismatch and recreates the table with the correct dimensions before
    // any indexing writes occur. code_chunks and FTS are always preserved.
    if let Some(ref emb) = embeddings {
        let dims = emb.dimensions();
        let dim_pool = pool.clone();
        if let Err(e) = dim_pool
            .interact(move |conn| {
                mira::db::ensure_code_vec_table_dimensions(conn, dims)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
        {
            tracing::warn!("Failed to ensure vec_code dimensions: {}", e);
        }
    }

    // Set project ID for usage tracking
    if let Some(ref emb) = embeddings {
        emb.set_project_id(Some(project_id)).await;
    }

    #[cfg(not(feature = "parsers"))]
    {
        anyhow::bail!("Code indexing requires the 'parsers' feature");
    }
    #[cfg(feature = "parsers")]
    {
        let stats = mira::indexer::index_project(&path, pool, embeddings, Some(project_id)).await?;

        println!(
            "Indexed {} files, {} symbols, {} code chunks",
            stats.files, stats.symbols, stats.chunks
        );

        Ok(())
    }
}
