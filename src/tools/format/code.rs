// src/tools/format/code.rs
// Formatters for code intelligence (symbols, commits, call graph, search)

use serde_json::Value;
use crate::tools::code_intel::{CodeImprovement, StyleReport};

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

/// Format style report for codebase analysis
pub fn style_report(report: &StyleReport) -> String {
    if report.total_functions == 0 {
        return "No functions indexed yet. Run `index project` first.".to_string();
    }

    format!(
        "Codebase Style (match this when writing code):\n\
        - Average function: {:.1} lines\n\
        - Distribution: {}% short (<10), {}% medium (10-30), {}% long (>30)\n\
        - Total: {} functions ({} short, {} medium, {} long)\n\
        - Abstraction level: {} ({} traits, {} structs)\n\
        - Test coverage: {} test functions ({:.0}% of codebase)\n\
        - Suggested max function length: {} lines",
        report.avg_function_length,
        report.short_pct as i64,
        report.medium_pct as i64,
        report.long_pct as i64,
        report.total_functions,
        report.short_functions,
        report.medium_functions,
        report.long_functions,
        report.abstraction_level,
        report.trait_count,
        report.struct_count,
        report.test_functions,
        report.test_ratio * 100.0,
        report.suggested_max_length
    )
}

/// Format style report as concise context for LLM prompts
#[allow(dead_code)]
pub fn style_context(report: &StyleReport) -> String {
    if report.total_functions == 0 {
        return String::new();
    }

    format!(
        "**Codebase Style (match this):**\n\
        - Average function: {:.0} lines\n\
        - Distribution: {}% short, {}% medium, {}% long\n\
        - Abstraction level: {}\n\
        - Keep functions under {} lines unless complex logic requires more",
        report.avg_function_length,
        report.short_pct as i64,
        report.medium_pct as i64,
        report.long_pct as i64,
        report.abstraction_level,
        report.suggested_max_length
    )
}

/// Format improvement suggestions for proactive context
#[allow(dead_code)]
pub fn improvement_suggestions(improvements: &[CodeImprovement]) -> String {
    if improvements.is_empty() {
        return String::new();
    }

    let mut out = format!("Code improvements ({}):\n", improvements.len());
    for imp in improvements.iter().take(5) {
        let short_path = imp.file_path.rsplit('/').next().unwrap_or(&imp.file_path);
        out.push_str(&format!(
            "  [{}] {} in {}:{} - {} ({}->{})\n",
            imp.severity.to_uppercase(),
            imp.improvement_type.replace('_', " "),
            short_path,
            imp.start_line,
            imp.suggestion,
            imp.current_value,
            imp.threshold
        ));
    }
    out.trim_end().to_string()
}

/// Format improvement suggestions concisely for hooks
#[allow(dead_code)]
pub fn improvement_context(improvements: &[CodeImprovement]) -> String {
    if improvements.is_empty() {
        return String::new();
    }

    let high_severity: Vec<_> = improvements.iter()
        .filter(|i| i.severity == "high")
        .collect();

    if high_severity.is_empty() {
        return String::new();
    }

    let mut out = String::from("Code improvements needed:\n");
    for imp in high_severity.iter().take(3) {
        out.push_str(&format!(
            "  - [{}] {}: {} - {} lines (max: {})\n",
            imp.severity.to_uppercase(),
            imp.improvement_type.replace('_', " "),
            imp.symbol_name,
            imp.current_value,
            imp.threshold
        ));
    }
    out.trim_end().to_string()
}
