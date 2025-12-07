// src/testing/reporters/mod.rs
// Test result reporters for different output formats

pub mod console;
pub mod json;
pub mod junit;

use crate::testing::scenarios::types::ScenarioResult;

/// Output format for test results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Console,
    Json,
    Junit,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "console" | "text" => Some(OutputFormat::Console),
            "json" => Some(OutputFormat::Json),
            "junit" | "xml" => Some(OutputFormat::Junit),
            _ => None,
        }
    }
}

/// Trait for test result reporters
pub trait Reporter {
    /// Report results for a set of scenarios
    fn report(&self, results: &[ScenarioResult], verbose: bool) -> String;
}

/// Get a reporter for the given format
pub fn get_reporter(format: OutputFormat) -> Box<dyn Reporter> {
    match format {
        OutputFormat::Console => Box::new(console::ConsoleReporter),
        OutputFormat::Json => Box::new(json::JsonReporter),
        OutputFormat::Junit => Box::new(junit::JunitReporter),
    }
}

pub use console::ConsoleReporter;
pub use json::JsonReporter;
pub use junit::JunitReporter;
