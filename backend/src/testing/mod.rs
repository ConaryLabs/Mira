// src/testing/mod.rs
// Testing infrastructure for Mira - test harness, mock providers, and observability

pub mod harness;
pub mod scenarios;

// Re-export main types for convenience
pub use harness::{TestClient, CapturedEvent, CapturedEvents, TestContext, Assertion, AssertionResult};
pub use scenarios::{TestScenario, TestStep};
