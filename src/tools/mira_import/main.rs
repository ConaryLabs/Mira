// backend/src/tools/mira_import/main.rs

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use clap::Parser;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::tools::mira_import::{import_conversations, schema};
use sqlx::SqlitePool;

#[derive(Parser)]
#[command(name = "mira-import")]
#[command(about = "Import and reprocess ChatGPT exports into Mira's memory system", long_about = None)]
struct Cli {
    /// Path to conversations.json (unzipped from ChatGPT export)
    #[arg(short, long)]
    input: PathBuf,

    /// SQLite DB path
    #[arg(long, default_value = "mira.sqlite")]
    sqlite: String,

    /// Qdrant base URL (e.g. http://localhost:6333)
    #[arg(long, default_value = "http://localhost:6333")]
    qdrant_url: String,

    /// Qdrant collection name
    #[arg(long, default_value = "mira_memories")]
    qdrant_collection: String,

    /// Enable debug logging
    #[arg(short, long, default_value_t = false)]
    debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set up logging before any async code runs
    if cli.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt().init();
    }

    // Connect to SQLite
    let pool = SqlitePool::connect(&cli.sqlite).await?;
    let sqlite_store = SqliteMemoryStore::new(pool);

    // Connect to Qdrant (constructor is async and takes &strs)
    let qdrant_store = QdrantMemoryStore::new(&cli.qdrant_url, &cli.qdrant_collection).await?;

    // Load export
    let file = File::open(cli.input)?;
    let reader = BufReader::new(file);
    let json: schema::ChatExport = serde_json::from_reader(reader)?;

    // Import
    import_conversations(json, &sqlite_store, &qdrant_store).await?;

    Ok(())
}
