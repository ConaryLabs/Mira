// background/code_health/dependencies.rs
// Module dependency analysis + circular dependency detection using Tarjan's SCC

use crate::db::dependencies::{ModuleDependency, clear_module_dependencies_sync, upsert_module_dependency_sync};
use crate::db::{StoreMemoryParams, store_memory_sync};
use crate::utils::ResultExt;
use rusqlite::Connection;
use std::collections::HashMap;

/// Analyze module dependencies from imports + call_graph + code_symbols + codebase_modules.
/// Returns the number of dependency edges found.
pub fn analyze_module_dependencies(
    code_conn: &Connection,
    main_conn: &Connection,
    project_id: i64,
) -> Result<usize, String> {
    // Step 1: Build module lookup from codebase_modules (file_path prefix -> module_id)
    let modules = get_module_paths(code_conn, project_id)?;
    if modules.is_empty() {
        return Ok(0);
    }

    // Step 2: Count import-based dependencies between modules
    let import_deps = count_import_dependencies(code_conn, project_id, &modules)?;

    // Step 3: Count call-based dependencies between modules
    let call_deps = count_call_dependencies(code_conn, project_id, &modules)?;

    // Step 4: Merge import + call edges
    let mut merged: HashMap<(String, String), (i64, i64)> = HashMap::new();
    for ((src, tgt), count) in &import_deps {
        merged.entry((src.clone(), tgt.clone())).or_default().1 = *count;
    }
    for ((src, tgt), count) in &call_deps {
        merged.entry((src.clone(), tgt.clone())).or_default().0 = *count;
    }

    if merged.is_empty() {
        return Ok(0);
    }

    // Step 5: Detect circular dependencies via Tarjan's SCC
    let adj = build_adjacency(&merged);
    let sccs = tarjan_scc(&adj);

    // Build set of circular edges for O(1) lookup
    let mut circular_edges: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for scc in &sccs {
        // For all pairs in the SCC that have edges, mark them circular
        for a in scc {
            for b in scc {
                if a != b && merged.contains_key(&(a.clone(), b.clone())) {
                    circular_edges.insert((a.clone(), b.clone()));
                }
            }
        }
    }

    // Step 6: Store results
    clear_module_dependencies_sync(code_conn, project_id).str_err()?;

    let mut stored = 0;
    for ((src, tgt), (calls, imports)) in &merged {
        let is_circular = circular_edges.contains(&(src.clone(), tgt.clone()));
        let dep_type = match (calls > &0, imports > &0) {
            (true, true) => "both",
            (true, false) => "call",
            (false, true) => "import",
            (false, false) => continue,
        };

        let dep = ModuleDependency {
            source_module_id: src.clone(),
            target_module_id: tgt.clone(),
            dependency_type: dep_type.to_string(),
            call_count: *calls,
            import_count: *imports,
            is_circular,
        };

        upsert_module_dependency_sync(code_conn, project_id, &dep)
            .str_err()?;
        stored += 1;
    }

    // Step 7: Store circular dependency findings in memory_facts
    for scc in &sccs {
        let modules_str = scc.join(" <-> ");
        let content = format!(
            "[circular-dependency] Circular dependency detected between modules: {}",
            modules_str
        );
        // Use first two modules for the key
        let key = if scc.len() >= 2 {
            format!("health:circular:{}:{}", scc[0], scc[1])
        } else {
            format!("health:circular:{}", scc[0])
        };

        store_memory_sync(
            main_conn,
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
        .map_err(|e| format!("Failed to store circular dep finding: {}", e))?;
    }

    if !sccs.is_empty() {
        tracing::info!(
            "Code health: found {} circular dependency groups ({} total edges)",
            sccs.len(),
            circular_edges.len()
        );
    }

    Ok(stored)
}

/// Module path info: (module_id, path_prefix)
struct ModuleInfo {
    module_id: String,
    path: String,
}

/// Get module id -> path mappings from codebase_modules
fn get_module_paths(conn: &Connection, project_id: i64) -> Result<Vec<ModuleInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT module_id, path FROM codebase_modules WHERE project_id = ?",
        )
        .str_err()?;

    let modules = stmt
        .query_map([project_id], |row| {
            Ok(ModuleInfo {
                module_id: row.get(0)?,
                path: row.get(1)?,
            })
        })
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Map a file_path to its owning module_id using longest prefix match
fn file_to_module<'a>(file_path: &str, modules: &'a [ModuleInfo]) -> Option<&'a str> {
    modules
        .iter()
        .filter(|m| file_path.starts_with(&m.path) || file_path.contains(&m.path))
        .max_by_key(|m| m.path.len())
        .map(|m| m.module_id.as_str())
}

/// Count import-based dependencies between modules
fn count_import_dependencies(
    conn: &Connection,
    project_id: i64,
    modules: &[ModuleInfo],
) -> Result<HashMap<(String, String), i64>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT file_path, import_path FROM imports
             WHERE project_id = ? AND is_external = 0",
        )
        .str_err()?;

    let mut deps: HashMap<(String, String), i64> = HashMap::new();

    let rows = stmt
        .query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .str_err()?;

    for row in rows {
        let (file_path, import_path) = row.str_err()?;
        let source_module = file_to_module(&file_path, modules);
        let target_module = file_to_module(&import_path, modules);

        if let (Some(src), Some(tgt)) = (source_module, target_module) {
            if src != tgt {
                *deps.entry((src.to_string(), tgt.to_string())).or_default() += 1;
            }
        }
    }

    Ok(deps)
}

/// Count call-based dependencies between modules via call_graph -> code_symbols -> codebase_modules
fn count_call_dependencies(
    conn: &Connection,
    project_id: i64,
    modules: &[ModuleInfo],
) -> Result<HashMap<(String, String), i64>, String> {
    // Join call_graph with code_symbols to get file paths for caller and callee
    let mut stmt = conn
        .prepare(
            "SELECT cs_caller.file_path, cs_callee.file_path, cg.call_count
             FROM call_graph cg
             JOIN code_symbols cs_caller ON cg.caller_id = cs_caller.id
             JOIN code_symbols cs_callee ON cg.callee_id = cs_callee.id
             WHERE cs_caller.project_id = ? AND cs_callee.project_id = ?",
        )
        .str_err()?;

    let mut deps: HashMap<(String, String), i64> = HashMap::new();

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
        let (caller_file, callee_file, call_count) = row.str_err()?;
        let source_module = file_to_module(&caller_file, modules);
        let target_module = file_to_module(&callee_file, modules);

        if let (Some(src), Some(tgt)) = (source_module, target_module) {
            if src != tgt {
                *deps.entry((src.to_string(), tgt.to_string())).or_default() += call_count;
            }
        }
    }

    Ok(deps)
}

/// Build adjacency list from merged dependency edges
fn build_adjacency(
    merged: &HashMap<(String, String), (i64, i64)>,
) -> HashMap<String, Vec<String>> {
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for (src, tgt) in merged.keys() {
        adj.entry(src.clone()).or_default().push(tgt.clone());
        adj.entry(tgt.clone()).or_default(); // ensure all nodes present
    }
    adj
}

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
                let w = state.stack.pop().unwrap();
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
        assert!(sccs.is_empty(), "Self-loops should not be reported as circular deps");
    }

    #[test]
    fn test_file_to_module_longest_prefix() {
        let modules = vec![
            ModuleInfo {
                module_id: "root".to_string(),
                path: "src/".to_string(),
            },
            ModuleInfo {
                module_id: "db".to_string(),
                path: "src/db/".to_string(),
            },
            ModuleInfo {
                module_id: "db_pool".to_string(),
                path: "src/db/pool/".to_string(),
            },
        ];

        assert_eq!(file_to_module("src/db/pool/mod.rs", &modules), Some("db_pool"));
        assert_eq!(file_to_module("src/db/memory.rs", &modules), Some("db"));
        assert_eq!(file_to_module("src/main.rs", &modules), Some("root"));
        assert_eq!(file_to_module("tests/foo.rs", &modules), None);
    }
}
