// crates/mira-server/src/background/code_health/analysis.rs
// LLM-powered code health analysis for complexity and error handling quality

use crate::db::Database;
use crate::llm::{DeepSeekClient, Message};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;

/// Use DeepSeek Reasoner to analyze large/complex functions
pub async fn scan_complexity(
    db: &Database,
    deepseek: &Arc<DeepSeekClient>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Find large functions (over 50 lines) that haven't been analyzed recently
    let large_functions = get_large_functions(db, project_id, 50)?;

    if large_functions.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        "Code health: analyzing {} large functions with LLM",
        large_functions.len()
    );

    let mut stored = 0;

    // Only analyze up to 3 functions per scan to avoid rate limiting
    for (name, file_path, start_line, end_line) in large_functions.into_iter().take(3) {
        // Read the function source
        let full_path = Path::new(project_path).join(&file_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Extract the function (with some context)
        let lines: Vec<&str> = source.lines().collect();
        let start = (start_line as usize).saturating_sub(1).min(lines.len());
        let end = (end_line as usize).min(lines.len());
        if start >= end {
            continue; // Stale line numbers
        }
        let function_code: String = lines[start..end].join("\n");

        // Skip if too short after extraction (might be wrong line numbers)
        if function_code.lines().count() < 30 {
            continue;
        }

        // Build the analysis prompt
        let prompt = format!(
            r#"Analyze this function for complexity issues. Be concise and actionable.

Function `{}` in {}:
```
{}
```

Review for:
1. Does this function do too many things? Should it be split?
2. Are there deeply nested conditionals that hurt readability?
3. Are there repeated patterns that should be extracted?
4. Is the control flow hard to follow?

If there are NO significant issues, respond with just: OK

If there ARE issues, respond with a brief summary (2-3 sentences max) of the most important problem and a concrete suggestion. Format:
ISSUE: <description>
SUGGESTION: <what to do>"#,
            name, file_path, function_code
        );

        // Call DeepSeek Reasoner
        let messages = vec![
            Message::system("You are a code reviewer focused on function complexity and maintainability. Be direct and concise."),
            Message::user(prompt),
        ];

        match deepseek.chat(messages, None).await {
            Ok(result) => {
                if let Some(content) = result.content {
                    let content = content.trim();

                    // Skip if no issues found
                    if content == "OK" || content.to_lowercase().contains("no significant issues") {
                        tracing::debug!("Code health: {} is OK", name);
                        continue;
                    }

                    // Store the issue
                    let issue_content = format!(
                        "[complexity] {}:{} `{}`\n{}",
                        file_path, start_line, name, content
                    );
                    let key = format!("health:complexity:{}:{}", file_path, name);

                    db.store_memory(
                        Some(project_id),
                        Some(&key),
                        &issue_content,
                        "health",
                        Some("complexity"),
                        0.75,
                    )
                    .map_err(|e| e.to_string())?;

                    tracing::info!("Code health: complexity issue found in {}", name);
                    stored += 1;
                }
            }
            Err(e) => {
                tracing::warn!("Code health: LLM analysis failed for {}: {}", name, e);
            }
        }

        // Small delay between API calls
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(stored)
}

/// Get large functions from the code symbols table
fn get_large_functions(
    db: &Database,
    project_id: i64,
    min_lines: i64,
) -> Result<Vec<(String, String, i64, i64)>, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT name, file_path, start_line, end_line
             FROM code_symbols
             WHERE project_id = ?
               AND symbol_type = 'function'
               AND end_line IS NOT NULL
               AND (end_line - start_line) >= ?
               AND file_path NOT LIKE '%/tests/%'
               AND file_path NOT LIKE '%_test.rs'
               AND name NOT LIKE 'test_%'
             ORDER BY (end_line - start_line) DESC
             LIMIT 10",
        )
        .map_err(|e| e.to_string())?;

    let results = stmt
        .query_map(params![project_id, min_lines], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// LLM-powered analysis of error handling quality in complex functions
pub async fn scan_error_quality(
    db: &Database,
    deepseek: &Arc<DeepSeekClient>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Find functions with many ? operators (error propagation heavy)
    let error_heavy_functions = get_error_heavy_functions(db, project_id, project_path)?;

    if error_heavy_functions.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        "Code health: analyzing {} error-heavy functions with LLM",
        error_heavy_functions.len()
    );

    let mut stored = 0;

    // Only analyze up to 2 functions per scan
    for (name, file_path, start_line, end_line, question_marks) in error_heavy_functions.into_iter().take(2) {
        let full_path = Path::new(project_path).join(&file_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let start = (start_line as usize).saturating_sub(1).min(lines.len());
        let end = (end_line as usize).min(lines.len());
        if start >= end {
            continue; // Stale line numbers
        }
        let function_code: String = lines[start..end].join("\n");

        let prompt = format!(
            r#"Analyze error handling quality in this function. Be concise.

Function `{}` in {} ({} error propagation points):
```
{}
```

Check for:
1. Are errors propagated with enough context? (Should use .context() or .map_err()?)
2. Are there places where errors are silently swallowed that shouldn't be?
3. Would a caller understand what went wrong from the error messages?
4. Are there inconsistent error handling patterns?

If error handling is GOOD, respond with just: OK

If there ARE issues, respond with the most important problem and fix. Format:
ISSUE: <description>
SUGGESTION: <what to do>"#,
            name, file_path, question_marks, function_code
        );

        let messages = vec![
            Message::system("You are a code reviewer focused on error handling quality and debuggability. Be direct and concise."),
            Message::user(prompt),
        ];

        match deepseek.chat(messages, None).await {
            Ok(result) => {
                if let Some(content) = result.content {
                    let content = content.trim();

                    if content == "OK" || content.to_lowercase().contains("error handling is good") {
                        tracing::debug!("Code health: error handling in {} is OK", name);
                        continue;
                    }

                    let issue_content = format!(
                        "[error_quality] {}:{} `{}`\n{}",
                        file_path, start_line, name, content
                    );
                    let key = format!("health:error_quality:{}:{}", file_path, name);

                    db.store_memory(
                        Some(project_id),
                        Some(&key),
                        &issue_content,
                        "health",
                        Some("error_quality"),
                        0.75,
                    )
                    .map_err(|e| e.to_string())?;

                    tracing::info!("Code health: error quality issue found in {}", name);
                    stored += 1;
                }
            }
            Err(e) => {
                tracing::warn!("Code health: LLM error analysis failed for {}: {}", name, e);
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(stored)
}

/// Find functions with many ? operators (error-propagation heavy)
fn get_error_heavy_functions(
    db: &Database,
    project_id: i64,
    project_path: &str,
) -> Result<Vec<(String, String, i64, i64, usize)>, String> {
    use std::fs;

    // Get functions from symbols
    let functions: Vec<(String, String, i64, i64)> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT name, file_path, start_line, end_line
                 FROM code_symbols
                 WHERE project_id = ?
                   AND symbol_type = 'function'
                   AND end_line IS NOT NULL
                   AND (end_line - start_line) >= 20
                   AND file_path NOT LIKE '%/tests/%'
                   AND name NOT LIKE 'test_%'
                 ORDER BY (end_line - start_line) DESC
                 LIMIT 50",
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect()
    };

    // Count ? operators in each function
    let mut results = Vec::new();

    for (name, file_path, start_line, end_line) in functions {
        let full_path = Path::new(project_path).join(&file_path);
        let source = match fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let start = (start_line as usize).saturating_sub(1);
        let end = (end_line as usize).min(lines.len());

        // Skip if line numbers are out of bounds (stale index)
        if start >= lines.len() || start >= end {
            continue;
        }

        let mut question_marks = 0;
        for line in &lines[start..end] {
            // Count ? operators (error propagation)
            question_marks += line.matches('?').count();
        }

        // Only include functions with significant error handling
        if question_marks >= 5 {
            results.push((name, file_path, start_line, end_line, question_marks));
        }
    }

    // Sort by question mark count descending
    results.sort_by(|a, b| b.4.cmp(&a.4));
    results.truncate(10);

    Ok(results)
}
