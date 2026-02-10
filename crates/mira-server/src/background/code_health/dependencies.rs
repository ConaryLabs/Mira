// background/code_health/dependencies.rs
// Module dependency analysis + circular dependency detection using Tarjan's SCC

use crate::db::dependencies::{
    ModuleDependency, clear_module_dependencies_sync, upsert_module_dependency_sync,
};
use crate::db::pool::DatabasePool;
use crate::db::{StoreMemoryParams, store_memory_sync};
use crate::utils::ResultExt;
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Sharded scan (async entry point, called from mod.rs orchestration)
// ============================================================================

/// Scan module dependencies using sharded pools.
pub(super) async fn scan_dependencies_sharded(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Need both connections simultaneously — get code conn first, then main conn
    let code_conn_result = code_pool
        .run(move |code_conn| Ok::<_, String>(collect_dependency_data(code_conn, project_id)))
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
                            team_id: None,
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

// ============================================================================
// Dependency data collection (sync, runs inside pool.run)
// ============================================================================

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
    // Get modules
    let mut stmt = conn
        .prepare("SELECT module_id, path FROM codebase_modules WHERE project_id = ?")
        .str_err()?;
    let modules: Vec<(String, String)> = stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .str_err()?
        .filter_map(crate::db::log_and_discard)
        .collect();

    if modules.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-sort modules by path length descending so longest (most specific) match wins first
    let mut modules_sorted = modules;
    modules_sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    // Cache file_path -> module_id lookups to avoid repeated linear scans
    let mut file_mod_cache: HashMap<String, Option<String>> = HashMap::new();
    let resolve_mod =
        |cache: &mut HashMap<String, Option<String>>, file_path: &str| -> Option<String> {
            if let Some(cached) = cache.get(file_path) {
                return cached.clone();
            }
            let result = modules_sorted
                .iter()
                .find(|(_, path)| {
                    file_path.starts_with(path.as_str()) || file_path.contains(path.as_str())
                })
                .map(|(id, _)| id.clone());
            cache.insert(file_path.to_string(), result.clone());
            result
        };

    // Count import deps
    let mut import_deps: HashMap<(String, String), i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT file_path, import_path FROM imports WHERE project_id = ? AND is_external = 0")
            .str_err()?;
        let rows = stmt
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .str_err()?;
        for row in rows {
            let (fp, ip) = row.str_err()?;
            if let (Some(src), Some(tgt)) = (
                resolve_mod(&mut file_mod_cache, &fp),
                resolve_mod(&mut file_mod_cache, &ip),
            ) && src != tgt
            {
                *import_deps.entry((src, tgt)).or_default() += 1;
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
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .str_err()?;
        for row in rows {
            let (f1, f2, cnt) = row.str_err()?;
            if let (Some(src), Some(tgt)) = (
                resolve_mod(&mut file_mod_cache, &f1),
                resolve_mod(&mut file_mod_cache, &f2),
            ) && src != tgt
            {
                *call_deps.entry((src, tgt)).or_default() += cnt;
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
    let mut circular_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
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

// ============================================================================
// Tarjan's SCC algorithm
// ============================================================================

/// Tarjan's strongly connected components algorithm.
/// Returns groups of size > 1 (circular dependencies).
pub fn tarjan_scc(adj: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    struct State {
        index_counter: usize,
        stack: Vec<String>,
        on_stack: std::collections::HashSet<String>,
        index: HashMap<String, usize>,
        lowlink: HashMap<String, usize>,
        result: Vec<Vec<String>>,
    }

    fn strongconnect(v: &str, adj: &HashMap<String, Vec<String>>, state: &mut State) {
        state.index.insert(v.to_string(), state.index_counter);
        state.lowlink.insert(v.to_string(), state.index_counter);
        state.index_counter += 1;
        state.stack.push(v.to_string());
        state.on_stack.insert(v.to_string());

        if let Some(neighbors) = adj.get(v) {
            for w in neighbors {
                if !state.index.contains_key(w.as_str()) {
                    strongconnect(w, adj, state);
                    let w_low = state.lowlink[w.as_str()];
                    let v_low = state.lowlink[v];
                    if w_low < v_low {
                        state.lowlink.insert(v.to_string(), w_low);
                    }
                } else if state.on_stack.contains(w.as_str()) {
                    let w_idx = state.index[w.as_str()];
                    let v_low = state.lowlink[v];
                    if w_idx < v_low {
                        state.lowlink.insert(v.to_string(), w_idx);
                    }
                }
            }
        }

        // If v is a root node, pop the SCC
        if state.lowlink[v] == state.index[v] {
            let mut scc = Vec::new();
            loop {
                // Safety: Tarjan's algorithm guarantees stack is non-empty here
                let Some(w) = state.stack.pop() else {
                    break;
                };
                state.on_stack.remove(&w);
                scc.push(w.clone());
                if w == v {
                    break;
                }
            }
            // Only keep SCCs with size > 1 (actual cycles)
            if scc.len() > 1 {
                scc.sort();
                state.result.push(scc);
            }
        }
    }

    let mut state = State {
        index_counter: 0,
        stack: Vec::new(),
        on_stack: std::collections::HashSet::new(),
        index: HashMap::new(),
        lowlink: HashMap::new(),
        result: Vec::new(),
    };

    for v in adj.keys() {
        if !state.index.contains_key(v.as_str()) {
            strongconnect(v, adj, &mut state);
        }
    }

    state.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // ═══════════════════════════════════════════════════════════════════════════
    // Tarjan's SCC tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_tarjan_no_cycles() {
        let mut adj = HashMap::new();
        adj.insert("a".to_string(), vec!["b".to_string()]);
        adj.insert("b".to_string(), vec!["c".to_string()]);
        adj.insert("c".to_string(), vec![]);

        let sccs = tarjan_scc(&adj);
        assert!(sccs.is_empty(), "DAG should have no cycles");
    }

    #[test]
    fn test_tarjan_simple_cycle() {
        let mut adj = HashMap::new();
        adj.insert("a".to_string(), vec!["b".to_string()]);
        adj.insert("b".to_string(), vec!["a".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 2);
        assert!(sccs[0].contains(&"a".to_string()));
        assert!(sccs[0].contains(&"b".to_string()));
    }

    #[test]
    fn test_tarjan_three_node_cycle() {
        let mut adj = HashMap::new();
        adj.insert("a".to_string(), vec!["b".to_string()]);
        adj.insert("b".to_string(), vec!["c".to_string()]);
        adj.insert("c".to_string(), vec!["a".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 3);
    }

    #[test]
    fn test_tarjan_multiple_sccs() {
        let mut adj = HashMap::new();
        // Cycle 1: a <-> b
        adj.insert("a".to_string(), vec!["b".to_string()]);
        adj.insert("b".to_string(), vec!["a".to_string(), "c".to_string()]);
        // Cycle 2: c <-> d
        adj.insert("c".to_string(), vec!["d".to_string()]);
        adj.insert("d".to_string(), vec!["c".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 2);
    }

    #[test]
    fn test_tarjan_empty_graph() {
        let adj: HashMap<String, Vec<String>> = HashMap::new();
        let sccs = tarjan_scc(&adj);
        assert!(sccs.is_empty());
    }

    #[test]
    fn test_tarjan_self_loop_not_counted() {
        // Self-loops create SCCs of size 1, which we filter out
        let mut adj = HashMap::new();
        adj.insert("a".to_string(), vec!["a".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert!(
            sccs.is_empty(),
            "Self-loops should not be reported as circular deps"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // collect_dependency_data tests
    // ═══════════════════════════════════════════════════════════════════════════

    /// Create a connection with code DB tables for collect_dependency_data tests.
    fn setup_code_db() -> Connection {
        crate::db::pool::ensure_sqlite_vec_registered();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_symbols (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                signature TEXT,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS call_graph (
                id INTEGER PRIMARY KEY,
                caller_id INTEGER REFERENCES code_symbols(id),
                callee_name TEXT NOT NULL,
                callee_id INTEGER REFERENCES code_symbols(id),
                call_count INTEGER DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS imports (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                import_path TEXT NOT NULL,
                is_external INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS codebase_modules (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                module_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                UNIQUE(project_id, module_id)
            );",
        )
        .unwrap();
        conn
    }

    fn insert_module(conn: &Connection, project_id: i64, module_id: &str, path: &str) {
        conn.execute(
            "INSERT INTO codebase_modules (project_id, module_id, name, path) VALUES (?1, ?2, ?2, ?3)",
            rusqlite::params![project_id, module_id, path],
        )
        .unwrap();
    }

    fn insert_import(conn: &Connection, project_id: i64, file_path: &str, import_path: &str) {
        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external) VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![project_id, file_path, import_path],
        )
        .unwrap();
    }

    fn insert_symbol(conn: &Connection, project_id: i64, name: &str, file_path: &str) -> i64 {
        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
             VALUES (?1, ?2, ?3, 'function', 1, 10)",
            rusqlite::params![project_id, file_path, name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_call(conn: &Connection, caller_id: i64, callee_id: i64, call_count: i64) {
        conn.execute(
            "INSERT INTO call_graph (caller_id, callee_name, callee_id, call_count)
             VALUES (?1, 'callee', ?2, ?3)",
            rusqlite::params![caller_id, callee_id, call_count],
        )
        .unwrap();
    }

    #[test]
    fn test_collect_deps_no_modules() {
        let conn = setup_code_db();
        let result = collect_dependency_data(&conn, 1).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_deps_modules_but_no_edges() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        let result = collect_dependency_data(&conn, 1).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_deps_import_only() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        insert_import(&conn, 1, "src/a/main.rs", "src/b/utils.rs");

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, "mod_a");
        assert_eq!(result[0].target, "mod_b");
        assert_eq!(result[0].dep_type, "import");
        assert_eq!(result[0].import_count, 1);
        assert_eq!(result[0].call_count, 0);
        assert!(!result[0].is_circular);
    }

    #[test]
    fn test_collect_deps_call_only() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        let caller = insert_symbol(&conn, 1, "foo", "src/a/main.rs");
        let callee = insert_symbol(&conn, 1, "bar", "src/b/lib.rs");
        insert_call(&conn, caller, callee, 3);

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dep_type, "call");
        assert_eq!(result[0].call_count, 3);
        assert_eq!(result[0].import_count, 0);
    }

    #[test]
    fn test_collect_deps_both_import_and_call() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        insert_import(&conn, 1, "src/a/main.rs", "src/b/utils.rs");
        let caller = insert_symbol(&conn, 1, "foo", "src/a/main.rs");
        let callee = insert_symbol(&conn, 1, "bar", "src/b/lib.rs");
        insert_call(&conn, caller, callee, 2);

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dep_type, "both");
        assert_eq!(result[0].call_count, 2);
        assert_eq!(result[0].import_count, 1);
    }

    #[test]
    fn test_collect_deps_circular() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        // A -> B and B -> A
        insert_import(&conn, 1, "src/a/main.rs", "src/b/utils.rs");
        insert_import(&conn, 1, "src/b/main.rs", "src/a/utils.rs");

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 2);
        for edge in &result {
            assert!(
                edge.is_circular,
                "Edge {} -> {} should be circular",
                edge.source, edge.target
            );
        }
    }

    #[test]
    fn test_collect_deps_same_module_ignored() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        // Import within same module
        insert_import(&conn, 1, "src/a/main.rs", "src/a/utils.rs");

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert!(result.is_empty(), "Intra-module deps should be ignored");
    }

    #[test]
    fn test_collect_deps_external_imports_excluded() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (1, 'src/a/main.rs', 'serde::Serialize', 1)",
            [],
        )
        .unwrap();

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_deps_project_isolation() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        insert_import(&conn, 1, "src/a/main.rs", "src/b/utils.rs");

        // Query for a different project
        let result = collect_dependency_data(&conn, 2).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_deps_multiple_imports_aggregated() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        // Multiple files in A import from B
        insert_import(&conn, 1, "src/a/foo.rs", "src/b/utils.rs");
        insert_import(&conn, 1, "src/a/bar.rs", "src/b/types.rs");

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].import_count, 2);
    }

    #[test]
    fn test_collect_deps_three_node_cycle() {
        let conn = setup_code_db();
        insert_module(&conn, 1, "mod_a", "src/a");
        insert_module(&conn, 1, "mod_b", "src/b");
        insert_module(&conn, 1, "mod_c", "src/c");
        // A -> B -> C -> A
        insert_import(&conn, 1, "src/a/main.rs", "src/b/lib.rs");
        insert_import(&conn, 1, "src/b/main.rs", "src/c/lib.rs");
        insert_import(&conn, 1, "src/c/main.rs", "src/a/lib.rs");

        let result = collect_dependency_data(&conn, 1).unwrap();
        assert_eq!(result.len(), 3);
        let circular_count = result.iter().filter(|e| e.is_circular).count();
        assert_eq!(circular_count, 3, "All edges in cycle should be circular");
    }

    #[test]
    fn test_collect_deps_longest_path_match() {
        let conn = setup_code_db();
        // Module with longer path should match more specifically
        insert_module(&conn, 1, "mod_parent", "src");
        insert_module(&conn, 1, "mod_child", "src/sub");
        // File in src/sub/ should match mod_child, not mod_parent
        insert_import(&conn, 1, "src/sub/main.rs", "src/other.rs");
        insert_module(&conn, 1, "mod_other", "src/other");

        let result = collect_dependency_data(&conn, 1).unwrap();
        // Should have mod_child -> mod_other, not mod_parent -> mod_other
        let edge = result
            .iter()
            .find(|e| e.target == "mod_other")
            .expect("Expected edge targeting mod_other");
        assert_eq!(edge.source, "mod_child", "Should match longest path prefix");
    }
}
