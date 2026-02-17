// crates/mira-server/src/mcp/responses/insights.rs
// Insights response types (extracted from session)

use super::ToolOutput;
use super::session::InsightsData;

pub type InsightsOutput = ToolOutput<InsightsData>;
