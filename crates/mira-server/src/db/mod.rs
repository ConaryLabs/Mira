// db/mod.rs
// Unified database layer with rusqlite + sqlite-vec

mod background;
mod cartographer;
mod config;
pub mod cross_project;
pub mod dependencies;
mod diff_analysis;
pub mod diff_outcomes;
pub mod documentation;
mod embeddings;
pub mod entities;
pub mod error_patterns;
mod index;
mod insights;
mod memory;
mod migration_helpers;
mod milestones;
pub mod observations;
pub mod pool;
mod project;
pub mod retention;
mod schema;
pub use schema::vectors::{
    check_embedding_provider_change, ensure_code_embeddings_queued,
    ensure_code_vec_table_dimensions, ensure_vec_table_dimensions, invalidate_code_embeddings,
};
mod search;
mod session;
mod session_goals;
pub mod session_tasks;
mod tasks;
pub mod team;
#[cfg(test)]
#[macro_use]
pub(crate) mod test_support;
#[cfg(test)]
mod cross_project_tests;
#[cfg(test)]
mod insights_tests;
#[cfg(test)]
mod memory_tests;
#[cfg(test)]
mod project_tests;
#[cfg(test)]
mod session_goals_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tasks_tests;
pub mod tech_debt;
mod types;
mod usage;

pub use background::{
    clear_health_issues_by_categories_sync,
    clear_old_health_issues_sync,
    delete_memory_by_key_sync,
    delete_pending_embedding_sync,
    get_documented_by_category_sync,
    get_error_heavy_functions_sync,
    // Code health analysis
    get_large_functions_sync,
    get_lib_symbols_sync,
    get_modules_for_doc_gaps_sync,
    // Summaries processor
    get_project_ids_needing_summaries_sync,
    get_projects_with_pending_summaries_sync,
    get_scan_info_sync,
    get_symbols_for_file_sync,
    get_unused_functions_sync,
    insert_system_marker_sync,
    is_time_older_than_sync,
    // Diff analysis
    map_files_to_symbols_sync,
    mark_health_scanned_sync,
    mark_llm_health_done_sync,
    memory_key_exists_sync,
    store_code_embedding_sync,
};
pub use cartographer::{
    count_cached_modules_sync, count_symbols_in_path_sync, get_cached_modules_sync,
    get_external_deps_sync, get_module_dependencies_sync, get_module_exports_sync,
    get_modules_needing_summaries_sync, update_module_purposes_sync, upsert_module_sync,
};
pub use cross_project::{
    CrossProjectMemory, CrossProjectPreference, find_solved_in_other_project_sync,
    format_cross_project_context, format_cross_project_preferences,
    get_cross_project_preferences_sync, recall_cross_project_sync,
};
pub use diff_analysis::{
    DiffAnalysis, StoreDiffAnalysisParams, get_cached_diff_analysis_sync,
    get_recent_diff_analyses_sync, store_diff_analysis_sync,
};
pub use documentation::{DocGap, DocInventory, DocTask, get_inventory_for_stale_check};
pub use embeddings::{PendingEmbedding, get_pending_embeddings_sync};
pub use error_patterns::{
    ErrorPatternRow, ResolvedErrorPattern, StoreErrorPatternParams, error_fingerprint,
    get_error_patterns_sync, get_unresolved_patterns_for_tool_sync, lookup_resolved_pattern_sync,
    resolve_error_pattern_sync, store_error_pattern_sync,
};
pub use index::{
    CompactStats,
    ImportInsert,
    // Batch insert operations
    SymbolInsert,
    clear_file_index_sync,
    clear_modules_without_purpose_sync,
    clear_project_index_sync,
    compact_vec_code_sync,
    count_embedded_chunks_sync,
    count_symbols_sync,
    insert_call_sync,
    insert_chunk_embedding_sync,
    insert_code_chunk_sync,
    insert_code_fts_entry_sync,
    insert_import_sync,
    insert_symbol_sync,
    queue_pending_embedding_sync,
};
pub(crate) use insights::compute_age_days;
pub use insights::{dismiss_insight_sync, get_health_history_sync, get_unified_insights_sync};
pub use memory::{
    MemoryScopeInfo,
    RankedMemory,
    RecallRow,
    StoreMemoryParams,
    clear_project_persona_sync,
    count_facts_without_embeddings_sync,
    delete_memory_sync,
    fetch_ranked_memories_for_export_sync,
    find_facts_without_embeddings_sync,
    get_base_persona_sync,
    get_global_memories_sync,
    get_health_alerts_sync as get_health_alerts_memory_sync, // alias for callers using the old 3-param signature
    get_memory_scope_sync,
    get_memory_stats_sync,
    get_preferences_sync as get_preferences_memory_sync,
    get_project_persona_sync,
    import_confirmed_memory_sync,
    mark_fact_has_embedding_sync,
    mark_memories_stale_for_file_sync,
    parse_memory_fact_row,
    recall_semantic_with_entity_boost_sync,
    record_memory_access_sync,
    search_memories_sync,
    store_fact_embedding_sync,
    // Sync functions for pool.interact() usage
    store_memory_sync,
};
pub use milestones::{
    calculate_goal_progress_sync, complete_milestone_sync, create_milestone_sync,
    delete_milestone_sync, get_milestone_by_id_sync, get_milestones_for_goal_sync,
    parse_milestone_row, update_goal_progress_from_milestones_sync, update_milestone_sync,
};
pub use observations::{
    StoreObservationParams, cleanup_expired_observations_sync, delete_observation_by_key_sync,
    delete_observations_by_categories_sync, delete_observations_by_type_sync,
    get_health_observations_sync, get_observation_info_sync, observation_key_exists_sync,
    query_observations_by_categories_sync, query_observations_by_type_sync,
    query_team_observations_sync, store_observation_sync,
};
pub use project::{
    clear_active_project_sync, delete_server_state_sync, get_active_project_ids_sync,
    get_active_projects_sync, get_health_alerts_sync, get_indexed_project_ids_sync,
    get_indexed_projects_sync, get_last_active_project_sync, get_or_create_project_sync,
    get_preferences_sync, get_project_briefing_sync, get_project_info_sync, get_project_path_sync,
    get_project_paths_by_ids_sync, get_projects_for_briefing_check_sync,
    get_projects_needing_suggestions_sync, get_server_state_sync, list_projects_sync,
    mark_session_for_briefing_sync, save_active_project_sync, search_memories_text_sync,
    set_server_state_sync, update_project_briefing_sync, update_project_name_sync,
    upsert_session_sync, upsert_session_with_branch_sync,
};
pub use retention::{cleanup_orphans, count_retention_candidates, run_data_retention_sync};
pub use search::{
    ChunkSearchResult, CrossRefResult, FtsSearchResult, SemanticCodeResult, SymbolSearchResult,
    chunk_like_search_sync, find_callees_sync, find_callers_sync, fts_search_sync,
    get_symbol_bounds_sync, semantic_code_search_sync, symbol_like_search_sync,
};
pub use session::{
    LineageRow, build_session_recap_sync, close_session_sync, create_session_ext_sync,
    create_session_sync, get_history_after_sync, get_recent_sessions_sync,
    get_session_behavior_summary_sync, get_session_history_scoped_sync, get_session_history_sync,
    get_session_lineage_sync, get_session_stats_sync, get_session_tool_summary_sync,
    get_sessions_needing_summary_sync, get_stale_sessions_sync, log_tool_call_sync,
    touch_session_sync, update_session_summary_sync,
};
pub use session_goals::{
    count_sessions_for_goal_sync, delete_session_goals_for_goal_sync, get_goals_for_session_sync,
    get_sessions_for_goal_sync, parse_session_goal_row, record_session_goal_sync,
};
pub use tasks::{
    count_goals_sync, create_goal_sync, create_task_sync, delete_goal_sync, delete_task_sync,
    get_active_goals_sync, get_goal_by_id_sync, get_goals_sync, get_pending_tasks_sync,
    get_task_by_id_sync, get_tasks_sync, parse_goal_row, parse_task_row, update_goal_sync,
    update_task_sync,
};
pub use team::{
    FileConflict, TeamInfo, TeamMemberInfo, cleanup_stale_sessions_sync,
    deactivate_team_session_sync, get_active_team_members_sync, get_file_conflicts_sync,
    get_member_files_sync, get_or_create_team_sync, get_team_for_session_sync,
    get_team_membership_for_session_sync, heartbeat_team_session_sync, record_file_ownership_sync,
    register_team_session_sync, validate_team_membership_sync,
};
pub use types::*;
pub use usage::{
    EmbeddingUsageRecord, EmbeddingUsageStats, LlmUsageRecord, UsageStats,
    get_embedding_usage_summary, get_llm_usage_summary, insert_embedding_usage_sync,
    insert_llm_usage_sync, query_embedding_usage_stats, query_llm_usage_stats,
};

// All database access goes through DatabasePool (db::pool).
// All functions are available as _sync variants that take &Connection directly.

/// Filter-map helper that logs row parse errors instead of silently swallowing them.
/// Use in place of `.filter_map(|r| r.ok())` on query_map iterators.
pub(crate) fn log_and_discard<T>(result: rusqlite::Result<T>) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("Row parse error: {}", e);
            None
        }
    }
}

/// Shared SQL fragment for ordering by priority (urgent > high > medium > low > rest).
/// Append to ORDER BY clauses to keep priority ranking consistent across modules.
pub const PRIORITY_ORDER_SQL: &str = "CASE priority WHEN 'urgent' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 WHEN 'low' THEN 4 ELSE 5 END";

/// Parsed status filter supporting negation (e.g. "!completed" → exclude completed).
pub struct StatusFilter<'a> {
    pub value: Option<&'a str>,
    pub negate: bool,
}

impl<'a> StatusFilter<'a> {
    /// Parse an optional status filter string, handling "!" prefix for negation.
    pub fn parse(filter: Option<&'a str>) -> Self {
        match filter {
            Some(s) => match s.strip_prefix('!') {
                Some(rest) => Self {
                    value: Some(rest),
                    negate: true,
                },
                None => Self {
                    value: Some(s),
                    negate: false,
                },
            },
            None => Self {
                value: None,
                negate: false,
            },
        }
    }

    /// Returns the SQL operator for this filter: "!=" if negated, "=" otherwise.
    pub fn sql_op(&self) -> &'static str {
        if self.negate { "!=" } else { "=" }
    }
}

/// Map a priority string to a numeric score (1.0 = urgent, 0.4 = low).
pub fn priority_score(priority: &str) -> f64 {
    match priority {
        "urgent" => 1.0,
        "high" => 0.85,
        "medium" => 0.6,
        "low" => 0.4,
        _ => 0.5,
    }
}
