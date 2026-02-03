// crates/mira-server/src/background/code_health/mod.rs
// Background worker for detecting code health issues using concrete signals

mod analysis;
mod cargo;
pub mod conventions;
pub mod dependencies;
mod detection;
pub mod patterns;
pub mod scoring;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreMemoryParams, clear_old_health_issues_sync, get_indexed_project_ids_sync,
    get_project_paths_by_ids_sync, get_scan_info_sync, get_unused_functions_sync,
    is_time_older_than_sync, mark_health_scanned_sync, memory_key_exists_sync, store_memory_sync,
};
use crate::llm::LlmClient;
use crate::utils::ResultExt;
use std::path::Path;
use std::sync::Arc;

/// Maximum unused function findings per scan
const MAX_UNUSED_FINDINGS: usize = 10;
/// Confidence for unused function findings
const CONFIDENCE_UNUSED: f64 = 0.5;

/// Check code health for all indexed projects.
///
/// - `main_pool`: for reading/writing memory_facts, health markers
/// - `code_pool`: for reading code_symbols, call_graph
pub async fn process_code_health(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    tracing::debug!("Code health: checking for projects needing scan");

    // Step 1: Get indexed project IDs from code DB
    let project_ids = code_pool
        .interact(move |conn| {
            get_indexed_project_ids_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    if project_ids.is_empty() {
        return Ok(0);
    }

    // Step 2: Get project paths from main DB and filter by health check needs
    let ids = project_ids.clone();
    let projects = main_pool
        .interact(move |conn| {
            let all_projects =
                get_project_paths_by_ids_sync(conn, &ids).map_err(|e| anyhow::anyhow!("{}", e))?;

            let mut needing_scan = Vec::new();
            for (project_id, project_path) in all_projects {
                if needs_health_scan(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))? {
                    needing_scan.push((project_id, project_path));
                    break; // One project per cycle
                }
            }
            Ok::<_, anyhow::Error>(needing_scan)
        })
        .await
        .str_err()?;

    if !projects.is_empty() {
        tracing::info!(
            "Code health: found {} projects needing scan",
            projects.len()
        );
    }

    let mut processed = 0;

    for (project_id, project_path) in projects {
        if !Path::new(&project_path).exists() {
            continue;
        }

        match scan_project_health(main_pool, code_pool, client, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Found {} health issues for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;
                main_pool
                    .interact(move |conn| {
                        mark_health_scanned(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
                    })
                    .await
                    .str_err()?;
            }
            Err(e) => {
                tracing::warn!("Failed to scan health for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
}

/// Check if project needs health scanning
/// Triggers when:
/// 1. Never scanned before
/// 2. Files changed (health_scan_needed flag is set)
/// 3. Fallback: > 1 day since last scan
fn needs_health_scan(conn: &rusqlite::Connection, project_id: i64) -> Result<bool, String> {
    // Check if the "needs scan" flag is set (triggered by file watcher)
    if memory_key_exists_sync(conn, project_id, "health_scan_needed") {
        return Ok(true);
    }

    // Check last scan time for fallback
    let scan_info = get_scan_info_sync(conn, project_id, "health_scan_time");

    match scan_info {
        None => Ok(true), // Never scanned
        Some((_, scan_time)) => {
            // Fallback: rescan if > 1 day old
            Ok(is_time_older_than_sync(conn, &scan_time, "-1 day"))
        }
    }
}

/// Mark project as health-scanned and clear the "needs scan" flag
fn mark_health_scanned(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    mark_health_scanned_sync(conn, project_id).str_err()
}

/// Mark project as needing a health scan (called by file watcher)
/// Sync version for pool.interact()
pub fn mark_health_scan_needed_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    store_memory_sync(
        conn,
        StoreMemoryParams {
            project_id: Some(project_id),
            key: Some("health_scan_needed"),
            content: "pending",
            fact_type: "system",
            category: Some("health"),
            confidence: 1.0,
            session_id: None,
            user_id: None,
            scope: "project",
            branch: None,
        },
    )
    .str_err()?;
    tracing::debug!("Marked project {} for health rescan", project_id);
    Ok(())
}

/// Scan a project for health issues.
///
/// - `main_pool`: for writing findings to memory_facts
/// - `code_pool`: for reading code_symbols/call_graph
async fn scan_project_health(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    tracing::info!("Code health: scanning project {}", project_path);

    // Clear old health issues (main DB)
    main_pool
        .interact(move |conn| {
            clear_old_health_issues(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    let mut total = 0;

    // Run file-based detection operations using main_pool (they only write to memory_facts)
    let project_path_owned = project_path.to_string();
    let detection_results = main_pool
        .interact(
            move |conn| -> Result<(usize, usize, usize, usize, usize), anyhow::Error> {
                // 1. Cargo check warnings (most important)
                tracing::debug!(
                    "Code health: running cargo check for {}",
                    project_path_owned
                );
                let warnings = cargo::scan_cargo_warnings(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if warnings > 0 {
                    tracing::info!("Code health: found {} cargo warnings", warnings);
                }

                // 2. TODO/FIXME comments
                let todos = detection::scan_todo_comments(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if todos > 0 {
                    tracing::info!("Code health: found {} TODOs", todos);
                }

                // 3. Unimplemented macros
                let unimpl = detection::scan_unimplemented(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if unimpl > 0 {
                    tracing::info!("Code health: found {} unimplemented! macros", unimpl);
                }

                // 4. Unwrap/expect audit (panic risks)
                let unwraps = detection::scan_unwrap_usage(conn, project_id, &project_path_owned)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if unwraps > 0 {
                    tracing::info!(
                        "Code health: found {} unwrap/expect calls in non-test code",
                        unwraps
                    );
                }

                // 5. Error handling quality (pattern-based)
                let error_handling =
                    detection::scan_error_handling(conn, project_id, &project_path_owned)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                if error_handling > 0 {
                    tracing::info!(
                        "Code health: found {} error handling issues",
                        error_handling
                    );
                }

                Ok((warnings, todos, unimpl, unwraps, error_handling))
            },
        )
        .await
        .str_err()?;

    let (warnings, todos, unimpl, unwraps, error_handling) = detection_results;
    total += warnings + todos + unimpl + unwraps + error_handling;

    // 6. Unused functions - reads from code DB (code_symbols/call_graph),
    //    writes findings to main DB (memory_facts)
    let unused = scan_unused_functions_sharded(main_pool, code_pool, project_id).await?;
    if unused > 0 {
        tracing::info!("Code health: found {} potentially unused functions", unused);
    }
    total += unused;

    // 7. LLM complexity analysis (for large functions) - async
    if let Some(llm) = client {
        let complexity =
            analysis::scan_complexity(code_pool, main_pool, llm, project_id, project_path).await?;
        if complexity > 0 {
            tracing::info!(
                "Code health: found {} complexity issues via LLM",
                complexity
            );
        }
        total += complexity;

        // 8. LLM error handling analysis (for functions with many ? operators) - async
        let error_quality =
            analysis::scan_error_quality(code_pool, main_pool, llm, project_id, project_path)
                .await?;
        if error_quality > 0 {
            tracing::info!(
                "Code health: found {} error quality issues via LLM",
                error_quality
            );
        }
        total += error_quality;
    }

    // 9. Module dependency analysis + circular dependency detection
    let dep_count =
        scan_dependencies_sharded(main_pool, code_pool, project_id).await?;
    if dep_count > 0 {
        tracing::info!("Code health: computed {} module dependency edges", dep_count);
    }
    total += dep_count;

    // 10. Architectural pattern detection
    let pattern_count =
        scan_patterns_sharded(main_pool, code_pool, project_id).await?;
    if pattern_count > 0 {
        tracing::info!("Code health: detected {} architectural patterns", pattern_count);
    }
    total += pattern_count;

    // 11. Tech debt scoring (runs last, aggregates all findings)
    match scoring::compute_tech_debt_scores(main_pool, code_pool, project_id).await {
        Ok(scored) => {
            if scored > 0 {
                tracing::info!("Code health: computed tech debt scores for {} modules", scored);
            }
        }
        Err(e) => {
            tracing::warn!("Code health: tech debt scoring failed: {}", e);
        }
    }

    // 12. Convention extraction (for context-aware suggestions)
    match scan_conventions_sharded(main_pool, code_pool, project_id).await {
        Ok(count) => {
            if count > 0 {
                tracing::info!("Code health: extracted conventions for {} modules", count);
            }
        }
        Err(e) => {
            tracing::warn!("Code health: convention extraction failed: {}", e);
        }
    }

    Ok(total)
}

/// Scan module dependencies using sharded pools.
async fn scan_dependencies_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Need both connections simultaneously — get code conn first, then main conn
    let code_conn_result = code_pool
        .run(move |code_conn| {
            // We need main_conn too, but we can't nest pool calls.
            // Collect the data we need from code DB first.
            // Actually, dependencies::analyze_module_dependencies needs both conns.
            // Let's collect module info from code DB, then do the analysis.
            Ok::<_, String>(collect_dependency_data(code_conn, project_id))
        })
        .await?;

    let dep_data = code_conn_result?;

    if dep_data.is_empty() {
        return Ok(0);
    }

    // Store dependency edges in code DB
    let edges = dep_data.len();
    let dep_data_for_code = dep_data.clone();
    code_pool
        .run(move |conn| {
            use crate::db::dependencies::{clear_module_dependencies_sync, upsert_module_dependency_sync, ModuleDependency};
            clear_module_dependencies_sync(conn, project_id).str_err()?;
            for d in &dep_data_for_code {
                let dep = ModuleDependency {
                    source_module_id: d.source.clone(),
                    target_module_id: d.target.clone(),
                    dependency_type: d.dep_type.clone(),
                    call_count: d.call_count,
                    import_count: d.import_count,
                    is_circular: d.is_circular,
                };
                upsert_module_dependency_sync(conn, project_id, &dep).str_err()?;
            }
            Ok::<_, String>(())
        })
        .await?;

    // Store circular dependency findings in main DB
    let circular_findings: Vec<_> = dep_data.iter().filter(|d| d.is_circular).collect();
    if !circular_findings.is_empty() {
        let findings = circular_findings
            .iter()
            .map(|d| (d.source.clone(), d.target.clone()))
            .collect::<Vec<_>>();
        main_pool
            .run(move |conn| {
                for (src, tgt) in &findings {
                    let key = format!("health:circular:{}:{}", src, tgt);
                    let content = format!(
                        "[circular-dependency] Circular dependency: {} <-> {}",
                        src, tgt
                    );
                    store_memory_sync(
                        conn,
                        StoreMemoryParams {
                            project_id: Some(project_id),
                            key: Some(&key),
                            content: &content,
                            fact_type: "health",
                            category: Some("circular_dependency"),
                            confidence: 0.9,
                            session_id: None,
                            user_id: None,
                            scope: "project",
                            branch: None,
                        },
                    )
                    .str_err()?;
                }
                Ok::<_, String>(())
            })
            .await?;
    }

    Ok(edges)
}

/// Intermediate dependency data that can be sent between pool closures
#[derive(Clone)]
struct DepEdge {
    source: String,
    target: String,
    dep_type: String,
    call_count: i64,
    import_count: i64,
    is_circular: bool,
}

/// Collect dependency data from code DB (runs inside pool.run)
fn collect_dependency_data(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<Vec<DepEdge>, String> {
    use dependencies::tarjan_scc;
    use std::collections::HashMap;

    // Get modules
    let mut stmt = conn
        .prepare("SELECT module_id, path FROM codebase_modules WHERE project_id = ?")
        .str_err()?;
    let modules: Vec<(String, String)> = stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    if modules.is_empty() {
        return Ok(Vec::new());
    }

    // Map file paths to modules
    let file_to_mod = |file_path: &str| -> Option<String> {
        modules
            .iter()
            .filter(|(_, path)| file_path.starts_with(path.as_str()) || file_path.contains(path.as_str()))
            .max_by_key(|(_, path)| path.len())
            .map(|(id, _)| id.clone())
    };

    // Count import deps
    let mut import_deps: HashMap<(String, String), i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT file_path, import_path FROM imports WHERE project_id = ? AND is_external = 0")
            .str_err()?;
        let rows = stmt
            .query_map([project_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .str_err()?;
        for row in rows {
            let (fp, ip) = row.str_err()?;
            if let (Some(src), Some(tgt)) = (file_to_mod(&fp), file_to_mod(&ip)) {
                if src != tgt {
                    *import_deps.entry((src, tgt)).or_default() += 1;
                }
            }
        }
    }

    // Count call deps
    let mut call_deps: HashMap<(String, String), i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT cs1.file_path, cs2.file_path, cg.call_count
                 FROM call_graph cg
                 JOIN code_symbols cs1 ON cg.caller_id = cs1.id
                 JOIN code_symbols cs2 ON cg.callee_id = cs2.id
                 WHERE cs1.project_id = ? AND cs2.project_id = ?",
            )
            .str_err()?;
        let rows = stmt
            .query_map([project_id, project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
            })
            .str_err()?;
        for row in rows {
            let (f1, f2, cnt) = row.str_err()?;
            if let (Some(src), Some(tgt)) = (file_to_mod(&f1), file_to_mod(&f2)) {
                if src != tgt {
                    *call_deps.entry((src, tgt)).or_default() += cnt;
                }
            }
        }
    }

    // Merge
    let mut merged: HashMap<(String, String), (i64, i64)> = HashMap::new();
    for ((src, tgt), count) in &import_deps {
        merged.entry((src.clone(), tgt.clone())).or_default().1 = *count;
    }
    for ((src, tgt), count) in &call_deps {
        merged.entry((src.clone(), tgt.clone())).or_default().0 = *count;
    }

    if merged.is_empty() {
        return Ok(Vec::new());
    }

    // Tarjan's SCC for circular detection
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for (src, tgt) in merged.keys() {
        adj.entry(src.clone()).or_default().push(tgt.clone());
        adj.entry(tgt.clone()).or_default();
    }
    let sccs = tarjan_scc(&adj);
    let mut circular_edges: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for scc in &sccs {
        for a in scc {
            for b in scc {
                if a != b && merged.contains_key(&(a.clone(), b.clone())) {
                    circular_edges.insert((a.clone(), b.clone()));
                }
            }
        }
    }

    // Build result
    let result: Vec<DepEdge> = merged
        .iter()
        .map(|((src, tgt), (calls, imports))| {
            let dep_type = match (calls > &0, imports > &0) {
                (true, true) => "both",
                (true, false) => "call",
                (false, true) => "import",
                (false, false) => "import",
            };
            DepEdge {
                source: src.clone(),
                target: tgt.clone(),
                dep_type: dep_type.to_string(),
                call_count: *calls,
                import_count: *imports,
                is_circular: circular_edges.contains(&(src.clone(), tgt.clone())),
            }
        })
        .collect();

    Ok(result)
}

/// Scan architectural patterns using sharded pools.
async fn scan_patterns_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Run pattern detection on code DB
    let pattern_findings = code_pool
        .run(move |conn| patterns::collect_pattern_data(conn, project_id))
        .await?;

    if pattern_findings.is_empty() {
        return Ok(0);
    }

    let count = pattern_findings.len();

    // Store pattern findings in main DB (memory_facts)
    main_pool
        .run(move |conn| {
            for finding in &pattern_findings {
                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&finding.key),
                        content: &finding.content,
                        fact_type: "health",
                        category: Some("architecture"),
                        confidence: finding.confidence,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .str_err()?;
            }
            Ok::<_, String>(())
        })
        .await?;

    Ok(count)
}

/// Scan coding conventions using sharded pools.
/// Reads code_chunks/imports/symbols from code_pool, writes to module_conventions in main_pool.
async fn scan_conventions_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Collect convention data from code DB
    let convention_data = code_pool
        .run(move |conn| conventions::collect_convention_data(conn, project_id))
        .await?;

    if convention_data.is_empty() {
        return Ok(0);
    }

    let count = convention_data.len();

    // Collect module paths for marking as extracted
    let module_paths: Vec<String> = convention_data.iter().map(|d| d.module_path.clone()).collect();

    // Store convention data in main DB
    main_pool
        .run(move |conn| conventions::upsert_module_conventions(conn, project_id, &convention_data))
        .await?;

    // Mark modules as extracted in code DB
    code_pool
        .run(move |conn| conventions::mark_conventions_extracted(conn, project_id, &module_paths))
        .await?;

    Ok(count)
}

/// Scan for unused functions using sharded pools.
/// Reads code_symbols/call_graph from code_pool, writes findings to main_pool.
async fn scan_unused_functions_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Read unused functions from code DB
    let unused = code_pool
        .interact(move |conn| {
            get_unused_functions_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    if unused.is_empty() {
        return Ok(0);
    }

    // Write findings to main DB
    let count = main_pool
        .interact(move |conn| -> Result<usize, anyhow::Error> {
            let mut stored = 0;
            for (name, file_path, line) in &unused {
                let content = format!(
                    "[unused] Function `{}` at {}:{} appears to have no callers",
                    name, file_path, line
                );
                let key = format!("health:unused:{}:{}", file_path, name);

                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: Some(project_id),
                        key: Some(&key),
                        content: &content,
                        fact_type: "health",
                        category: Some("unused"),
                        confidence: CONFIDENCE_UNUSED,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;

                stored += 1;
                if stored >= MAX_UNUSED_FINDINGS {
                    break;
                }
            }
            Ok(stored)
        })
        .await
        .str_err()?;

    Ok(count)
}

/// Clear old health issues before refresh
fn clear_old_health_issues(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    clear_old_health_issues_sync(conn, project_id).str_err()
}
