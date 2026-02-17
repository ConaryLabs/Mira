// crates/mira-server/src/mcp/responses/code.rs
// Response types for the unified code tool

use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type CodeOutput = ToolOutput<CodeData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CodeData {
    Search(SearchResultsData),
    Symbols(SymbolsData),
    CallGraph(CallGraphData),
    Dependencies(DependenciesData),
    Patterns(PatternsData),
    TechDebt(TechDebtData),
    DeadCode(DeadCodeData),
    Conventions(ConventionsData),
    DebtDelta(DebtDeltaData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResultsData {
    pub results: Vec<CodeSearchResult>,
    pub search_type: String,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodeSearchResult {
    pub file_path: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_info: Option<String>,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SymbolsData {
    pub symbols: Vec<SymbolInfo>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SymbolInfo {
    pub name: String,
    pub symbol_type: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CallGraphData {
    pub target: String,
    pub direction: String,
    pub functions: Vec<CallGraphEntry>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CallGraphEntry {
    pub function_name: String,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DependenciesData {
    pub edges: Vec<DependencyEdge>,
    pub circular_count: usize,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DependencyEdge {
    pub source: String,
    pub target: String,
    pub dependency_type: String,
    pub call_count: i64,
    pub import_count: i64,
    pub is_circular: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PatternsData {
    pub modules: Vec<ModulePatterns>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModulePatterns {
    pub module_id: String,
    pub module_name: String,
    pub patterns: Vec<PatternEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PatternEntry {
    pub pattern: String,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TechDebtData {
    pub modules: Vec<TechDebtModule>,
    pub summary: Vec<TechDebtTier>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TechDebtModule {
    pub module_path: String,
    pub tier: String,
    pub overall_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_factors: Option<Vec<DebtFactor>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebtFactor {
    pub name: String,
    pub score: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TechDebtTier {
    pub tier: String,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeadCodeData {
    pub unreferenced: Vec<UnreferencedSymbol>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UnreferencedSymbol {
    pub name: String,
    pub symbol_type: String,
    pub file_path: String,
    pub start_line: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConventionsData {
    pub module_id: String,
    pub module_name: String,
    pub error_handling: Option<String>,
    pub test_pattern: Option<String>,
    pub naming: Option<String>,
    pub key_imports: Option<String>,
    pub detected_patterns: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebtDeltaData {
    pub modules: Vec<ModuleStanding>,
    pub summary: DebtDeltaSummary,
}

/// Current module debt standing (no per-module history available)
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModuleStanding {
    pub module_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_path: Option<String>,
    pub score: f64,
    pub tier: String,
}

/// Aggregate delta from health snapshots (accurate project-level trend)
#[derive(Debug, Serialize, JsonSchema)]
pub struct DebtDeltaSummary {
    pub previous_avg: f64,
    pub current_avg: f64,
    pub avg_delta: f64,
    pub trend: String,
    pub module_count: usize,
}
