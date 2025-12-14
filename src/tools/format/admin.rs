// src/tools/format/admin.rs
// Formatters for admin operations (tables, queries, project, guidelines, build errors)

use serde_json::Value;

/// Format project set response
pub fn project_set(name: &str, path: &str) -> String {
    format!("Project: {} ({})", name, path)
}

/// Format table list
pub fn table_list(tables: &[(String, i64)]) -> String {
    if tables.is_empty() {
        return "No tables.".to_string();
    }

    let mut out = format!("{} tables:\n", tables.len());
    for (name, count) in tables {
        out.push_str(&format!("  {} ({})\n", name, count));
    }

    out.trim_end().to_string()
}

/// Format query results
pub fn query_results(columns: &[String], rows: &[Vec<Value>]) -> String {
    if rows.is_empty() {
        return "No results.".to_string();
    }

    let mut out = format!("{} row{}:\n", rows.len(), if rows.len() == 1 { "" } else { "s" });

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            let len = format_value(val).len();
            if i < widths.len() && len > widths[i] {
                widths[i] = len.min(30); // Cap at 30
            }
        }
    }

    // Header
    out.push_str("  ");
    for (i, col) in columns.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(10);
        out.push_str(&format!("{:width$} ", col, width = w));
    }
    out.push('\n');

    // Rows (limit to 20)
    for row in rows.iter().take(20) {
        out.push_str("  ");
        for (i, val) in row.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(10);
            let formatted = format_value(val);
            let display = if formatted.len() > 30 {
                format!("{}...", &formatted[..27])
            } else {
                formatted
            };
            out.push_str(&format!("{:width$} ", display, width = w));
        }
        out.push('\n');
    }

    if rows.len() > 20 {
        out.push_str(&format!("  ... and {} more\n", rows.len() - 20));
    }

    out.trim_end().to_string()
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(o) => format!("{{{}}}", o.len()),
    }
}

/// Format build errors
pub fn build_errors(results: &[Value]) -> String {
    if results.is_empty() {
        return "No build errors.".to_string();
    }

    let mut out = format!("{} error{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let file = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let line = r.get("line_number").and_then(|v| v.as_i64());
        let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("?");

        let loc = line.map(|l| format!(":{}", l)).unwrap_or_default();
        let msg_short = if message.len() > 60 { format!("{}...", &message[..57]) } else { message.to_string() };

        out.push_str(&format!("  {}{} {}\n", file, loc, msg_short));
    }

    out.trim_end().to_string()
}

/// Format guidelines
pub fn guidelines(results: &[Value]) -> String {
    if results.is_empty() {
        return "No guidelines.".to_string();
    }

    let mut out = String::new();
    let mut current_category = String::new();

    for r in results {
        let category = r.get("category").and_then(|v| v.as_str()).unwrap_or("general");
        let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("?");

        if category != current_category {
            if !current_category.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", category));
            current_category = category.to_string();
        }

        out.push_str(&format!("  {}\n", content));
    }

    out.trim_end().to_string()
}
