// src/context/mod.rs
//! Context injection system - carousel-based proactive context delivery
//!
//! Instead of dumping all context on every response (envelope pattern),
//! we rotate through context categories to:
//! - Save tokens
//! - Prevent habituation (Claude ignoring repetitive context)
//! - Deliver focused, sharp injections
//!
//! Critical items (corrections, blocked goals) always break through.

mod carousel;

pub use carousel::{
    ContextCarousel, ContextCategory, CarouselMode, CarouselState,
    CarouselTrigger, CarouselDecision, AnchorItem,
    ROTATION_INTERVAL, MAX_STARVATION_TURNS, ANCHOR_MAX_TOKENS, ANCHOR_MAX_ITEMS,
};
