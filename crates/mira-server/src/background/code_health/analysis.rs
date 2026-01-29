// crates/mira-server/src/background/code_health/analysis.rs
// LLM-powered code health analysis for complexity and error handling quality

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreMemoryParams, get_error_heavy_functions_sync, get_large_functions_sync, store_memory_sync,
};
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use crate::utils::ResultExt;
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
        format!(
            "{}\n// ... [code truncated, {} bytes total -> {} bytes]",
            &truncated[..last_newline],
            code.len(),
            max_bytes
        )
    } else {
        // No newline found, just truncate
        format!(
            "{}... [code truncated, {} bytes total -> {} bytes]",
            truncated,
            code.len(),
            max_bytes
        )
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
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
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
    F: Fn(&rusqlite::Connection, i64, &str) -> Result<Vec<(String, String, i64, i64)>, String>
        + Send
        + Sync
        + 'static,
    P: Fn(&str, &str, i64, i64, &str) -> String + Send + Sync + 'static,
{
    // Query database for functions to analyze
    let project_path_owned = project_path.to_string();
    let functions = pool
        .interact(move |conn| {
            query_fn(conn, project_id, &project_path_owned).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

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
        let function_code =
            match extract_function_code(project_path, &file_path, start_line, end_line) {
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

        // Call LLM - use appropriate prompt builder based on category
        let messages = if category == "complexity" {
            PromptBuilder::for_code_health_complexity().build_messages(prompt)
        } else if category == "error_quality" {
            PromptBuilder::for_code_health_error_quality().build_messages(prompt)
        } else {
            // Fallback
            PromptBuilder::for_code_health_complexity().build_messages(prompt)
        };

        match client.chat(messages, None).await {
            Ok(result) => {
                // Record usage
                record_llm_usage(
                    pool,
                    client.provider_type(),
                    &client.model_name(),
                    &format!("background:code_health:{}", category),
                    &result,
                    Some(project_id),
                    None,
                )
                .await;

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

                    pool.interact(move |conn| {
                        store_memory_sync(
                            conn,
                            StoreMemoryParams {
                                project_id: Some(project_id),
                                key: Some(&key),
                                content: &issue_content,
                                fact_type: "health",
                                category: Some(category),
                                confidence: 0.75,
                                session_id: None,
                                user_id: None,
                                scope: "project",
                                branch: None,
                            },
                        )
                        .map_err(|e| anyhow::anyhow!("Failed to store: {}", e))
                    })
                    .await
                    .str_err()?;

                    tracing::info!("Code health: {} issue found in {}", category, name);
                    stored += 1;
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Code health: LLM {} analysis failed for {}: {}",
                    category,
                    name,
                    e
                );
            }
        }

        // Small delay between API calls
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(stored)
}

/// Use LLM to analyze large/complex functions
pub async fn scan_complexity(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    analyze_functions(
        pool,
        client,
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
    get_large_functions_sync(conn, project_id, min_lines).str_err()
}

/// LLM-powered analysis of error handling quality in complex functions
pub async fn scan_error_quality(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
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
        pool,
        client,
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
    )
    .await
}

/// Find functions with many ? operators (error-propagation heavy)
fn get_error_heavy_functions(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<Vec<(String, String, i64, i64, usize)>, String> {
    use std::fs;

    // Get functions from symbols (uses db function)
    let functions = get_error_heavy_functions_sync(conn, project_id).str_err()?;

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
