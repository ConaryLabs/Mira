// src/memory/storage/sqlite/mod.rs
pub mod store;
pub mod core;
pub mod structured_ops;
pub mod user_message_processing;

pub use store::SqliteMemoryStore;
pub use structured_ops::{
    save_structured_response, 
    load_structured_response, 
    get_response_statistics,
    ResponseStatistics,
    // Backwards compatibility aliases
    get_response_statistics as get_structured_response_stats,
    ResponseStatistics as StructuredResponseStats,
};
