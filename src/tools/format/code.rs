// src/tools/format/code.rs
// Formatters for code intelligence (symbols, commits, call graph, search)

use serde_json::Value;

/// Format index status
pub fn index_status(action: &str, path: &str, stats: Option<&Value>) -> String {
    match stats {
        Some(s) => {
            let files = s.get("files_indexed").and_then(|v| v.as_i64()).unwrap_or(0);
            let symbols = s.get("symbols_indexed").and_then(|v| v.as_i64()).unwrap_or(0);
            let commits = s.get("commits_indexed").and_then(|v| v.as_i64()).unwrap_or(0);

            if commits > 0 {
                format!("Indexed {}: {} files, {} symbols, {} commits", path, files, symbols, commits)
            } else if symbols > 0 {
                format!("Indexed {}: {} files, {} symbols", path, files, symbols)
            } else {
                format!("Indexed {}: {} files", path, files)
            }
        }
        None => format!("{} complete: {}", action, path),
    }
}

/// Format code search results
pub fn code_search_results(results: &[Value]) -> String {
    if results.is_empty() {
        return "No matches.".to_string();
    }

    let mut out = format!("Found {} match{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "es" }
    );

    for r in results {
        let file = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let symbol = r.get("symbol_name").and_then(|v| v.as_str());
        let score = r.get("score").and_then(|v| v.as_f64());

        let rel = score.map(|s| format!(" [{:.0}%]", s * 100.0)).unwrap_or_default();

        match symbol {
            Some(sym) => out.push_str(&format!("  {}:{}{}\n", file, sym, rel)),
            None => out.push_str(&format!("  {}{}\n", file, rel)),
        }
    }

    out.trim_end().to_string()
}

/// Format symbols list
pub fn symbols_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No symbols.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = r.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let line = r.get("line").and_then(|v| v.as_i64());

        let loc = line.map(|l| format!(":{}", l)).unwrap_or_default();
        out.push_str(&format!("  {} ({}){}\n", name, kind, loc));
    }

    out.trim_end().to_string()
}

/// Format commit list
pub fn commit_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No commits.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let hash = r.get("hash").and_then(|v| v.as_str()).unwrap_or("?");
        let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("?");
        let author = r.get("author").and_then(|v| v.as_str());

        let short_hash = if hash.len() > 7 { &hash[..7] } else { hash };
        let short_msg = if message.len() > 60 { format!("{}...", &message[..57]) } else { message.to_string() };

        match author {
            Some(a) => out.push_str(&format!("  {} {} ({})\n", short_hash, short_msg, a)),
            None => out.push_str(&format!("  {} {}\n", short_hash, short_msg)),
        }
    }

    out.trim_end().to_string()
}

/// Format related files
pub fn related_files(results: &[Value]) -> String {
    if results.is_empty() {
        return "No related files.".to_string();
    }

    let mut out = format!("{} related file{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let path = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let rel_type = r.get("relation_type").and_then(|v| v.as_str()).unwrap_or("?");
        let score = r.get("score").and_then(|v| v.as_f64());

        let rel = score.map(|s| format!(" ({:.0}%)", s * 100.0)).unwrap_or_default();
        out.push_str(&format!("  {} [{}]{}\n", path, rel_type, rel));
    }

    out.trim_end().to_string()
}

/// Format call graph
pub fn call_graph(results: &[Value]) -> String {
    if results.is_empty() {
        return "No call graph data.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let caller = r.get("caller").and_then(|v| v.as_str()).unwrap_or("?");
        let callee = r.get("callee").and_then(|v| v.as_str()).unwrap_or("?");
        out.push_str(&format!("  {} → {}\n", caller, callee));
    }

    out.trim_end().to_string()
}
