// crates/mira-server/src/background/code_health/analysis.rs
// LLM-powered code health analysis for complexity and error handling quality

use crate::db::Database;
use crate::llm::{DeepSeekClient, PromptBuilder};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;

/// Maximum bytes of function code to include in analysis (approx 5000 tokens)
const MAX_FUNCTION_CODE_BYTES: usize = 20_000;

/// Truncate function code if it exceeds the limit, preserving line structure
fn truncate_function_code(code: &str, max_bytes: usize) -> String {
    if code.len() <= max_bytes {
        return code.to_string();
    }

    tracing::warn!(
        "Function code too large ({} bytes), truncating to {} bytes",
        code.len(),
        max_bytes
    );

    // Find a good truncation point - try to end at a line boundary
    let truncated = &code[..max_bytes];
    if let Some(last_newline) = truncated.rfind('\n') {
        // Keep everything up to the last newline
        format!("{}\n// ... [code truncated, {} bytes total -> {} bytes]",
                &truncated[..last_newline], code.len(), max_bytes)
    } else {
        // No newline found, just truncate
        format!("{}... [code truncated, {} bytes total -> {} bytes]",
                truncated, code.len(), max_bytes)
    }
}

// Helper function to extract function code from a file
fn extract_function_code(
    project_path: &str,
    file_path: &str,
    start_line: i64,
    end_line: i64,
) -> Option<String> {
    let full_path = Path::new(project_path).join(file_path);
    let source = match std::fs::read_to_string(&full_path) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let lines: Vec<&str> = source.lines().collect();
    let start = (start_line as usize).saturating_sub(1).min(lines.len());
    let end = (end_line as usize).min(lines.len());
    if start >= end {
        return None; // Stale line numbers
    }
    Some(lines[start..end].join("\n"))
}

/// Generic LLM analysis function that abstracts the common pattern
async fn analyze_functions<F, P>(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    project_id: i64,
    project_path: &str,
    query_fn: F,
    prompt_builder: P,
    key_prefix: &'static str,
    content_prefix: &'static str,
    category: &'static str,
    limit: usize,
) -> Result<usize, String>
where
    F: Fn(&rusqlite::Connection, i64, &str) -> Result<Vec<(String, String, i64, i64)>, String> + Send + Sync + 'static,
    P: Fn(&str, &str, i64, i64, &str) -> String + Send + Sync + 'static,
{
    // Query database for functions to analyze
    let db_clone = db.clone();
    let project_path_owned = project_path.to_string();
    let functions = Database::run_blocking(db_clone, move |conn| {
        query_fn(conn, project_id, &project_path_owned)
    }).await?;

    if functions.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        "Code health: analyzing {} functions with LLM",
        functions.len()
    );

    let mut stored = 0;

    // Analyze functions up to the limit
    for (name, file_path, start_line, end_line) in functions.into_iter().take(limit) {
        // Extract the function code
        let function_code = match extract_function_code(project_path, &file_path, start_line, end_line) {
            Some(code) => code,
            None => continue,
        };

        // Skip if too short after extraction
        if function_code.lines().count() < 10 {
            continue;
        }

        // Truncate if function is too large to avoid token limit errors
        let function_code = truncate_function_code(&function_code, MAX_FUNCTION_CODE_BYTES);

        // Build the analysis prompt
        let prompt = prompt_builder(&name, &file_path, start_line, end_line, &function_code);

        // Call DeepSeek Reasoner - use appropriate prompt builder based on category
        let messages = if category == "complexity" {
            PromptBuilder::for_code_health_complexity().build_messages(prompt)
        } else if category == "error_quality" {
            PromptBuilder::for_code_health_error_quality().build_messages(prompt)
        } else {
            // Fallback
            PromptBuilder::for_code_health_complexity().build_messages(prompt)
        };

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
                        "[{}] {}:{} `{}`\n{}",
                        content_prefix, file_path, start_line, name, content
                    );
                    let key = format!("{}:{}:{}", key_prefix, file_path, name);

                    let db_clone = db.clone();
                    tokio::task::spawn_blocking(move || {
                        db_clone.store_memory(
                            Some(project_id),
                            Some(&key),
                            &issue_content,
                            "health",
                            Some(category),
                            0.75,
                        )
                    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))?
                    .map_err(|e| e.to_string())?;

                    tracing::info!("Code health: {} issue found in {}", category, name);
                    stored += 1;
                }
            }
            Err(e) => {
                tracing::warn!("Code health: LLM {} analysis failed for {}: {}", category, name, e);
            }
        }

        // Small delay between API calls
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(stored)
}

/// Use DeepSeek Reasoner to analyze large/complex functions
pub async fn scan_complexity(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    analyze_functions(
        db,
        deepseek,
        project_id,
        project_path,
        |conn, pid, _| get_large_functions(conn, pid, 50),
        |name, file_path, _start_line, _end_line, function_code| {
            format!(
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
            )
        },
        "health:complexity",
        "complexity",
        "complexity",
        3,
    ).await
}

/// Get large functions from the code symbols table
fn get_large_functions(
    conn: &rusqlite::Connection,
    project_id: i64,
    min_lines: i64,
) -> Result<Vec<(String, String, i64, i64)>, String> {
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
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Wrapper that calls get_error_heavy_functions and drops the question_marks count
    // (we'll count them in the prompt builder)
    let query_wrapper = |conn: &rusqlite::Connection, pid: i64, proj_path: &str| {
        let results = get_error_heavy_functions(conn, pid, proj_path)?;
        // Convert 5-tuple to 4-tuple by dropping question_marks
        let four_tuple: Vec<(String, String, i64, i64)> = results
            .into_iter()
            .map(|(name, file_path, start_line, end_line, _question_marks)| {
                (name, file_path, start_line, end_line)
            })
            .collect();
        Ok(four_tuple)
    };

    analyze_functions(
        db,
        deepseek,
        project_id,
        project_path,
        query_wrapper,
        |name, file_path, _start_line, _end_line, function_code| {
            // Count ? operators in the function code
            let question_marks = function_code.matches('?').count();
            format!(
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
            )
        },
        "health:error_quality",
        "error_quality",
        "error_quality",
        2,
    ).await
}

/// Find functions with many ? operators (error-propagation heavy)
fn get_error_heavy_functions(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<Vec<(String, String, i64, i64, usize)>, String> {
    use std::fs;

    // Get functions from symbols
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

    let functions: Vec<(String, String, i64, i64)> = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

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
