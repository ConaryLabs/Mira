//! Structured output types for MCP tools.
//!
//! Each tool returns a wrapper struct with `action`, `message`, and optional typed `data`.
//! Using `Json<T>` return types, rmcp auto-infers `outputSchema` for each tool.
//! The root type is always an object (MCP requirement).

use schemars::JsonSchema;
use serde::Serialize;

// ============================================================================
// Memory
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct MemoryOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<MemoryData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MemoryData {
    Remember(RememberData),
    Recall(RecallData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RememberData {
    pub id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecallData {
    pub memories: Vec<MemoryItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MemoryItem {
    pub id: i64,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

// ============================================================================
// Project
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ProjectData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProjectData {
    Start(ProjectStartData),
    Get(ProjectGetData),
    Set(ProjectSetData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectStartData {
    pub project_id: i64,
    pub project_name: Option<String>,
    pub project_path: String,
    pub project_type: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectGetData {
    pub project_id: i64,
    pub project_name: Option<String>,
    pub project_path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectSetData {
    pub project_id: i64,
    pub project_name: Option<String>,
}

// ============================================================================
// Code
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodeOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<CodeData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CodeData {
    Search(SearchResultsData),
    Symbols(SymbolsData),
    CallGraph(CallGraphData),
    Dependencies(DependenciesData),
    Patterns(PatternsData),
    TechDebt(TechDebtData),
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

// ============================================================================
// Goal
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<GoalData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum GoalData {
    Created(GoalCreatedData),
    BulkCreated(GoalBulkCreatedData),
    List(GoalListData),
    Get(GoalGetData),
    MilestoneProgress(MilestoneProgressData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalCreatedData {
    pub goal_id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalBulkCreatedData {
    pub goals: Vec<GoalCreatedEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalCreatedEntry {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalListData {
    pub goals: Vec<GoalSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalSummary {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalGetData {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub milestones: Vec<MilestoneInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MilestoneInfo {
    pub id: i64,
    pub title: String,
    pub weight: i32,
    pub completed: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MilestoneProgressData {
    pub milestone_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<i32>,
}

// ============================================================================
// Index
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<IndexData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IndexData {
    Project(IndexProjectData),
    Status(IndexStatusData),
    Compact(IndexCompactData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexProjectData {
    pub files: usize,
    pub symbols: usize,
    pub chunks: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules_summarized: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexStatusData {
    pub symbols: usize,
    pub embedded_chunks: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexCompactData {
    pub rows_preserved: usize,
    pub estimated_savings_mb: f64,
}

// ============================================================================
// Session
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<SessionData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SessionData {
    Current(SessionCurrentData),
    ListSessions(SessionListData),
    History(SessionHistoryData),
    Insights(InsightsData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionCurrentData {
    pub session_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionListData {
    pub sessions: Vec<SessionSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionSummary {
    pub id: String,
    pub started_at: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionHistoryData {
    pub session_id: String,
    pub entries: Vec<HistoryEntry>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryEntry {
    pub tool_name: String,
    pub created_at: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InsightsData {
    pub insights: Vec<InsightItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InsightItem {
    pub source: String,
    pub source_type: String,
    pub description: String,
    pub priority_score: f64,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

// ============================================================================
// Expert
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExpertOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ExpertData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ExpertData {
    Consult(ConsultData),
    Configure(ConfigureData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConsultData {
    pub opinions: Vec<ExpertOpinion>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExpertOpinion {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigureData {
    pub configs: Vec<ExpertConfigEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExpertConfigEntry {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_custom_prompt: Option<bool>,
}

// ============================================================================
// Documentation
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DocData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocData {
    List(DocListData),
    Get(DocGetData),
    Inventory(DocInventoryData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocListData {
    pub tasks: Vec<DocTaskItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocTaskItem {
    pub id: i64,
    pub doc_category: String,
    pub target_doc_path: String,
    pub priority: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocGetData {
    pub task_id: i64,
    pub target_doc_path: String,
    pub full_target_path: String,
    pub doc_type: String,
    pub doc_category: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub guidelines: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocInventoryData {
    pub docs: Vec<DocInventoryItem>,
    pub total: usize,
    pub stale_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocInventoryItem {
    pub doc_path: String,
    pub doc_type: String,
    pub is_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_reason: Option<String>,
}

// ============================================================================
// Finding
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<FindingData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FindingData {
    List(FindingListData),
    Get(FindingGetData),
    Stats(FindingStatsData),
    Patterns(FindingPatternsData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingListData {
    pub findings: Vec<FindingItem>,
    pub stats: FindingStatsData,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingItem {
    pub id: i64,
    pub finding_type: String,
    pub severity: String,
    pub status: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingGetData {
    pub id: i64,
    pub finding_type: String,
    pub severity: String,
    pub status: String,
    pub expert_role: String,
    pub confidence: f64,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FindingStatsData {
    pub pending: i64,
    pub accepted: i64,
    pub rejected: i64,
    pub fixed: i64,
    pub total: i64,
    pub acceptance_rate: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingPatternsData {
    pub patterns: Vec<LearnedPattern>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LearnedPattern {
    pub id: i64,
    pub correction_type: String,
    pub confidence: f64,
    pub occurrence_count: i64,
    pub acceptance_rate: f64,
    pub what_was_wrong: String,
    pub what_is_right: String,
}

// ============================================================================
// Diff
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffOutput {
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiffData>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DiffData {
    Analysis(DiffAnalysisData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffAnalysisData {
    pub from_ref: String,
    pub to_ref: String,
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
}

// ============================================================================
// Reply
// ============================================================================

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReplyOutput {
    pub action: String,
    pub message: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::tool::schema_for_output;

    #[test]
    fn all_schemas_are_valid_mcp_output() {
        // Each output type must produce a root type "object" schema
        assert!(schema_for_output::<MemoryOutput>().is_ok(), "MemoryOutput");
        assert!(
            schema_for_output::<ProjectOutput>().is_ok(),
            "ProjectOutput"
        );
        assert!(schema_for_output::<CodeOutput>().is_ok(), "CodeOutput");
        assert!(schema_for_output::<GoalOutput>().is_ok(), "GoalOutput");
        assert!(schema_for_output::<IndexOutput>().is_ok(), "IndexOutput");
        assert!(
            schema_for_output::<SessionOutput>().is_ok(),
            "SessionOutput"
        );
        assert!(schema_for_output::<ExpertOutput>().is_ok(), "ExpertOutput");
        assert!(schema_for_output::<DocOutput>().is_ok(), "DocOutput");
        assert!(
            schema_for_output::<FindingOutput>().is_ok(),
            "FindingOutput"
        );
        assert!(schema_for_output::<DiffOutput>().is_ok(), "DiffOutput");
        assert!(schema_for_output::<ReplyOutput>().is_ok(), "ReplyOutput");
    }
}
