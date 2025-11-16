// src/operations/engine/code_handlers.rs
// Handlers for code intelligence operations - exposes AST analysis to LLM

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{info, warn};

use crate::memory::features::code_intelligence::CodeIntelligenceService;

/// Handles code intelligence operations
pub struct CodeHandlers {
    code_intelligence: Arc<CodeIntelligenceService>,
}

impl CodeHandlers {
    pub fn new(code_intelligence: Arc<CodeIntelligenceService>) -> Self {
        Self { code_intelligence }
    }

    /// Execute a code intelligence tool call
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        match tool_name {
            "find_function_internal" => self.find_function(args).await,
            "find_class_or_struct_internal" => self.find_class_or_struct(args).await,
            "search_code_semantic_internal" => self.search_code_semantic(args).await,
            "find_imports_internal" => self.find_imports(args).await,
            "analyze_dependencies_internal" => self.analyze_dependencies(args).await,
            "get_complexity_hotspots_internal" => self.get_complexity_hotspots(args).await,
            "get_quality_issues_internal" => self.get_quality_issues(args).await,
            "get_file_symbols_internal" => self.get_file_symbols(args).await,
            "find_tests_for_code_internal" => self.find_tests_for_code(args).await,
            "get_codebase_stats_internal" => self.get_codebase_stats(args).await,
            "find_callers_internal" => self.find_callers(args).await,
            "get_element_definition_internal" => self.get_element_definition(args).await,
            _ => Err(anyhow::anyhow!("Unknown code tool: {}", tool_name)),
        }
    }

    /// Find function definitions by name pattern
    async fn find_function(&self, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .context("Missing name parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let include_tests = args
            .get("include_tests")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(false);
        let min_complexity = args
            .get("min_complexity")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i32>().ok());

        info!("[CODE] Finding functions matching: {}", name);

        // Use search_elements_for_project from CodeIntelligenceService
        match self
            .code_intelligence
            .search_elements_for_project(name, project_id, limit)
            .await
        {
            Ok(elements) => {
                // Filter for functions only
                let functions: Vec<Value> = elements
                    .iter()
                    .filter(|e| e.element_type == "function")
                    .filter(|e| include_tests || !e.is_test)
                    .filter(|e| {
                        if let Some(min_comp) = min_complexity {
                            e.complexity_score >= min_comp
                        } else {
                            true
                        }
                    })
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "file_path": e.full_path.split("::").next().unwrap_or(""),
                            "full_path": e.full_path,
                            "visibility": e.visibility,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                            "complexity": e.complexity_score,
                            "is_async": e.is_async,
                            "is_test": e.is_test,
                            "documentation": e.documentation.clone().unwrap_or_default(),
                            "signature": e.content.lines().next().unwrap_or("").to_string(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": functions.len(),
                    "functions": functions,
                    "message": format!("Found {} function(s) matching '{}'", functions.len(), name)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to find functions: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "functions": []
                }))
            }
        }
    }

    /// Find class/struct/enum definitions
    async fn find_class_or_struct(&self, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .context("Missing name parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let include_private = args
            .get("include_private")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(false);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i32>().ok());

        info!("[CODE] Finding classes/structs matching: {}", name);

        match self
            .code_intelligence
            .search_elements_for_project(name, project_id, limit)
            .await
        {
            Ok(elements) => {
                let types: Vec<Value> = elements
                    .iter()
                    .filter(|e| matches!(e.element_type.as_str(), "class" | "struct" | "enum"))
                    .filter(|e| include_private || e.visibility != "private")
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "type": e.element_type,
                            "file_path": e.full_path.split("::").next().unwrap_or(""),
                            "full_path": e.full_path,
                            "visibility": e.visibility,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                            "documentation": e.documentation.clone().unwrap_or_default(),
                            "preview": e.content.lines().take(10).collect::<Vec<_>>().join("\n"),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": types.len(),
                    "types": types,
                    "message": format!("Found {} type(s) matching '{}'", types.len(), name)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to find types: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "types": []
                }))
            }
        }
    }

    /// Semantic code search using vector embeddings
    async fn search_code_semantic(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .context("Missing query parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(10);

        info!("[CODE] Semantic search: {}", query);

        match self
            .code_intelligence
            .search_code(query, project_id, limit)
            .await
        {
            Ok(results) => {
                let code_results: Vec<Value> = results
                    .iter()
                    .map(|entry| {
                        json!({
                            "content": entry.content,
                            "role": entry.role,
                            "tags": entry.tags.clone().unwrap_or_default(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": code_results.len(),
                    "results": code_results,
                    "query": query,
                    "message": format!("Found {} semantically relevant code elements", code_results.len())
                }))
            }
            Err(e) => {
                warn!("[CODE] Semantic search failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "results": []
                }))
            }
        }
    }

    /// Find imports of a symbol (placeholder - needs implementation in CodeIntelligenceService)
    async fn find_imports(&self, args: Value) -> Result<Value> {
        let symbol = args
            .get("symbol")
            .and_then(|v| v.as_str())
            .context("Missing symbol parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50);

        info!("[CODE] Finding imports of: {}", symbol);

        // Use semantic search to find imports mentioning the symbol
        let search_query = format!("import {}", symbol);
        match self
            .code_intelligence
            .search_code(&search_query, project_id, limit)
            .await
        {
            Ok(results) => {
                let imports: Vec<Value> = results
                    .iter()
                    .filter(|entry| {
                        entry.content.contains("import") || entry.content.contains("use")
                    })
                    .map(|entry| {
                        json!({
                            "content": entry.content,
                            "tags": entry.tags.clone().unwrap_or_default(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": imports.len(),
                    "imports": imports,
                    "symbol": symbol,
                    "message": format!("Found {} file(s) importing '{}'", imports.len(), symbol)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to find imports: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "imports": []
                }))
            }
        }
    }

    /// Analyze dependencies (placeholder - needs DB query implementation)
    async fn analyze_dependencies(&self, args: Value) -> Result<Value> {
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let _file_path = args.get("file_path").and_then(|v| v.as_str());
        let _group_by = args
            .get("group_by")
            .and_then(|v| v.as_str())
            .unwrap_or("type");

        info!("[CODE] Analyzing dependencies for project: {}", project_id);

        // TODO: Implement dependency analysis using external_dependencies table
        // For now, return placeholder
        Ok(json!({
            "success": true,
            "message": "Dependency analysis coming soon",
            "dependencies": {
                "npm_packages": [],
                "local_imports": [],
                "std_lib": []
            }
        }))
    }

    /// Get complexity hotspots
    async fn get_complexity_hotspots(&self, args: Value) -> Result<Value> {
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let min_complexity = args
            .get("min_complexity")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(10);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i32>().ok());

        info!("[CODE] Getting complexity hotspots (min: {})", min_complexity);

        match self
            .code_intelligence
            .get_complexity_hotspots_for_project(project_id, limit)
            .await
        {
            Ok(elements) => {
                let hotspots: Vec<Value> = elements
                    .iter()
                    .filter(|e| e.complexity_score >= min_complexity)
                    .map(|e| {
                        let severity = if e.complexity_score > 20 {
                            "critical"
                        } else if e.complexity_score > 15 {
                            "high"
                        } else {
                            "medium"
                        };

                        json!({
                            "name": e.name,
                            "file_path": e.full_path.split("::").next().unwrap_or(""),
                            "full_path": e.full_path,
                            "complexity": e.complexity_score,
                            "severity": severity,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                            "suggestion": "Consider breaking this function into smaller, more focused functions"
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": hotspots.len(),
                    "hotspots": hotspots,
                    "message": format!("Found {} complexity hotspot(s)", hotspots.len())
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to get complexity hotspots: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "hotspots": []
                }))
            }
        }
    }

    /// Get quality issues (placeholder - needs full implementation)
    async fn get_quality_issues(&self, args: Value) -> Result<Value> {
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let _file_path = args.get("file_path").and_then(|v| v.as_str());
        let _severity = args.get("severity").and_then(|v| v.as_str());
        let _issue_type = args.get("issue_type").and_then(|v| v.as_str());
        let _limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50);

        info!("[CODE] Getting quality issues for project: {}", project_id);

        // TODO: Query code_quality_issues table
        // For now, return placeholder
        Ok(json!({
            "success": true,
            "count": 0,
            "issues": [],
            "message": "Quality issue analysis coming soon"
        }))
    }

    /// Get file symbols
    async fn get_file_symbols(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("Missing file_path parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let _include_private = args
            .get("include_private")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(true);
        let include_content = args
            .get("include_content")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(false);

        info!("[CODE] Getting symbols for file: {}", file_path);

        // Use search_elements_for_project with file path pattern
        let search_pattern = format!("{}%", file_path);
        match self
            .code_intelligence
            .search_elements_for_project(&search_pattern, project_id, Some(100))
            .await
        {
            Ok(elements) => {
                // Filter to exact file path match
                let file_elements: Vec<Value> = elements
                    .iter()
                    .filter(|e| e.full_path.starts_with(file_path))
                    .map(|e| {
                        let mut symbol = json!({
                            "name": e.name,
                            "type": e.element_type,
                            "visibility": e.visibility,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                            "complexity": e.complexity_score,
                            "is_async": e.is_async,
                            "is_test": e.is_test,
                            "documentation": e.documentation.clone().unwrap_or_default(),
                        });

                        if include_content {
                            symbol["content"] = json!(e.content);
                        }

                        symbol
                    })
                    .collect();

                // Group by type
                let mut functions = Vec::new();
                let mut classes = Vec::new();
                let mut others = Vec::new();

                for elem in file_elements {
                    match elem["type"].as_str().unwrap_or("") {
                        "function" => functions.push(elem),
                        "class" | "struct" | "enum" => classes.push(elem),
                        _ => others.push(elem),
                    }
                }

                Ok(json!({
                    "success": true,
                    "file_path": file_path,
                    "summary": {
                        "total_symbols": functions.len() + classes.len() + others.len(),
                        "functions": functions.len(),
                        "classes": classes.len(),
                        "others": others.len()
                    },
                    "symbols": {
                        "functions": functions,
                        "classes": classes,
                        "others": others
                    },
                    "message": format!("Found {} symbol(s) in {}", functions.len() + classes.len() + others.len(), file_path)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to get file symbols: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "symbols": {}
                }))
            }
        }
    }

    /// Find tests for code element
    async fn find_tests_for_code(&self, args: Value) -> Result<Value> {
        let element_name = args
            .get("element_name")
            .and_then(|v| v.as_str())
            .context("Missing element_name parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let _file_path = args.get("file_path").and_then(|v| v.as_str());

        info!("[CODE] Finding tests for: {}", element_name);

        // Search for test functions mentioning the element name
        let search_pattern = format!("%{}%", element_name);
        match self
            .code_intelligence
            .search_elements_for_project(&search_pattern, project_id, Some(50))
            .await
        {
            Ok(elements) => {
                let tests: Vec<Value> = elements
                    .iter()
                    .filter(|e| e.is_test)
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "file_path": e.full_path.split("::").next().unwrap_or(""),
                            "full_path": e.full_path,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                            "test_type": if e.name.contains("integration") { "integration" } else { "unit" },
                            "preview": e.content.lines().take(5).collect::<Vec<_>>().join("\n"),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": tests.len(),
                    "tests": tests,
                    "element_name": element_name,
                    "message": format!("Found {} test(s) for '{}'", tests.len(), element_name)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to find tests: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "tests": []
                }))
            }
        }
    }

    /// Get codebase statistics
    async fn get_codebase_stats(&self, args: Value) -> Result<Value> {
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let _breakdown_by = args
            .get("breakdown_by")
            .and_then(|v| v.as_str())
            .unwrap_or("language");

        info!("[CODE] Getting codebase stats for project: {}", project_id);

        // Use get_repo_stats if we have attachment_id, otherwise aggregate from elements
        // For now, use search to get overview
        match self
            .code_intelligence
            .search_elements_for_project("%", project_id, Some(1000))
            .await
        {
            Ok(elements) => {
                let total_elements = elements.len();
                let functions = elements.iter().filter(|e| e.element_type == "function").count();
                let classes = elements
                    .iter()
                    .filter(|e| matches!(e.element_type.as_str(), "class" | "struct" | "enum"))
                    .count();
                let tests = elements.iter().filter(|e| e.is_test).count();
                let avg_complexity: f64 = if !elements.is_empty() {
                    elements.iter().map(|e| e.complexity_score as f64).sum::<f64>()
                        / elements.len() as f64
                } else {
                    0.0
                };
                let complex_functions = elements.iter().filter(|e| e.complexity_score > 10).count();

                Ok(json!({
                    "success": true,
                    "project_id": project_id,
                    "stats": {
                        "total_elements": total_elements,
                        "functions": functions,
                        "classes": classes,
                        "tests": tests,
                        "test_coverage_ratio": if functions > 0 { tests as f64 / functions as f64 } else { 0.0 },
                        "avg_complexity": (avg_complexity * 100.0).round() / 100.0,
                        "complex_functions": complex_functions,
                        "complexity_ratio": if functions > 0 { complex_functions as f64 / functions as f64 } else { 0.0 }
                    },
                    "message": format!("Analyzed {} code elements", total_elements)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to get codebase stats: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "stats": {}
                }))
            }
        }
    }

    /// Find callers of a function (placeholder - needs call graph implementation)
    async fn find_callers(&self, args: Value) -> Result<Value> {
        let function_name = args
            .get("function_name")
            .and_then(|v| v.as_str())
            .context("Missing function_name parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50);

        info!("[CODE] Finding callers of: {}", function_name);

        // Use semantic search to find code mentioning the function
        match self
            .code_intelligence
            .search_code(function_name, project_id, limit)
            .await
        {
            Ok(results) => {
                let callers: Vec<Value> = results
                    .iter()
                    .filter(|entry| entry.content.contains(function_name))
                    .map(|entry| {
                        json!({
                            "content": entry.content,
                            "role": entry.role,
                            "tags": entry.tags.clone().unwrap_or_default(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "count": callers.len(),
                    "callers": callers,
                    "function_name": function_name,
                    "message": format!("Found {} potential caller(s) of '{}'", callers.len(), function_name)
                }))
            }
            Err(e) => {
                warn!("[CODE] Failed to find callers: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "callers": []
                }))
            }
        }
    }

    /// Get element definition
    async fn get_element_definition(&self, args: Value) -> Result<Value> {
        let element_name = args
            .get("element_name")
            .and_then(|v| v.as_str())
            .context("Missing element_name parameter")?;
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .context("Missing project_id parameter")?;
        let file_path = args.get("file_path").and_then(|v| v.as_str());

        info!("[CODE] Getting definition of: {}", element_name);

        let search_pattern = if let Some(path) = file_path {
            format!("{}::{}", path, element_name)
        } else {
            element_name.to_string()
        };

        match self
            .code_intelligence
            .search_elements_for_project(&search_pattern, project_id, Some(10))
            .await
        {
            Ok(elements) => {
                // Find exact match
                let definition = elements.iter().find(|e| e.name == element_name);

                if let Some(def) = definition {
                    Ok(json!({
                        "success": true,
                        "element": {
                            "name": def.name,
                            "type": def.element_type,
                            "file_path": def.full_path.split("::").next().unwrap_or(""),
                            "full_path": def.full_path,
                            "visibility": def.visibility,
                            "start_line": def.start_line,
                            "end_line": def.end_line,
                            "complexity": def.complexity_score,
                            "is_async": def.is_async,
                            "is_test": def.is_test,
                            "documentation": def.documentation.clone().unwrap_or_default(),
                            "content": def.content,
                            "signature_hash": def.signature_hash
                        },
                        "message": format!("Found definition of '{}'", element_name)
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": format!("Element '{}' not found", element_name),
                        "element": null
                    }))
                }
            }
            Err(e) => {
                warn!("[CODE] Failed to get element definition: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string(),
                    "element": null
                }))
            }
        }
    }
}
