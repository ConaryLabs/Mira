// crates/mira-server/src/background/documentation/generation.rs
// Documentation draft generation using LLM providers

use crate::db::Database;
use crate::db::documentation::{
    get_pending_doc_tasks, mark_doc_task_error, store_doc_draft, DocTask,
};
use crate::llm::{PromptBuilder, Provider};
use rusqlite::{params, OptionalExtension};
use std::path::Path;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use super::{file_checksum, read_file_content};

/// Max drafts to generate per cycle (rate limiting)
const MAX_DRAFTS_PER_CYCLE: usize = 3;

/// Delay between draft generations (seconds)
const DRAFT_GENERATION_DELAY: u64 = 2;

/// Generate pending documentation drafts
pub async fn generate_pending_drafts(
    db: &Arc<Database>,
    llm_factory: &Arc<crate::llm::ProviderFactory>,
) -> Result<usize, String> {
    let db_clone = db.clone();

    // Get pending tasks
    let tasks = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        get_pending_doc_tasks(&conn, None, MAX_DRAFTS_PER_CYCLE)
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    if tasks.is_empty() {
        return Ok(0);
    }

    tracing::info!("Documentation: generating {} drafts", tasks.len());

    let mut generated = 0;

    for task in tasks {
        // Rate limiting between generations
        if generated > 0 {
            sleep(Duration::from_secs(DRAFT_GENERATION_DELAY)).await;
        }

        match generate_single_draft(db, llm_factory, &task).await {
            Ok(_) => {
                generated += 1;
                tracing::debug!("Generated draft for {}", task.target_doc_path);
            }
            Err(e) => {
                tracing::warn!("Failed to generate draft for {}: {}", task.target_doc_path, e);
                // Mark error (synchronous DB operation)
                {
                    let conn = db.conn();
                    mark_doc_task_error(&conn, task.id, &e)?;
                }
            }
        }
    }

    Ok(generated)
}

/// Generate a single documentation draft
async fn generate_single_draft(
    db: &Arc<Database>,
    llm_factory: &Arc<crate::llm::ProviderFactory>,
    task: &DocTask,
) -> Result<(), String> {
    // Get project path
    let (project_path, _project_name) = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let project_id = task.project_id.ok_or("No project_id")?;
        move || {
            let conn = db_clone.conn();
            conn.query_row(
                "SELECT path, name FROM projects WHERE id = ?",
                [project_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let project_path = Path::new(&project_path);

    // Build the prompt based on doc category
    let prompt = build_generation_prompt(db, project_path, task).await?;

    // Get LLM client (use default provider)
    let client = llm_factory
        .get_provider(Provider::DeepSeek)
        .or_else(|| llm_factory.get_provider(Provider::OpenAi))
        .or_else(|| llm_factory.get_provider(Provider::Gemini))
        .ok_or("No LLM provider available for documentation generation")?;

    // Generate documentation
    let messages = PromptBuilder::for_documentation()
        .build_messages(prompt);

    let result = client.chat(messages, None).await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    let draft_content = result.content.as_ref()
        .ok_or("No content in LLM response")?
        .clone();

    // Calculate target doc checksum (if file exists)
    let target_path = project_path.join(&task.target_doc_path);
    let target_checksum = if target_path.exists() {
        file_checksum(&target_path).unwrap_or_else(|| "none".to_string())
    } else {
        "none".to_string()
    };

    // Store the draft
    tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let task_id = task.id;
        let draft = draft_content.clone();
        move || {
            let conn = db_clone.conn();
            store_doc_draft(&conn, task_id, &draft, &target_checksum)
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    Ok(())
}

/// Build generation prompt based on doc category
async fn build_generation_prompt(
    db: &Arc<Database>,
    project_path: &Path,
    task: &DocTask,
) -> Result<String, String> {
    let project_id = task.project_id.ok_or("No project_id")?;

    match task.doc_category.as_str() {
        "mcp_tool" => build_mcp_tool_prompt(db, project_path, task).await,
        "module" => build_module_prompt(db, project_path, task).await,
        "public_api" => build_public_api_prompt(db, project_path, task).await,
        "config" => build_config_prompt(db, project_path, task).await,
        "contributing" => build_contributing_prompt(db, project_path, project_id).await,
        "readme" => build_readme_prompt(db, project_path, project_id).await,
        _ => build_generic_prompt(db, project_path, task).await,
    }
}

/// Build prompt for MCP tool documentation
async fn build_mcp_tool_prompt(
    _db: &Arc<Database>,
    project_path: &Path,
    task: &DocTask,
) -> Result<String, String> {
    let tool_name = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    // Read the tool implementation from mcp/mod.rs
    let mcp_mod_path = project_path.join("crates/mira-server/src/mcp/mod.rs");
    let mcp_content = tokio::task::spawn_blocking(move || {
        read_file_content(&mcp_mod_path).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    // Extract the specific tool function
    let tool_code = extract_tool_function(&mcp_content, tool_name);

    Ok(format!(
        r#"Generate comprehensive documentation for the MCP tool `{tool_name}`.

Tool source code:
```rust
{tool_code}
```

Create documentation that includes:
1. **Purpose**: What the tool does and when to use it
2. **Parameters**: All parameters with types and descriptions
3. **Return Value**: What the tool returns
4. **Example Usage**: Practical example of calling the tool
5. **Error Conditions**: Common errors and how to handle them

Write in clear markdown suitable for `docs/tools/{tool_name}.md`."#
    ))
}

/// Extract a tool function from mcp/mod.rs content
fn extract_tool_function(content: &str, tool_name: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut in_target_fn = false;
    let mut fn_lines = Vec::new();
    let mut brace_count = 0;

    for line in &lines {
        if line.trim().starts_with("#[tool(") {
            // Check if next line is our target function
            continue;
        }

        if line.trim().starts_with(&format!("async fn {}", tool_name)) {
            in_target_fn = true;
        }

        if in_target_fn {
            fn_lines.push(*line);
            brace_count += line.matches('{').count() as i32;
            brace_count -= line.matches('}').count() as i32;

            if brace_count == 0 && fn_lines.len() > 5 {
                break;
            }
        }
    }

    if fn_lines.is_empty() {
        format!("// Tool function for {} not found", tool_name)
    } else {
        fn_lines.join("\n")
    }
}

/// Build prompt for module documentation
async fn build_module_prompt(
    db: &Arc<Database>,
    project_path: &Path,
    task: &DocTask,
) -> Result<String, String> {
    let module_id = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    // Get module info from database
    let module_info = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let project_id = task.project_id.ok_or("No project_id")?;
        let module_id = module_id.to_string();
        move || {
            let conn = db_clone.conn();
            conn.query_row(
                "SELECT * FROM codebase_modules WHERE project_id = ? AND module_id = ?",
                params![project_id, module_id],
                |row| {
                    Ok((
                        row.get::<_, String>("module_id")?,
                        row.get::<_, String>("purpose")?,
                        row.get::<_, String>("exports")?,
                        row.get::<_, String>("depends_on")?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let (module_id, purpose, exports, depends_on) = match module_info {
        Some(info) => info,
        None => {
            return Ok(format!(
                "Generate architecture documentation for the `{}` module. \
                The module structure and key exports should be documented.",
                module_id
            ))
        }
    };

    // Get module code
    let module_path = project_path.join(&format!("src/{}.rs", module_id.replace("::", "/")));
    let module_code = tokio::task::spawn_blocking(move || {
        read_file_content(&module_path).unwrap_or_else(|_| "// Code not found".to_string())
    })
    .await
    .unwrap_or_default();

    Ok(format!(
        r#"Generate architecture documentation for the `{module_id}` module.

**Purpose:** {purpose}

**Key Exports:** {exports}

**Dependencies:** {depends_on}

Module source code:
```rust
// First 500 lines of the module:
{}
```

Create documentation that includes:
1. **Overview**: What the module does and its role in the architecture
2. **Key Components**: Major structs, enums, and functions
3. **Data Flow**: How data moves through the module
4. **Dependencies**: What other modules it depends on and why
5. **Usage Notes**: Important patterns or conventions when using this module

Write in clear markdown suitable for `docs/modules/{module_id}.md`."#,
        module_code.lines().take(500).collect::<Vec<_>>().join("\n")
    ))
}

/// Build prompt for public API documentation
async fn build_public_api_prompt(
    _db: &Arc<Database>,
    _project_path: &Path,
    task: &DocTask,
) -> Result<String, String> {
    let api_name = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    Ok(format!(
        r#"Generate API documentation for `{}`.

Create documentation that includes:
1. **Overview**: What this API is and what it's used for
2. **Function Signature**: The complete function signature with types
3. **Parameters**: Description of each parameter
4. **Return Value**: What the function returns and possible error conditions
5. **Examples**: Practical usage examples
6. **Notes**: Any important caveats or gotchas

Write in clear markdown suitable for `docs/api/{}.md`."#,
        api_name, api_name
    ))
}

/// Build prompt for configuration documentation
async fn build_config_prompt(
    _db: &Arc<Database>,
    _project_path: &Path,
    _task: &DocTask,
) -> Result<String, String> {
    Ok(r#"Generate comprehensive configuration documentation.

Include:
1. **Environment Variables**: All required and optional environment variables
2. **Configuration Files**: File locations and format
3. **Default Values**: What each setting defaults to
4. **Examples**: Sample configuration for common setups

Write in clear markdown suitable for `docs/CONFIGURATION.md`."#.to_string())
}

/// Build prompt for contributing guide
async fn build_contributing_prompt(
    _db: &Arc<Database>,
    _project_path: &Path,
    _project_id: i64,
) -> Result<String, String> {
    Ok(r#"Generate a contributing guide for this project.

Include:
1. **Getting Started**: How to set up development environment
2. **Code Style**: Formatting and naming conventions
3. **Testing**: How to run and write tests
4. **Submitting Changes**: Pull request process
5. **Code Review**: What reviewers look for

Write in clear markdown suitable for `CONTRIBUTING.md`."#.to_string())
}

/// Build prompt for README
async fn build_readme_prompt(
    _db: &Arc<Database>,
    _project_path: &Path,
    _project_id: i64,
) -> Result<String, String> {
    Ok(r#"Generate a comprehensive README for this project.

Include:
1. **Project Title and Tagline**: Brief description
2. **Features**: Key capabilities in bullet points
3. **Quick Start**: Installation and first steps
4. **Usage**: Basic usage examples
5. **Documentation Links**: Links to other docs
6. **Contributing**: Brief note about contributions
7. **License**: License information

Write in clear markdown suitable for `README.md`."#.to_string())
}

/// Build generic documentation prompt
async fn build_generic_prompt(
    _db: &Arc<Database>,
    _project_path: &Path,
    task: &DocTask,
) -> Result<String, String> {
    Ok(format!(
        r#"Generate documentation for `{}`.

Reason for documentation: {}

Create comprehensive, clear markdown documentation that helps developers understand and use this aspect of the codebase."#,
        task.target_doc_path,
        task.reason.as_deref().unwrap_or("missing documentation")
    ))
}
