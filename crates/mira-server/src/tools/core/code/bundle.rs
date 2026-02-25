// crates/mira-server/src/tools/core/code/bundle.rs
// Context bundling for agent spawning — packages code intelligence into spawn prompts.

use rusqlite::params;
use std::fmt::Write;

use crate::error::MiraError;
use crate::mcp::responses::{BundleData, CodeData, CodeOutput, Json};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};

/// Default character budget (~1500 tokens at ~4 chars/token = 6000 chars)
const DEFAULT_BUDGET: usize = 6000;
/// Minimum allowed budget
const MIN_BUDGET: usize = 500;
/// Maximum allowed budget (50k chars ~ 12.5k tokens)
const MAX_BUDGET: usize = 50_000;

/// Budget allocation percentages
const MODULE_MAP_PCT: usize = 20;
const SYMBOLS_PCT: usize = 30;
const DEPS_PCT: usize = 10;
const SNIPPETS_PCT: usize = 40;

#[derive(Debug, Clone, Copy)]
pub enum BundleDepth {
    /// Module summaries + public API signatures only
    Overview,
    /// Above + key function bodies, dependency graph
    Standard,
    /// Above + full relevant code chunks
    Deep,
}

impl BundleDepth {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.map(|s| s.to_lowercase()).as_deref() {
            Some("overview") => Self::Overview,
            Some("deep") => Self::Deep,
            _ => Self::Standard,
        }
    }
}

/// Internal module data collected from DB
struct ModuleInfo {
    id: String,
    name: String,
    path: String,
    purpose: Option<String>,
    exports: Vec<String>,
    symbol_count: u32,
    line_count: Option<i32>,
}

/// Internal symbol data
struct SymbolEntry {
    name: String,
    symbol_type: String,
    file_path: String,
    start_line: i64,
    signature: Option<String>,
}

/// Internal dependency edge
struct DepEdge {
    source: String,
    target: String,
    call_count: i64,
    import_count: i64,
}

/// Internal code chunk
struct ChunkEntry {
    file_path: String,
    content: String,
    start_line: i64,
}

/// Generate a context bundle for agent spawning.
pub async fn generate_bundle<C: ToolContext>(
    ctx: &C,
    scope: String,
    budget: Option<i64>,
    depth: Option<String>,
) -> Result<Json<CodeOutput>, MiraError> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let budget = budget
        .map(|b| (b.max(0) as usize).clamp(MIN_BUDGET, MAX_BUDGET))
        .unwrap_or(DEFAULT_BUDGET);
    let depth = BundleDepth::from_str_opt(depth.as_deref());

    // Reject empty/whitespace scope
    if scope.trim().is_empty() {
        return Err(MiraError::InvalidInput(
            "scope must not be empty for code(action=bundle)".to_string(),
        ));
    }

    // Resolve scope to a path pattern for DB queries
    let path_pattern = resolve_scope_pattern(&scope);

    // Collect data from code DB
    let (modules, symbols, deps, chunks) =
        collect_bundle_data(ctx, project_id, &path_pattern, &depth).await?;

    if modules.is_empty() {
        // Try semantic search fallback for query-style scopes
        let fallback = try_semantic_fallback(ctx, project_id, &scope, &depth).await?;
        if let Some((mods, syms, dep_edges, code_chunks)) = fallback {
            let content = format_bundle(
                &scope,
                &mods,
                &syms,
                &dep_edges,
                &code_chunks,
                budget,
                &depth,
            );
            let char_count = content.len();
            return Ok(Json(CodeOutput {
                action: "bundle".into(),
                message: format!(
                    "Bundle for '{}': {} modules, {} symbols, {} deps ({} chars)",
                    scope,
                    mods.len(),
                    syms.len(),
                    dep_edges.len(),
                    char_count
                ),
                data: Some(CodeData::Bundle(BundleData { content })),
            }));
        }

        return Ok(Json(CodeOutput {
            action: "bundle".into(),
            message: format!(
                "No indexed modules found for scope '{}'. Run index(action=\"project\") first, or try a different scope.",
                scope
            ),
            data: Some(CodeData::Bundle(BundleData {
                content: String::new(),
            })),
        }));
    }

    let content = format_bundle(&scope, &modules, &symbols, &deps, &chunks, budget, &depth);
    let char_count = content.len();

    Ok(Json(CodeOutput {
        action: "bundle".into(),
        message: format!(
            "Bundle for '{}': {} modules, {} symbols, {} deps ({} chars)",
            scope,
            modules.len(),
            symbols.len(),
            deps.len(),
            char_count
        ),
        data: Some(CodeData::Bundle(BundleData { content })),
    }))
}

/// Resolve a scope string into a SQL LIKE pattern for file_path/module path matching.
fn resolve_scope_pattern(scope: &str) -> String {
    let scope = scope.trim().trim_end_matches('/');

    // Escape SQL LIKE special chars
    let escaped = scope
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");

    format!("{}%", escaped)
}

/// Collect all bundle data from the code database.
async fn collect_bundle_data<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    path_pattern: &str,
    depth: &BundleDepth,
) -> Result<
    (
        Vec<ModuleInfo>,
        Vec<SymbolEntry>,
        Vec<DepEdge>,
        Vec<ChunkEntry>,
    ),
    MiraError,
> {
    let pattern = path_pattern.to_string();
    let include_chunks = !matches!(depth, BundleDepth::Overview);

    type BundleTuple = (
        Vec<ModuleInfo>,
        Vec<SymbolEntry>,
        Vec<DepEdge>,
        Vec<ChunkEntry>,
    );

    let result = ctx
        .code_pool()
        .run(move |conn| -> Result<BundleTuple, MiraError> {
            // 1. Modules matching scope
            let modules = query_modules(conn, project_id, &pattern).map_err(MiraError::Db)?;
            if modules.is_empty() {
                return Ok((vec![], vec![], vec![], vec![]));
            }

            // 2. Symbols in matching files
            let symbols = query_symbols(conn, project_id, &pattern).map_err(MiraError::Db)?;

            // 3. Dependencies involving matching modules
            let module_ids: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
            let deps = query_deps(conn, project_id, &module_ids).map_err(MiraError::Db)?;

            // 4. Code chunks (skip for overview depth)
            let chunks = if include_chunks {
                query_chunks(conn, project_id, &pattern).map_err(MiraError::Db)?
            } else {
                vec![]
            };

            Ok((modules, symbols, deps, chunks))
        })
        .await
        .map_err(|e| MiraError::Other(format!("Bundle query failed: {}", e)))?;

    Ok(result)
}

/// Try semantic search as fallback when path-based matching finds nothing.
async fn try_semantic_fallback<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    query: &str,
    depth: &BundleDepth,
) -> Result<
    Option<(
        Vec<ModuleInfo>,
        Vec<SymbolEntry>,
        Vec<DepEdge>,
        Vec<ChunkEntry>,
    )>,
    MiraError,
> {
    // Only try semantic fallback if scope doesn't look like a path
    if query.contains('/') || query.contains('.') {
        return Ok(None);
    }

    let search_results = super::query_search_code(ctx, query, 20).await?;
    if search_results.results.is_empty() {
        return Ok(None);
    }

    // Extract directory prefixes from search results and pick the most frequent
    let dir_prefixes: Vec<String> = search_results
        .results
        .iter()
        .filter_map(|r| {
            let path = &r.file_path;
            path.rfind('/').map(|idx| path[..idx + 1].to_string())
        })
        .collect();

    if dir_prefixes.is_empty() {
        return Ok(None);
    }

    // Count occurrences to find the most common prefix
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for p in &dir_prefixes {
        *counts.entry(p.as_str()).or_insert(0) += 1;
    }
    let Some(top_prefix) = counts
        .into_iter()
        .max_by(|(a_prefix, a_count), (b_prefix, b_count)| {
            a_count.cmp(b_count).then_with(|| b_prefix.cmp(a_prefix))
        })
        .map(|(prefix, _)| prefix.to_string())
    else {
        return Ok(None);
    };
    let pattern = resolve_scope_pattern(&top_prefix);
    let include_chunks = !matches!(depth, BundleDepth::Overview);

    type FallbackTuple = (
        Vec<ModuleInfo>,
        Vec<SymbolEntry>,
        Vec<DepEdge>,
        Vec<ChunkEntry>,
    );

    let result = ctx
        .code_pool()
        .run(move |conn| -> Result<FallbackTuple, MiraError> {
            let modules = query_modules(conn, project_id, &pattern).map_err(MiraError::Db)?;
            let symbols = query_symbols(conn, project_id, &pattern).map_err(MiraError::Db)?;
            let module_ids: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
            let deps = query_deps(conn, project_id, &module_ids).map_err(MiraError::Db)?;
            let chunks = if include_chunks {
                query_chunks(conn, project_id, &pattern).map_err(MiraError::Db)?
            } else {
                vec![]
            };
            Ok((modules, symbols, deps, chunks))
        })
        .await
        .map_err(|e| MiraError::Other(format!("Bundle semantic fallback failed: {}", e)))?;

    if result.0.is_empty() {
        return Ok(None);
    }

    Ok(Some(result))
}

// ============================================================================
// DB query helpers (run inside code_pool closure)
// ============================================================================

fn query_modules(
    conn: &rusqlite::Connection,
    project_id: i64,
    pattern: &str,
) -> rusqlite::Result<Vec<ModuleInfo>> {
    let mut stmt = conn.prepare(
        "SELECT module_id, name, path, purpose, exports, symbol_count, line_count
         FROM codebase_modules
         WHERE project_id = ? AND (path LIKE ? ESCAPE '\\' OR module_id LIKE ? ESCAPE '\\')
         ORDER BY path",
    )?;

    let rows = stmt
        .query_map(params![project_id, pattern, pattern], |row| {
            let exports_json: String = row.get(4)?;
            let exports: Vec<String> =
                serde_json::from_str(&exports_json).unwrap_or_else(|e| {
                    tracing::debug!("Failed to parse module exports JSON: {}", e);
                    vec![]
                });
            Ok(ModuleInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                purpose: row.get(3)?,
                exports,
                symbol_count: row.get(5)?,
                line_count: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

fn query_symbols(
    conn: &rusqlite::Connection,
    project_id: i64,
    pattern: &str,
) -> rusqlite::Result<Vec<SymbolEntry>> {
    let mut stmt = conn.prepare(
        "SELECT name, symbol_type, file_path, start_line, signature
         FROM code_symbols
         WHERE project_id = ? AND file_path LIKE ? ESCAPE '\\'
         ORDER BY file_path, start_line
         LIMIT 200",
    )?;

    let rows = stmt
        .query_map(params![project_id, pattern], |row| {
            Ok(SymbolEntry {
                name: row.get(0)?,
                symbol_type: row.get(1)?,
                file_path: row.get(2)?,
                start_line: row.get(3)?,
                signature: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

fn query_deps(
    conn: &rusqlite::Connection,
    project_id: i64,
    module_ids: &[&str],
) -> rusqlite::Result<Vec<DepEdge>> {
    if module_ids.is_empty() {
        return Ok(vec![]);
    }

    // Build IN clause — safe since module_ids come from our own DB query
    let placeholders: Vec<String> = module_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();
    let in_clause = placeholders.join(",");

    let sql = format!(
        "SELECT source_module_id, target_module_id, call_count, import_count
         FROM module_dependencies
         WHERE project_id = ?
           AND (source_module_id IN ({in_clause}) OR target_module_id IN ({in_clause}))
         ORDER BY call_count + import_count DESC
         LIMIT 50"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(DepEdge {
                source: row.get(0)?,
                target: row.get(1)?,
                call_count: row.get(2)?,
                import_count: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

fn query_chunks(
    conn: &rusqlite::Connection,
    project_id: i64,
    pattern: &str,
) -> rusqlite::Result<Vec<ChunkEntry>> {
    let mut stmt = conn.prepare(
        "SELECT file_path, chunk_content, start_line
         FROM code_chunks
         WHERE project_id = ? AND file_path LIKE ? ESCAPE '\\'
         ORDER BY file_path, start_line
         LIMIT 100",
    )?;

    let rows = stmt
        .query_map(params![project_id, pattern], |row| {
            Ok(ChunkEntry {
                file_path: row.get(0)?,
                content: row.get(1)?,
                start_line: row.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

// ============================================================================
// Formatting
// ============================================================================

fn format_bundle(
    scope: &str,
    modules: &[ModuleInfo],
    symbols: &[SymbolEntry],
    deps: &[DepEdge],
    chunks: &[ChunkEntry],
    budget: usize,
    depth: &BundleDepth,
) -> String {
    let mod_budget = budget * MODULE_MAP_PCT / 100;
    let sym_budget = budget * SYMBOLS_PCT / 100;
    let dep_budget = budget * DEPS_PCT / 100;
    let snip_budget = budget * SNIPPETS_PCT / 100;

    let mut out = String::with_capacity(budget);

    let _ = writeln!(out, "## Bundle: {}\n", scope);

    // Section 1: Module Map
    let _ = writeln!(out, "### Module Map");
    let mut section_len = 0;
    for m in modules {
        let purpose = m.purpose.as_deref().unwrap_or("(no summary)");
        let exports_hint = if m.exports.is_empty() {
            String::new()
        } else {
            format!(" exports: {}", m.exports.join(", "))
        };
        let line = format!(
            "- **{}** ({}) -- {} [{} symbols, {}L{}]\n",
            m.name,
            m.path,
            purpose,
            m.symbol_count,
            m.line_count.unwrap_or(0),
            exports_hint
        );
        if section_len + line.len() > mod_budget {
            let _ = writeln!(out, "- ... ({} more modules)", modules.len());
            break;
        }
        out.push_str(&line);
        section_len += line.len();
    }
    out.push('\n');

    // Section 2: Key Symbols (with signatures for standard+deep)
    let _ = writeln!(out, "### Key Symbols");
    let mut section_len = 0;
    let show_signatures = !matches!(depth, BundleDepth::Overview);

    // Group by file for readability
    let mut current_file = "";
    for s in symbols {
        if s.file_path != current_file {
            let header = format!("**{}**:\n", s.file_path);
            if section_len + header.len() > sym_budget {
                let _ = writeln!(out, "... ({} more symbols)", symbols.len());
                break;
            }
            out.push_str(&header);
            section_len += header.len();
            current_file = &s.file_path;
        }

        let line = if show_signatures {
            if let Some(sig) = &s.signature {
                format!("- `{}` [L{}]\n", truncate_str(sig, 120), s.start_line)
            } else {
                format!("- {} `{}` [L{}]\n", s.symbol_type, s.name, s.start_line)
            }
        } else {
            format!("- {} `{}` [L{}]\n", s.symbol_type, s.name, s.start_line)
        };

        if section_len + line.len() > sym_budget {
            let _ = writeln!(out, "... (truncated)");
            break;
        }
        out.push_str(&line);
        section_len += line.len();
    }
    out.push('\n');

    // Section 3: Dependencies
    if !deps.is_empty() {
        let _ = writeln!(out, "### Dependencies");
        let mut section_len = 0;
        for d in deps {
            let line = format!(
                "- {} -> {} ({} calls, {} imports)\n",
                d.source, d.target, d.call_count, d.import_count
            );
            if section_len + line.len() > dep_budget {
                let _ = writeln!(out, "... ({} more edges)", deps.len());
                break;
            }
            out.push_str(&line);
            section_len += line.len();
        }
        out.push('\n');
    }

    // Section 4: Code Snippets (standard+deep only)
    if !chunks.is_empty() && !matches!(depth, BundleDepth::Overview) {
        let _ = writeln!(out, "### Code");
        let mut section_len = 0;
        let mut current_file = "";

        for c in chunks {
            if c.file_path != current_file {
                let header = format!("**{}**:\n", c.file_path);
                if section_len + header.len() > snip_budget {
                    break;
                }
                out.push_str(&header);
                section_len += header.len();
                current_file = &c.file_path;
            }

            // For deep, include full chunks; for standard, truncate
            let max_chunk = match depth {
                BundleDepth::Deep => 2000,
                _ => 500,
            };

            let content = truncate_str(&c.content, max_chunk);
            let snippet = format!("```\n// L{}\n{}\n```\n", c.start_line, content);

            if section_len + snippet.len() > snip_budget {
                break;
            }
            out.push_str(&snippet);
            section_len += snippet.len();
        }
    }

    // Final truncation to hard budget (reserve space for suffix)
    if out.len() > budget {
        let suffix = "\n... (truncated to budget)";
        let truncate_at = budget.saturating_sub(suffix.len());
        // Find safe UTF-8 boundary
        let mut end = truncate_at;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push_str(suffix);
    }

    out
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Find a safe UTF-8 boundary
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Helper matching the clamping logic in generate_bundle
    fn clamp_budget(b: i64) -> usize {
        (b.max(0) as usize).clamp(MIN_BUDGET, MAX_BUDGET)
    }

    #[test]
    fn budget_clamped_negative() {
        assert_eq!(clamp_budget(-1), MIN_BUDGET);
    }

    #[test]
    fn budget_clamped_excessive() {
        assert_eq!(clamp_budget(999_999), MAX_BUDGET);
    }

    #[test]
    fn budget_normal_passthrough() {
        assert_eq!(clamp_budget(8000), 8000);
    }

    #[test]
    fn resolve_scope_escapes_sql_wildcards() {
        assert_eq!(resolve_scope_pattern("src/tools"), "src/tools%");
        assert_eq!(resolve_scope_pattern("src/tools/"), "src/tools%");
        assert_eq!(resolve_scope_pattern("foo_bar"), "foo\\_bar%");
        assert_eq!(resolve_scope_pattern("100%"), "100\\%%");
    }

    #[test]
    fn format_bundle_respects_budget() {
        let modules = vec![ModuleInfo {
            id: "m1".into(),
            name: "test_mod".into(),
            path: "src/test_mod.rs".into(),
            purpose: Some("A test module".into()),
            exports: vec!["foo".into()],
            symbol_count: 5,
            line_count: Some(100),
        }];
        let symbols = vec![SymbolEntry {
            name: "foo".into(),
            symbol_type: "function".into(),
            file_path: "src/test_mod.rs".into(),
            start_line: 10,
            signature: Some("pub fn foo(x: i32) -> bool".into()),
        }];

        let budget = 800;
        let result = format_bundle(
            "src/test_mod",
            &modules,
            &symbols,
            &[],
            &[],
            budget,
            &BundleDepth::Standard,
        );
        assert!(
            result.len() <= budget,
            "output {} exceeds budget {}",
            result.len(),
            budget
        );
    }

    #[test]
    fn format_bundle_truncation_stays_within_budget() {
        // Generate content that will exceed a tiny budget
        let modules: Vec<ModuleInfo> = (0..50)
            .map(|i| ModuleInfo {
                id: format!("m{}", i),
                name: format!("module_{}", i),
                path: format!("src/mod_{}.rs", i),
                purpose: Some(format!(
                    "Module {} does things with long description text",
                    i
                )),
                exports: vec![],
                symbol_count: 10,
                line_count: Some(200),
            })
            .collect();

        let budget = 500;
        let result = format_bundle(
            "src/",
            &modules,
            &[],
            &[],
            &[],
            budget,
            &BundleDepth::Overview,
        );
        assert!(
            result.len() <= budget,
            "output len {} exceeds budget {}",
            result.len(),
            budget
        );
    }

    #[test]
    fn truncate_str_handles_utf8() {
        let s = "hello world";
        assert_eq!(truncate_str(s, 5), "hello");
        assert_eq!(truncate_str(s, 100), "hello world");

        // Multi-byte: each char is 4 bytes
        let emoji = "\u{1F600}\u{1F601}"; // 8 bytes
        let t = truncate_str(emoji, 5);
        assert_eq!(t.len(), 4); // truncates to first emoji
    }

    #[test]
    fn most_common_prefix_wins() {
        let prefixes = vec![
            "src/tools/".to_string(),
            "src/db/".to_string(),
            "src/tools/".to_string(),
            "src/tools/".to_string(),
            "src/db/".to_string(),
        ];
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for p in &prefixes {
            *counts.entry(p.as_str()).or_insert(0) += 1;
        }
        let top = counts
            .into_iter()
            .max_by(|(a_prefix, a_count), (b_prefix, b_count)| {
                a_count.cmp(b_count).then_with(|| b_prefix.cmp(a_prefix))
            })
            .map(|(prefix, _)| prefix.to_string())
            .unwrap();
        assert_eq!(top, "src/tools/");
    }

    #[test]
    fn tied_prefixes_resolve_deterministically() {
        // Equal frequency: should pick lexicographically first (shortest/earliest path)
        let prefixes = vec!["src/db/".to_string(), "src/tools/".to_string()];
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for p in &prefixes {
            *counts.entry(p.as_str()).or_insert(0) += 1;
        }
        let top = counts
            .into_iter()
            .max_by(|(a_prefix, a_count), (b_prefix, b_count)| {
                a_count.cmp(b_count).then_with(|| b_prefix.cmp(a_prefix))
            })
            .map(|(prefix, _)| prefix.to_string())
            .unwrap();
        assert_eq!(top, "src/db/");
    }
}
