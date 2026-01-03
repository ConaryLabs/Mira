// web/chat/tools.rs
// Tool execution for DeepSeek chat

use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use crate::web::deepseek;
use crate::web::state::AppState;
use mira_types::{AgentRole, WsEvent};

/// Claude Code usage guide - injected when spawn_claude is first used
const CLAUDE_CODE_GUIDE: &str = r#"## Claude Code Instance Guide (v2.0.76)

You now have a Claude Code instance running. Use `send_to_claude` with this instance_id for follow-up.

### What Claude Code Can Do
- **Read/Write/Edit files** with surgical precision (AST-aware)
- **Run terminal commands** (bash, git, npm, cargo, etc.)
- **Multi-file changes** atomically coordinated
- **Web search/fetch** for documentation lookups

### Effective Follow-ups via send_to_claude
Be specific in your messages:
- "Run the tests and fix any failures"
- "Commit the changes with message 'feat: add X'"
- "Also update the related tests in tests/unit/"
- "Show me the git diff of your changes"

### Claude's Available Tools
- `Read`, `Write`, `Edit`, `Glob`, `Grep` - file operations
- `Bash` - terminal commands (supports background execution)
- `WebFetch`, `WebSearch` - web access
- `Task` - spawn subagents for parallel work

### Tips
- Claude maintains full conversation context
- Output streams to UI in real-time
- Instance persists until killed or task complete
- Multiple instances can run in parallel
"#;

/// System prompt for the persistent collaborator Claude instance
const COLLABORATOR_SYSTEM_PROMPT: &str = r#"You are Claude, running as a COLLABORATOR alongside Mira (a DeepSeek AI). You work together on coding tasks.

## How This Works

When you see [MIRA_MESSAGE id="..."], Mira is asking you something. You MUST:
1. Use your tools (Read, Write, Bash, Grep, etc.) to investigate if needed
2. Call reply_to_mira(in_reply_to="<id>", content="<your response>") when done

Example:
[MIRA_MESSAGE id="abc123"]
Are the tests passing?
[/MIRA_MESSAGE]

You would run the tests, then call:
reply_to_mira(in_reply_to="abc123", content="Yes, all 42 tests pass.")

IMPORTANT: Always respond via reply_to_mira tool, not just text output. Mira is waiting for your structured response.

## Your Role
- You have access to the filesystem, terminal, and all Claude Code tools
- You can read files, run commands, make changes
- Work with Mira to solve problems collaboratively
- Be concise but thorough in your responses
"#;

/// Execute tool calls and return results
#[instrument(skip(state, tool_calls), fields(tool_count = tool_calls.len()))]
pub async fn execute_tools(
    state: &AppState,
    tool_calls: &[deepseek::ToolCall],
) -> Vec<(String, String)> {
    let mut results = Vec::new();

    for tc in tool_calls {
        let start_time = Instant::now();
        let args: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

        debug!(
            tool = %tc.function.name,
            call_id = %tc.id,
            args = %args,
            "Executing tool"
        );

        let result = match tc.function.name.as_str() {
            "recall_memories" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(5);

                match execute_recall(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "search_code" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);

                match execute_code_search(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_tasks" => {
                match execute_list_tasks(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_goals" => {
                match execute_list_goals(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "spawn_claude" => {
                let initial_prompt = args
                    .get("initial_prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let working_dir = args
                    .get("working_directory")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        // Use project path if available
                        futures::executor::block_on(state.get_project())
                            .map(|p| p.path)
                    })
                    .unwrap_or_else(|| ".".to_string());

                match state
                    .claude_manager
                    .spawn(working_dir.clone(), Some(initial_prompt.to_string()))
                    .await
                {
                    Ok(id) => {
                        // Broadcast to WebSocket so terminal tray can pick it up
                        state.broadcast(mira_types::WsEvent::ClaudeSpawned {
                            instance_id: id.clone(),
                            working_dir,
                        });
                        format!(
                            "Claude instance started with ID: {}\n\n{}",
                            id, CLAUDE_CODE_GUIDE
                        )
                    }
                    Err(e) => format!("Error spawning Claude: {}", e),
                }
            }
            "send_to_claude" => {
                let instance_id = args
                    .get("instance_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");

                match state.claude_manager.send_input(instance_id, message).await {
                    Ok(_) => "Message sent to Claude".to_string(),
                    Err(e) => format!("Error: {}", e),
                }
            }
            "discuss" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                execute_discuss(state, message).await
            }
            "google_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let num_results = args.get("num_results").and_then(|v| v.as_u64()).unwrap_or(5) as u32;
                execute_google_search(state, query, num_results).await
            }
            "web_fetch" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                execute_web_fetch(state, url).await
            }
            "research" => {
                let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("");
                let depth = args.get("depth").and_then(|v| v.as_str()).unwrap_or("quick");
                execute_research(state, question, depth).await
            }
            _ => {
                warn!(tool = %tc.function.name, "Unknown tool requested");
                format!("Unknown tool: {}", tc.function.name)
            }
        };

        let duration_ms = start_time.elapsed().as_millis() as u64;
        let success = !result.starts_with("Error");

        if success {
            info!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result_len = result.len(),
                "Tool executed successfully"
            );
        } else {
            error!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result = %result,
                "Tool execution failed"
            );
        }

        // Log tool result (don't broadcast - causes UI flooding with large results)
        // Tool calls are shown in final message via tool_calls field

        results.push((tc.id.clone(), result));
    }

    results
}

async fn execute_recall(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = state.db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let mut stmt = conn.prepare(
                "SELECT f.content FROM memory_facts f
                 JOIN vec_memory v ON f.id = v.fact_id
                 WHERE (f.project_id = ?1 OR ?1 IS NULL)
                 ORDER BY vec_distance_cosine(v.embedding, ?2)
                 LIMIT ?3",
            )?;

            let memories: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    row.get(0)
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !memories.is_empty() {
                return Ok(format!(
                    "{}Found {} memories:\n{}",
                    context_header,
                    memories.len(),
                    memories.join("\n---\n")
                ));
            }
        }
    }

    Ok(format!("{}No memories found", context_header))
}

async fn execute_code_search(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    // Try semantic search first - get embedding before acquiring connection
    let mut semantic_results: Vec<(String, String, f32)> = Vec::new();
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let conn = state.db.conn();
            let mut stmt = conn.prepare(
                "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?2) as distance
                 FROM vec_code
                 WHERE project_id = ?1 OR ?1 IS NULL
                 ORDER BY distance
                 LIMIT ?3",
            )?;

            semantic_results = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    let path: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let distance: f32 = row.get(2)?;
                    Ok((path, content, 1.0 - distance)) // Convert to score
                })?
                .filter_map(|r| r.ok())
                .collect();
        }
    }

    // Check semantic result quality
    let best_score = semantic_results
        .iter()
        .map(|(_, _, s)| *s)
        .fold(0.0f32, |a, b| a.max(b));

    // If semantic results are poor, try keyword fallback
    let (results, search_type) = if semantic_results.is_empty() || best_score < 0.25 {
        let conn = state.db.conn();
        let keyword_results = keyword_search_fallback(&conn, query, project_id, project_path.as_deref(), limit as usize);

        if !keyword_results.is_empty() {
            tracing::debug!(
                "Semantic search poor (best_score={:.2}), using {} keyword results",
                best_score,
                keyword_results.len()
            );
            (keyword_results, "keyword")
        } else if !semantic_results.is_empty() {
            (semantic_results, "semantic")
        } else {
            return Ok(format!("{}No code matches found", context_header));
        }
    } else {
        (semantic_results, "semantic")
    };

    let formatted: Vec<String> = results
        .into_iter()
        .map(|(path, content, score)| format!("## {} (score: {:.2})\n```\n{}\n```", path, score, content))
        .collect();

    Ok(format!(
        "{}Found {} code matches ({} search):\n{}",
        context_header,
        formatted.len(),
        search_type,
        formatted.join("\n\n")
    ))
}

/// Keyword-based fallback when semantic search yields poor results
fn keyword_search_fallback(
    conn: &rusqlite::Connection,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Vec<(String, String, f32)> {
    let mut results = Vec::new();

    // Split query into terms for flexible matching
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return results;
    }

    let like_patterns: Vec<String> = terms
        .iter()
        .map(|t| format!("%{}%", t.to_lowercase()))
        .collect();

    // Search vec_code chunk_content
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let sql = "SELECT file_path, chunk_content FROM vec_code
                       WHERE project_id = ? AND LOWER(chunk_content) LIKE ?
                       LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(sql) {
                if let Ok(rows) = stmt.query_map(rusqlite::params![pid, pattern, limit as i64], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        if !results.iter().any(|(f, c, _)| f == &row.0 && c == &row.1) {
                            results.push((row.0, row.1, 0.5));
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    // Also search symbol names
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let sql = "SELECT file_path, name, signature, start_line, end_line
                       FROM code_symbols
                       WHERE project_id = ? AND LOWER(name) LIKE ?
                       LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(sql) {
                if let Ok(rows) = stmt.query_map(rusqlite::params![pid, pattern, limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                }) {
                    for row in rows.flatten() {
                        let (file_path, name, signature, start_line, end_line) = row;

                        // Try to read actual code from file
                        let content = if let (Some(proj_path), Some(start), Some(end)) =
                            (project_path, start_line, end_line)
                        {
                            let full_path = std::path::Path::new(proj_path).join(&file_path);
                            if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                                let lines: Vec<&str> = file_content.lines().collect();
                                let start_idx = (start as usize).saturating_sub(1);
                                let end_idx = (end as usize).min(lines.len());
                                lines[start_idx..end_idx].join("\n")
                            } else {
                                signature.unwrap_or_else(|| name.clone())
                            }
                        } else {
                            signature.unwrap_or_else(|| name.clone())
                        };

                        if !results.iter().any(|(f, _, _)| f == &file_path && content.contains(&name)) {
                            results.push((file_path, content, 0.6));
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    results.truncate(limit);
    results
}

async fn execute_list_tasks(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, priority FROM tasks
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 20",
    )?;

    let tasks: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let priority: String = row.get(2)?;
            Ok(format!("- [{}] {} ({})", status, title, priority))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if tasks.is_empty() {
        Ok("No tasks found".to_string())
    } else {
        Ok(format!("Tasks:\n{}", tasks.join("\n")))
    }
}

async fn execute_list_goals(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, progress_percent FROM goals
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 10",
    )?;

    let goals: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let progress: i32 = row.get(2)?;
            Ok(format!("- [{}] {} ({}%)", status, title, progress))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if goals.is_empty() {
        Ok("No goals found".to_string())
    } else {
        Ok(format!("Goals:\n{}", goals.join("\n")))
    }
}

/// Execute Google search
async fn execute_google_search(state: &AppState, query: &str, num_results: u32) -> String {
    if query.is_empty() {
        return "Error: query is required".to_string();
    }

    match &state.google_search {
        Some(client) => match client.search(query, num_results).await {
            Ok(results) => {
                if results.is_empty() {
                    "No search results found".to_string()
                } else {
                    let formatted: Vec<String> = results
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            format!("{}. **{}**\n   {}\n   {}", i + 1, r.title, r.url, r.snippet)
                        })
                        .collect();
                    format!("Search results for \"{}\":\n\n{}", query, formatted.join("\n\n"))
                }
            }
            Err(e) => format!("Error: {}", e),
        },
        None => "Error: Google Search is not configured. Set GOOGLE_API_KEY and GOOGLE_SEARCH_CX environment variables.".to_string(),
    }
}

/// Execute web fetch
async fn execute_web_fetch(state: &AppState, url: &str) -> String {
    if url.is_empty() {
        return "Error: url is required".to_string();
    }

    match state.web_fetcher.fetch(url).await {
        Ok(page) => {
            format!(
                "# {}\n\nURL: {}\nWord count: {}\n\n---\n\n{}",
                page.title, page.url, page.word_count, page.content
            )
        }
        Err(e) => format!("Error fetching {}: {}", url, e),
    }
}

/// Source citation for research results
#[derive(Debug, Clone)]
struct Source {
    title: String,
    url: String,
    snippet: String,
}

/// Execute research - the intelligent grounding pipeline
async fn execute_research(state: &AppState, question: &str, depth: &str) -> String {
    if question.is_empty() {
        return "Error: question is required".to_string();
    }

    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => return "Error: DeepSeek client not configured".to_string(),
    };

    let google = match &state.google_search {
        Some(g) => g,
        None => return "Error: Google Search not configured. Set GOOGLE_API_KEY and GOOGLE_SEARCH_CX.".to_string(),
    };

    let start_time = std::time::Instant::now();
    info!(question = %question, depth = %depth, "Starting research pipeline");

    // Determine search parameters based on depth
    let (num_queries, pages_to_read) = match depth {
        "thorough" => (3, 5),
        _ => (1, 3), // quick
    };

    // Get project context for query generation
    let project_context = state.get_project().await
        .map(|p| p.name.unwrap_or_else(|| "unknown".to_string()));

    // Step 1: Generate search queries (always use query generation for disambiguation)
    let queries = match generate_search_queries(deepseek, question, num_queries, project_context.as_deref()).await {
        Ok(q) => q,
        Err(e) => {
            warn!("Failed to generate queries, using original: {}", e);
            vec![question.to_string()]
        }
    };

    info!(queries = ?queries, "Generated search queries");

    // Step 2: Search and collect unique URLs
    let mut all_results = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

    for query in &queries {
        match google.search(query, 5).await {
            Ok(results) => {
                for r in results {
                    if !seen_urls.contains(&r.url) {
                        seen_urls.insert(r.url.clone());
                        all_results.push(Source {
                            title: r.title,
                            url: r.url,
                            snippet: r.snippet,
                        });
                    }
                }
            }
            Err(e) => {
                warn!(query = %query, error = %e, "Search failed");
            }
        }
    }

    if all_results.is_empty() {
        return format!("No search results found for: {}", question);
    }

    info!(total_results = all_results.len(), "Collected search results");

    // Step 3: Fetch and read top pages
    let mut page_contents: Vec<(Source, String)> = Vec::new();

    for source in all_results.iter().take(pages_to_read) {
        match state.web_fetcher.fetch(&source.url).await {
            Ok(page) => {
                // Truncate content to ~3000 chars per page to stay within limits
                let content = if page.content.len() > 3000 {
                    format!("{}...", &page.content[..3000])
                } else {
                    page.content
                };
                page_contents.push((source.clone(), content));
            }
            Err(e) => {
                debug!(url = %source.url, error = %e, "Failed to fetch page, using snippet");
                // Fall back to snippet
                page_contents.push((source.clone(), source.snippet.clone()));
            }
        }
    }

    info!(pages_read = page_contents.len(), "Fetched page contents");

    // Step 4: Synthesize with DeepSeek-chat
    let synthesis = match synthesize_research(deepseek, question, &page_contents).await {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "Synthesis failed");
            // Fall back to returning raw snippets
            let snippets: Vec<String> = all_results.iter()
                .take(5)
                .enumerate()
                .map(|(i, s)| format!("[{}] **{}**\n{}\n{}", i + 1, s.title, s.url, s.snippet))
                .collect();
            return format!(
                "Research for: {}\n\n(Synthesis failed, showing raw results)\n\n{}",
                question,
                snippets.join("\n\n")
            );
        }
    };

    // Step 5: Format response with citations
    let sources_list: Vec<String> = page_contents.iter()
        .enumerate()
        .map(|(i, (s, _))| format!("[{}] {} - {}", i + 1, s.title, s.url))
        .collect();

    let duration_ms = start_time.elapsed().as_millis();
    info!(duration_ms = duration_ms, "Research pipeline complete");

    format!(
        "{}\n\n---\n**Sources:**\n{}",
        synthesis,
        sources_list.join("\n")
    )
}

/// Generate multiple search queries from a question using DeepSeek-chat
async fn generate_search_queries(
    deepseek: &std::sync::Arc<crate::web::deepseek::DeepSeekClient>,
    question: &str,
    num_queries: usize,
    project_context: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let system = r#"You generate precise web search queries. Your job is to disambiguate queries to find relevant results.

Rules:
1. ONLY add technical context if the query is clearly technical (programming, frameworks, APIs)
2. For general queries (food, travel, products, etc.), keep them general - just add year if relevant
3. If project context is provided AND the query relates to that domain, use it for disambiguation
4. Avoid adding unnecessary jargon that narrows results too much

Examples:
- Technical + Rust project: "Leptos vs Yew" → "Leptos Rust framework vs Yew framework comparison"
- Technical + no project: "React hooks" → "React hooks JavaScript tutorial"
- General query: "best pizza NYC" → "best pizza NYC 2025"
- General query: "how to make scrambled eggs" → "how to make perfect scrambled eggs recipe"
- Ambiguous tech: "Apollo" → keep as-is unless context suggests GraphQL vs space program

Output ONLY the search queries, one per line. No numbering, no explanation."#;

    let context_hint = match project_context {
        Some(name) => format!("\nProject context: {} (use for disambiguation if query is related)", name),
        None => String::new(),
    };

    let user = format!(
        "Question: {}{}\n\nGenerate {} search queries:",
        question, context_hint, num_queries
    );

    let response = deepseek.chat_simple(system, &user).await?;

    let queries: Vec<String> = response
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'))
        .take(num_queries)
        .map(String::from)
        .collect();

    if queries.is_empty() {
        Ok(vec![question.to_string()])
    } else {
        Ok(queries)
    }
}

/// Synthesize research findings into a coherent answer
async fn synthesize_research(
    deepseek: &std::sync::Arc<crate::web::deepseek::DeepSeekClient>,
    question: &str,
    sources: &[(Source, String)],
) -> anyhow::Result<String> {
    let system = r#"You are a research synthesizer. Given a question and source materials, write a clear, accurate answer that:
1. Directly addresses the question
2. Synthesizes information from multiple sources
3. Uses inline citations like [1], [2] to reference sources
4. Is concise but comprehensive
5. Notes any conflicting information or uncertainty

Do NOT include a sources list - just the synthesized answer with inline citations."#;

    let sources_text: Vec<String> = sources
        .iter()
        .enumerate()
        .map(|(i, (s, content))| {
            format!(
                "=== Source [{}]: {} ===\nURL: {}\n\n{}",
                i + 1, s.title, s.url, content
            )
        })
        .collect();

    let user = format!(
        "Question: {}\n\n{}\n\nSynthesize an answer with inline citations:",
        question,
        sources_text.join("\n\n")
    );

    deepseek.chat_simple(system, &user).await
}

/// Execute discuss tool - real-time conversation with Claude
/// Uses --print mode for each discussion (Claude runs, responds, exits)
async fn execute_discuss(state: &AppState, message: &str) -> String {
    let message_id = Uuid::new_v4().to_string();

    // Get session ID for thread context
    let session_id = state.session_id.read().await.clone().unwrap_or_else(|| "default".to_string());

    // Get working directory from project
    let working_dir = state
        .get_project()
        .await
        .map(|p| p.path)
        .unwrap_or_else(|| ".".to_string());

    // Create response channel
    let (tx, rx) = oneshot::channel();
    {
        let mut pending = state.pending_responses.write().await;
        pending.insert(message_id.clone(), tx);
    }

    // Broadcast that Mira is sending a message
    state.broadcast(WsEvent::AgentMessage {
        message_id: message_id.clone(),
        from: AgentRole::Mira,
        to: AgentRole::Claude,
        content: message.to_string(),
        thread_id: session_id.clone(),
    });

    // Build the full prompt for Claude (includes context + message + instructions)
    let full_prompt = format!(
        "{}\n\n---\n\n[MIRA_MESSAGE id=\"{}\"]\n{}\n[/MIRA_MESSAGE]\n\nRespond using the reply_to_mira MCP tool with in_reply_to=\"{}\".",
        COLLABORATOR_SYSTEM_PROMPT, message_id, message, message_id
    );

    // Spawn Claude with --print mode (runs once, responds, exits)
    info!(
        message_id = %message_id,
        working_dir = %working_dir,
        "Spawning Claude for discussion"
    );

    match state.claude_manager.spawn(working_dir.clone(), Some(full_prompt)).await {
        Ok(instance_id) => {
            state.broadcast(WsEvent::ClaudeSpawned {
                instance_id: instance_id.clone(),
                working_dir,
            });

            // Wait for response with timeout (2 minutes)
            match tokio::time::timeout(Duration::from_secs(120), rx).await {
                Ok(Ok(response)) => {
                    info!(message_id = %message_id, response_len = response.len(), "Received Claude response");
                    format!("Claude: {}", response)
                }
                Ok(Err(_)) => {
                    warn!(message_id = %message_id, "Claude response channel closed");
                    "Claude's response channel was closed unexpectedly".to_string()
                }
                Err(_) => {
                    // Clean up on timeout
                    state.pending_responses.write().await.remove(&message_id);
                    warn!(message_id = %message_id, "Timeout waiting for Claude response");
                    "Timeout: Claude did not respond within 2 minutes".to_string()
                }
            }
        }
        Err(e) => {
            state.pending_responses.write().await.remove(&message_id);
            format!("Error spawning Claude: {}", e)
        }
    }
}
