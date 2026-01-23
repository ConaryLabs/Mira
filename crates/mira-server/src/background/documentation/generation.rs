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

/// Detected project language/type
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ProjectContext {
    language: String,
    framework: Option<String>,  // For future use (e.g., React, Django, Actix)
    is_mcp_server: bool,
}

/// Detect project language from manifest files
fn detect_project_language(project_path: &Path) -> ProjectContext {
    // Check for Rust (Cargo.toml)
    if project_path.join("Cargo.toml").exists() {
        let is_mcp = project_path.join("crates/mira-server/src/mcp").exists()
            || project_path.join("src/mcp").exists()
            || project_path.join(".mcp.json").exists();
        return ProjectContext {
            language: "Rust".to_string(),
            framework: None,
            is_mcp_server: is_mcp,
        };
    }

    // Check for Python (pyproject.toml, setup.py, requirements.txt)
    if project_path.join("pyproject.toml").exists()
        || project_path.join("setup.py").exists()
        || project_path.join("requirements.txt").exists()
    {
        return ProjectContext {
            language: "Python".to_string(),
            framework: None,
            is_mcp_server: project_path.join(".mcp.json").exists(),
        };
    }

    // Check for Node.js/TypeScript (package.json)
    if project_path.join("package.json").exists() {
        let is_typescript = project_path.join("tsconfig.json").exists();
        return ProjectContext {
            language: if is_typescript { "TypeScript".to_string() } else { "JavaScript".to_string() },
            framework: None,
            is_mcp_server: project_path.join(".mcp.json").exists(),
        };
    }

    // Check for Go (go.mod)
    if project_path.join("go.mod").exists() {
        return ProjectContext {
            language: "Go".to_string(),
            framework: None,
            is_mcp_server: project_path.join(".mcp.json").exists(),
        };
    }

    // Default
    ProjectContext {
        language: "Unknown".to_string(),
        framework: None,
        is_mcp_server: false,
    }
}

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

    // Detect project language/type
    let project_ctx = detect_project_language(project_path);
    tracing::debug!("Documentation: detected project language {:?}", project_ctx);

    // Build the prompt based on doc category
    let prompt = build_generation_prompt(db, project_path, &project_ctx, task).await?;

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
    project_ctx: &ProjectContext,
    task: &DocTask,
) -> Result<String, String> {
    let project_id = task.project_id.ok_or("No project_id")?;

    match task.doc_category.as_str() {
        "mcp_tool" => build_mcp_tool_prompt(db, project_path, project_ctx, task).await,
        "module" => build_module_prompt(db, project_path, project_ctx, task).await,
        "public_api" => build_public_api_prompt(project_path, project_ctx, task).await,
        "config" => build_config_prompt(project_path, project_ctx).await,
        "contributing" => build_contributing_prompt(project_path, project_ctx).await,
        "readme" => build_readme_prompt(db, project_path, project_ctx, project_id).await,
        _ => build_generic_prompt(project_path, project_ctx, task).await,
    }
}

/// Build prompt for MCP tool documentation
async fn build_mcp_tool_prompt(
    db: &Arc<Database>,
    project_path: &Path,
    project_ctx: &ProjectContext,
    task: &DocTask,
) -> Result<String, String> {
    let tool_name = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    let project_id = task.project_id.ok_or("No project_id")?;

    // Read the tool implementation from mcp/mod.rs
    let mcp_mod_path = project_path.join("crates/mira-server/src/mcp/mod.rs");
    let mcp_content = tokio::task::spawn_blocking(move || {
        read_file_content(&mcp_mod_path).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    // Extract the specific tool function
    let tool_code = extract_tool_function(&mcp_content, tool_name);

    // Get the request type for this tool from symbols
    let request_type_name = format!("{}Request", to_pascal_case(tool_name));
    let request_type_info = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let type_name = request_type_name.clone();
        move || {
            let conn = db_clone.conn();
            conn.query_row(
                "SELECT signature FROM code_symbols
                 WHERE project_id = ? AND name = ? AND symbol_type = 'struct'",
                params![project_id, type_name],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))?
    .ok()
    .flatten()
    .flatten()
    .unwrap_or_else(|| format!("// {} definition not found", request_type_name));

    let language = &project_ctx.language;
    let code_fence_lang = language.to_lowercase();

    // Only include MCP context if this is actually an MCP server project
    let mcp_context = if project_ctx.is_mcp_server {
        "This is an **MCP (Model Context Protocol) tool**. MCP tools are called via JSON-RPC. \
         Claude Code and other MCP clients invoke these tools by name with JSON parameters."
    } else {
        "This is a tool/function in the project."
    };

    let example_section = if project_ctx.is_mcp_server {
        format!(r#"4. **Example Usage**: Show how to call this tool via MCP. Example format:
   ```json
   {{
     "tool": "{tool_name}",
     "parameters": {{
       "field1": "value1",
       "field2": 123
     }}
   }}
   ```"#, tool_name = tool_name)
    } else {
        format!("4. **Example Usage**: Show how to call this function in {}", language)
    };

    Ok(format!(
        r#"You are documenting a **{language}** project.

{mcp_context}

## Tool: `{tool_name}`

### Request Type
```{code_lang}
{request_type}
```

### Implementation
```{code_lang}
{tool_code}
```

## Instructions

Create documentation that includes:

1. **Purpose**: What the tool does and when to use it

2. **Parameters**: Document each field from the request struct/type above with:
   - Field name
   - Type
   - Whether required or optional
   - Description of what it does

3. **Return Value**: What the tool returns (look at the function return type)

{example}

5. **Error Conditions**: Common errors based on the implementation

IMPORTANT:
- This is a {language} project
- Use the actual parameter types from the request struct/type above
- Don't invent parameters that aren't defined in the code

Write in clear markdown suitable for `docs/tools/{tool_name}.md`."#,
        language = language,
        mcp_context = mcp_context,
        tool_name = tool_name,
        code_lang = code_fence_lang,
        request_type = request_type_info,
        tool_code = tool_code,
        example = example_section,
    ))
}

/// Convert snake_case to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
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

/// Module info from database
struct ModuleInfo {
    module_id: String,
    path: String,
    purpose: String,
    exports: String,
    depends_on: String,
}

/// Build prompt for module documentation
async fn build_module_prompt(
    db: &Arc<Database>,
    project_path: &Path,
    project_ctx: &ProjectContext,
    task: &DocTask,
) -> Result<String, String> {
    let module_id = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    let project_id = task.project_id.ok_or("No project_id")?;
    let language = &project_ctx.language;

    // Get module info from database (including the correct path)
    let module_info = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let module_id = module_id.to_string();
        move || {
            let conn = db_clone.conn();
            conn.query_row(
                "SELECT module_id, path, purpose, exports, depends_on
                 FROM codebase_modules WHERE project_id = ? AND module_id = ?",
                params![project_id, module_id],
                |row| {
                    Ok(ModuleInfo {
                        module_id: row.get("module_id")?,
                        path: row.get("path")?,
                        purpose: row.get("purpose")?,
                        exports: row.get("exports")?,
                        depends_on: row.get("depends_on")?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let info = match module_info {
        Some(info) => info,
        None => {
            return Ok(format!(
                "This is a {} project. Generate architecture documentation for the `{}` module. \
                The module structure and key exports should be documented. \
                Write all code examples in {}.",
                language, module_id, language
            ))
        }
    };

    // Get symbols for this module from the database
    let symbols = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        let module_path = info.path.clone();
        move || {
            let conn = db_clone.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT name, symbol_type, signature
                     FROM code_symbols
                     WHERE project_id = ? AND file_path LIKE ?
                     ORDER BY symbol_type, name
                     LIMIT 50",
                )
                .map_err(|e| e.to_string())?;

            let path_pattern = format!("{}%", module_path);
            stmt.query_map(params![project_id, path_pattern], |row| {
                Ok((
                    row.get::<_, String>("name")?,
                    row.get::<_, String>("symbol_type")?,
                    row.get::<_, Option<String>>("signature")?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))?
    .unwrap_or_default();

    // Format symbols by type
    let mut symbol_sections = String::new();
    let mut current_type = String::new();
    for (name, sym_type, signature) in &symbols {
        if sym_type != &current_type {
            if !current_type.is_empty() {
                symbol_sections.push('\n');
            }
            symbol_sections.push_str(&format!("\n**{}s:**\n", capitalize(sym_type)));
            current_type = sym_type.clone();
        }
        if let Some(sig) = signature {
            symbol_sections.push_str(&format!("- `{}`\n", sig));
        } else {
            symbol_sections.push_str(&format!("- `{}`\n", name));
        }
    }

    // Use the correct path from database to read module code
    let module_path = project_path.join(&info.path);
    let mod_rs_path = module_path.join("mod.rs");
    let single_file_path = module_path.with_extension("rs");

    let module_code = tokio::task::spawn_blocking(move || {
        // Try mod.rs first (directory module), then single file
        if mod_rs_path.exists() {
            read_file_content(&mod_rs_path).unwrap_or_else(|_| "// Code not found".to_string())
        } else if single_file_path.exists() {
            read_file_content(&single_file_path).unwrap_or_else(|_| "// Code not found".to_string())
        } else if module_path.exists() && module_path.is_file() {
            read_file_content(&module_path).unwrap_or_else(|_| "// Code not found".to_string())
        } else {
            format!("// Module path not found: {:?}", module_path)
        }
    })
    .await
    .unwrap_or_else(|_| "// Failed to read module".to_string());

    let code_fence_lang = language.to_lowercase();

    Ok(format!(
        r#"You are documenting a **{language}** project. Generate architecture documentation for the `{module_id}` module.

## Indexed Module Information

**Module ID:** {module_id}
**Path:** {path}
**Purpose:** {purpose}

**Exports:** {exports}

**Dependencies:** {depends_on}

## Symbols Found in Module
{symbols}

## Module Source Code (first 500 lines)
```{code_lang}
{code}
```

## Instructions

Create documentation that includes:
1. **Overview**: What the module does and its role in the architecture (use the Purpose above)
2. **Key Components**: Document the actual structs, enums, and functions listed in Symbols above
3. **Data Flow**: How data moves through the module based on the code
4. **Dependencies**: Explain what the listed dependencies are used for
5. **Usage Examples**: Show {language} code examples of how to use the key exports

IMPORTANT:
- This is a {language} project - all code examples must be in {language}
- Use the actual symbols and signatures shown above, don't invent new ones
- Reference the actual source code provided
- Write clear markdown suitable for `docs/modules/{module_id}.md`"#,
        language = language,
        module_id = info.module_id,
        path = info.path,
        purpose = info.purpose,
        exports = info.exports,
        depends_on = info.depends_on,
        symbols = if symbol_sections.is_empty() { "(no symbols indexed)".to_string() } else { symbol_sections },
        code_lang = code_fence_lang,
        code = module_code.lines().take(500).collect::<Vec<_>>().join("\n"),
    ))
}

/// Capitalize first letter
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Build prompt for public API documentation
async fn build_public_api_prompt(
    _project_path: &Path,
    project_ctx: &ProjectContext,
    task: &DocTask,
) -> Result<String, String> {
    let api_name = task
        .target_doc_path
        .strip_suffix(".md")
        .and_then(|p| p.split('/').last())
        .unwrap_or("unknown");

    let language = &project_ctx.language;

    Ok(format!(
        r#"This is a **{language}** project. Generate API documentation for `{api_name}`.

Create documentation that includes:
1. **Overview**: What this API is and what it's used for
2. **Function Signature**: The complete function signature with types
3. **Parameters**: Description of each parameter
4. **Return Value**: What the function returns and possible error conditions
5. **Examples**: Practical {language} usage examples
6. **Notes**: Any important caveats or gotchas

Write in clear markdown suitable for `docs/api/{api_name}.md`."#,
        language = language,
        api_name = api_name,
    ))
}

/// Build prompt for configuration documentation
async fn build_config_prompt(
    _project_path: &Path,
    project_ctx: &ProjectContext,
) -> Result<String, String> {
    let language = &project_ctx.language;

    Ok(format!(
        r#"This is a **{language}** project. Generate comprehensive configuration documentation.

Include:
1. **Environment Variables**: All required and optional environment variables
2. **Configuration Files**: File locations and format (appropriate for {language} projects)
3. **Default Values**: What each setting defaults to
4. **Examples**: Sample configuration for common setups

Write in clear markdown suitable for `docs/CONFIGURATION.md`."#,
        language = language,
    ))
}

/// Build prompt for contributing guide
async fn build_contributing_prompt(
    _project_path: &Path,
    project_ctx: &ProjectContext,
) -> Result<String, String> {
    let language = &project_ctx.language;

    // Language-specific contributing guidance
    let tooling_notes = match language.as_str() {
        "Rust" => "- Use `cargo fmt` for formatting\n- Use `cargo clippy` for linting\n- Run `cargo test` before submitting",
        "Python" => "- Use `black` or `ruff` for formatting\n- Use `mypy` for type checking\n- Run `pytest` before submitting",
        "TypeScript" | "JavaScript" => "- Use `prettier` for formatting\n- Use `eslint` for linting\n- Run `npm test` before submitting",
        "Go" => "- Use `go fmt` for formatting\n- Use `golangci-lint` for linting\n- Run `go test ./...` before submitting",
        _ => "- Follow the project's established formatting conventions\n- Run tests before submitting",
    };

    Ok(format!(
        r#"This is a **{language}** project. Generate a contributing guide.

Include:
1. **Getting Started**: How to set up the {language} development environment
2. **Code Style**: Formatting and naming conventions for {language}
3. **Testing**: How to run and write tests
4. **Submitting Changes**: Pull request process on GitHub
5. **Code Review**: What reviewers look for

Recommended tooling for {language}:
{tooling}

Write in clear markdown suitable for `CONTRIBUTING.md`."#,
        language = language,
        tooling = tooling_notes,
    ))
}

/// Build prompt for README
async fn build_readme_prompt(
    db: &Arc<Database>,
    _project_path: &Path,
    project_ctx: &ProjectContext,
    project_id: i64,
) -> Result<String, String> {
    let language = &project_ctx.language;

    // Get project name from database
    let project_name = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        move || {
            let conn = db_clone.conn();
            conn.query_row(
                "SELECT name FROM projects WHERE id = ?",
                [project_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
            .unwrap_or_else(|| "Project".to_string())
        }
    })
    .await
    .unwrap_or_else(|_| "Project".to_string());

    // Language-specific installation instructions
    let install_hint = match language.as_str() {
        "Rust" => "cargo install or cargo build --release",
        "Python" => "pip install or poetry install",
        "TypeScript" | "JavaScript" => "npm install or yarn install",
        "Go" => "go install or go build",
        _ => "See installation instructions below",
    };

    Ok(format!(
        r#"This is a **{language}** project called **{name}**. Generate a comprehensive README for GitHub.

Include:
1. **Project Title and Tagline**: Use "{name}" and write a brief compelling description
2. **Badges**: Include appropriate badges (build status, version, license) as applicable for {language} projects
3. **Features**: Key capabilities in bullet points
4. **Quick Start**: Installation via {install_hint}
5. **Usage**: Basic {language} usage examples
6. **Documentation Links**: Links to other docs in the docs/ folder
7. **Contributing**: Brief note about contributions (link to CONTRIBUTING.md)
8. **License**: License information

Write in clear markdown suitable for `README.md` on GitHub."#,
        language = language,
        name = project_name,
        install_hint = install_hint,
    ))
}

/// Build generic documentation prompt
async fn build_generic_prompt(
    _project_path: &Path,
    project_ctx: &ProjectContext,
    task: &DocTask,
) -> Result<String, String> {
    let language = &project_ctx.language;

    Ok(format!(
        r#"This is a **{language}** project. Generate documentation for `{path}`.

Reason for documentation: {reason}

Create comprehensive, clear markdown documentation that helps developers understand and use this aspect of the codebase.
All code examples should be in {language}."#,
        language = language,
        path = task.target_doc_path,
        reason = task.reason.as_deref().unwrap_or("missing documentation")
    ))
}
