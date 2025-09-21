// src/memory/storage/sqlite/mod.rs

pub mod store;
pub mod core;
pub mod structured_ops;  // NEW: Atomic structured response operations

pub use store::SqliteMemoryStore;
pub use structured_ops::{
    save_structured_response, 
    load_structured_response, 
    get_structured_response_stats,
    StructuredResponseStats
};
