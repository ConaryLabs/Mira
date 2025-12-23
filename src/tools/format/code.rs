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

/// Format code search results - shows symbol name, type, file:line, and score
pub fn code_search_results(results: &[Value]) -> String {
    if results.is_empty() {
        return "No matches.".to_string();
    }

    let total = results.len();
    let show = std::cmp::min(10, total);

    let mut out = format!("Found {} match{}:\n",
        total,
        if total == 1 { "" } else { "es" }
    );

    for r in results.iter().take(show) {
        let file = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let symbol = r.get("symbol_name").and_then(|v| v.as_str());
        let symbol_type = r.get("symbol_type").and_then(|v| v.as_str());
        let start_line = r.get("start_line").and_then(|v| v.as_i64());
        let score = r.get("score").and_then(|v| v.as_f64());

        // Shorten path: just filename or last component
        let short_file = file.rsplit('/').next().unwrap_or(file);

        let score_str = score.map(|s| format!(" [{:.0}%]", s * 100.0)).unwrap_or_default();
        let line_str = start_line.map(|l| format!(":{}", l)).unwrap_or_default();

        match (symbol, symbol_type) {
            (Some(sym), Some(typ)) => {
                out.push_str(&format!("  {} ({}) {}{}{}\n", sym, typ, short_file, line_str, score_str))
            }
            (Some(sym), None) => {
                out.push_str(&format!("  {} {}{}{}\n", sym, short_file, line_str, score_str))
            }
            (None, Some(typ)) => {
                // No symbol name but has type - use content if available
                let content = r.get("content").and_then(|v| v.as_str());
                if let Some(c) = content {
                    // Extract first line of content as fallback name
                    let first_line = c.lines().next().unwrap_or("?");
                    // Strip " (type)" suffix if content was formatted as "Name (type)"
                    let type_suffix = format!(" ({})", typ);
                    let name = first_line.strip_suffix(&type_suffix).unwrap_or(first_line);
                    let short_name = if name.len() > 40 { format!("{}...", &name[..37]) } else { name.to_string() };
                    out.push_str(&format!("  {} ({}) {}{}{}\n", short_name, typ, short_file, line_str, score_str))
                } else {
                    out.push_str(&format!("  ({}) {}{}{}\n", typ, short_file, line_str, score_str))
                }
            }
            (None, None) => {
                out.push_str(&format!("  {}{}{}\n", short_file, line_str, score_str))
            }
        }
    }

    if total > show {
        out.push_str(&format!("  ... and {} more\n", total - show));
    }

    out.trim_end().to_string()
}

/// Format symbols list - shows first 10 with details
pub fn symbols_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No symbols.".to_string();
    }

    let total = results.len();
    let show = std::cmp::min(10, total);

    let mut out = format!("{} symbol{}:\n", total, if total == 1 { "" } else { "s" });

    for r in results.iter().take(show) {
        let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        // Support "type", "symbol_type", and "kind" field names
        let kind = r.get("type")
            .or_else(|| r.get("symbol_type"))
            .or_else(|| r.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let start = r.get("start_line").and_then(|v| v.as_i64());
        let end = r.get("end_line").and_then(|v| v.as_i64());

        let loc = match (start, end) {
            (Some(s), Some(e)) if s != e => format!(" lines {}-{}", s, e),
            (Some(s), _) => format!(" line {}", s),
            _ => String::new(),
        };
        out.push_str(&format!("  {} ({}){}\n", name, kind, loc));
    }

    if total > show {
        out.push_str(&format!("  ... and {} more\n", total - show));
    }

    out.trim_end().to_string()
}

/// Format commit list - shows first 10 with details
pub fn commit_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No commits.".to_string();
    }

    let total = results.len();
    let show = std::cmp::min(10, total);

    let mut out = format!("{} commit{}:\n", total, if total == 1 { "" } else { "s" });

    for r in results.iter().take(show) {
        // Support both "hash" and "commit_hash" field names
        let hash = r.get("commit_hash")
            .or_else(|| r.get("hash"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("?");
        // Support both "author" and "author_name" field names
        let author = r.get("author")
            .or_else(|| r.get("author_name"))
            .and_then(|v| v.as_str());

        let short_hash = if hash.len() > 7 { &hash[..7] } else { hash };
        // Get first line of message, truncate if needed
        let first_line = message.lines().next().unwrap_or(message);
        let short_msg = if first_line.len() > 60 {
            format!("{}...", &first_line[..57])
        } else {
            first_line.to_string()
        };

        match author {
            Some(a) => out.push_str(&format!("  {} {} ({})\n", short_hash, short_msg, a)),
            None => out.push_str(&format!("  {} {}\n", short_hash, short_msg)),
        }
    }

    if total > show {
        out.push_str(&format!("  ... and {} more\n", total - show));
    }

    out.trim_end().to_string()
}

/// Format related files
pub fn related_files(results: &[Value]) -> String {
    if results.is_empty() {
        return "No related files.".to_string();
    }

    let total = results.len();
    let show = std::cmp::min(10, total);

    let mut out = format!("{} related file{}:\n", total, if total == 1 { "" } else { "s" });

    for r in results.iter().take(show) {
        let path = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let rel_type = r.get("relation_type").and_then(|v| v.as_str()).unwrap_or("?");
        let score = r.get("score").and_then(|v| v.as_f64());

        let rel = score.map(|s| format!(" ({:.0}%)", s * 100.0)).unwrap_or_default();
        out.push_str(&format!("  {} [{}]{}\n", path, rel_type, rel));
    }

    if total > show {
        out.push_str(&format!("  ... and {} more\n", total - show));
    }

    out.trim_end().to_string()
}

/// Format cochange patterns - files that change together
pub fn cochange_patterns(results: &[Value]) -> String {
    if results.is_empty() {
        return "No cochange patterns.".to_string();
    }

    let total = results.len();
    let show = std::cmp::min(10, total);

    let mut out = format!("{} cochange pattern{}:\n", total, if total == 1 { "" } else { "s" });

    for r in results.iter().take(show) {
        let file = r.get("file").and_then(|v| v.as_str()).unwrap_or("?");
        let count = r.get("cochange_count").and_then(|v| v.as_i64()).unwrap_or(0);
        let confidence = r.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);

        out.push_str(&format!("  {} ({}x, {:.0}% confidence)\n", file, count, confidence * 100.0));
    }

    if total > show {
        out.push_str(&format!("  ... and {} more\n", total - show));
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
