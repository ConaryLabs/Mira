// crates/mira-server/src/background/mod.rs
// Background workers for idle-time processing
//
// Split into two lanes:
// - Fast lane: embeddings/indexing (woken immediately by Notify)
// - Slow lane: LLM tasks (summaries, pondering, code health)

mod briefings;

pub mod code_health;
pub mod diff_analysis;
pub mod documentation;
mod embeddings;
mod fast_lane;
mod pondering;
pub mod proactive;
pub mod session_summaries;
mod slow_lane;
mod summaries;
pub mod watcher;

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::ProviderFactory;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

pub use fast_lane::FastLaneWorker;
pub use slow_lane::SlowLaneWorker;

/// Handle for waking the fast lane worker
#[derive(Clone)]
pub struct FastLaneNotify {
    notify: Arc<Notify>,
}

impl FastLaneNotify {
    /// Wake the fast lane worker to process pending embeddings
    pub fn wake(&self) {
        self.notify.notify_one();
    }
}

/// Spawn both background workers
///
/// The `pool` parameter is the code index pool (used by fast lane for embeddings).
/// The slow lane receives `pool` as its code_pool and needs a main_pool for
/// non-code operations (sessions, memory, LLM usage). Since the slow lane
/// currently only reads code tables and writes to main tables via the pool
/// passed here, we pass the same pool. Callers that need separate pools
/// should use `spawn_with_pools` instead.
///
/// Returns:
/// - shutdown sender (send true to stop all workers)
/// - fast lane notify handle (call .wake() after queuing embeddings)
pub fn spawn(
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    llm_factory: Arc<ProviderFactory>,
) -> (watch::Sender<bool>, FastLaneNotify) {
    // When called with a single pool, use it for both (backwards compat for tests)
    spawn_with_pools(pool.clone(), pool, embeddings, llm_factory)
}

/// Spawn both background workers with separate main and code pools.
///
/// - `code_pool`: Used for code index operations (embeddings, symbols, vec_code)
/// - `main_pool`: Used for sessions, memory, LLM usage, and other main DB operations
pub fn spawn_with_pools(
    code_pool: Arc<DatabasePool>,
    main_pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    llm_factory: Arc<ProviderFactory>,
) -> (watch::Sender<bool>, FastLaneNotify) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let notify = Arc::new(Notify::new());

    // Spawn fast lane worker (embeddings → code DB)
    let fast_lane = FastLaneWorker::new(
        code_pool.clone(),
        embeddings,
        shutdown_rx.clone(),
        notify.clone(),
    );
    tokio::spawn(async move {
        fast_lane.run().await;
    });

    // Spawn slow lane worker (LLM tasks → needs both DBs)
    let slow_lane = SlowLaneWorker::new(main_pool, code_pool, llm_factory, shutdown_rx);
    tokio::spawn(async move {
        slow_lane.run().await;
    });

    let fast_lane_notify = FastLaneNotify { notify };

    (shutdown_tx, fast_lane_notify)
}
