// src/memory/features/code_intelligence/javascript_parser.rs
use crate::memory::features::code_intelligence::types::*;
use anyhow::Result;
use sha2::{Digest, Sha256};
use swc_common::{FileName, SourceMap, Span, Spanned, sync::Lrc};
use swc_ecma_ast::{CallExpr, ClassDecl, Expr, FnDecl, ImportDecl, Pat, VarDecl};
use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Clone)]
pub struct JavaScriptParser {
    max_complexity: i64,
}

impl JavaScriptParser {
    pub fn new() -> Self {
        Self { max_complexity: 15 }
    }

    pub fn with_max_complexity(max_complexity: i64) -> Self {
        Self { max_complexity }
    }
}

impl LanguageParser for JavaScriptParser {
    async fn parse_file(&self, content: &str, file_path: &str) -> Result<FileAnalysis> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom(file_path.to_string())),
            content.to_string(),
        );

        // ES syntax - JSX works by default for .jsx files
        let syntax = Syntax::Es(Default::default());

        let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);

        let mut parser = Parser::new_from(lexer);
        let module = parser
            .parse_module()
            .map_err(|e| anyhow::anyhow!("JavaScript parse error in {}: {:?}", file_path, e))?;

        let mut analyzer = JavaScriptAnalyzer::new(self.max_complexity, content, file_path);
        module.visit_with(&mut analyzer);

        let doc_coverage = analyzer.calculate_doc_coverage();

        Ok(FileAnalysis {
            elements: analyzer.elements,
            dependencies: analyzer.dependencies,
            quality_issues: analyzer.quality_issues,
            complexity_score: analyzer.total_complexity,
            test_count: analyzer.test_count,
            doc_coverage,
            // REMOVED: websocket_calls (Phase 1 - WebSocket tracking deleted)
        })
    }

    fn can_parse(&self, _content: &str, file_path: Option<&str>) -> bool {
        file_path.map_or(false, |path| {
            path.ends_with(".js") || path.ends_with(".jsx") || path.ends_with(".mjs")
        })
    }

    fn language(&self) -> &'static str {
        "javascript"
    }
}

struct JavaScriptAnalyzer<'a> {
    max_complexity: i64,
    elements: Vec<CodeElement>,
    dependencies: Vec<ExternalDependency>,
    quality_issues: Vec<QualityIssue>,
    total_complexity: i64,
    test_count: i64,
    content: &'a str,
    file_path: &'a str,
    current_path: Vec<String>,
    // REMOVED: websocket_calls (Phase 1 - WebSocket tracking deleted)
    current_function: Option<String>,
}

impl<'a> JavaScriptAnalyzer<'a> {
    fn new(max_complexity: i64, content: &'a str, file_path: &'a str) -> Self {
        Self {
            max_complexity,
            elements: Vec::new(),
            dependencies: Vec::new(),
            quality_issues: Vec::new(),
            total_complexity: 0,
            test_count: 0,
            content,
            file_path,
            current_path: Vec::new(),
            // REMOVED: websocket_calls (Phase 1 - WebSocket tracking deleted)
            current_function: None,
        }
    }

    fn calculate_doc_coverage(&self) -> f64 {
        if self.elements.is_empty() {
            return 1.0;
        }
        let documented = self
            .elements
            .iter()
            .filter(|e| e.documentation.is_some())
            .count();
        documented as f64 / self.elements.len() as f64
    }

    fn get_full_path(&self, name: &str) -> String {
        let module_path = if self.current_path.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", self.current_path.join("."), name)
        };

        let clean_file_path = self.file_path.replace("\\", "/");
        format!("{}::{}", clean_file_path, module_path)
    }

    fn extract_text(&self, span: Span) -> String {
        let start = span.lo.0 as usize;
        let end = span.hi.0 as usize;
        if start < self.content.len() && end <= self.content.len() && start < end {
            self.content[start..end].to_string()
        } else {
            String::new()
        }
    }

    fn get_line_number(&self, span: Span) -> i64 {
        let pos = span.lo.0 as usize;
        self.content[..pos.min(self.content.len())]
            .chars()
            .filter(|&c| c == '\n')
            .count() as i64
            + 1
    }

    fn get_end_line_number(&self, span: Span) -> i64 {
        let pos = span.hi.0 as usize;
        self.content[..pos.min(self.content.len())]
            .chars()
            .filter(|&c| c == '\n')
            .count() as i64
            + 1
    }

    fn create_signature_hash(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        Digest::update(&mut hasher, content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn calculate_cyclomatic_complexity(&self, content: &str) -> i64 {
        let mut complexity = 1i64;

        for keyword in [
            "if", "else if", "for", "while", "case", "catch", "&&", "||", "?",
        ] {
            complexity += content.matches(keyword).count() as i64;
        }

        complexity
    }

    fn is_react_component(&self, name: &str) -> bool {
        name.chars().next().map_or(false, |c| c.is_uppercase())
    }

    fn is_react_hook(&self, name: &str) -> bool {
        name.starts_with("use") && name.len() > 3
    }

    fn is_test_function(&self, name: &str) -> bool {
        name.contains("test")
            || name.contains("Test")
            || name.contains("spec")
            || name.starts_with("it")
            || name.starts_with("describe")
    }

    fn detect_quality_issues(&mut self, element: &CodeElement) {
        if element.complexity_score > self.max_complexity {
            self.quality_issues.push(QualityIssue {
                issue_type: "complexity".to_string(),
                severity: "medium".to_string(),
                title: "High cyclomatic complexity".to_string(),
                description: format!(
                    "{}: Function '{}' has complexity {} (threshold: {})",
                    self.file_path, element.name, element.complexity_score, self.max_complexity
                ),
                suggested_fix: Some(
                    "Consider breaking this function into smaller parts".to_string(),
                ),
                fix_confidence: 0.7,
                is_auto_fixable: false,
            });
        }

        if element.documentation.is_none() && element.visibility == "public" {
            self.quality_issues.push(QualityIssue {
                issue_type: "documentation".to_string(),
                severity: "low".to_string(),
                title: "Missing documentation".to_string(),
                description: format!(
                    "{}: Exported {} '{}' lacks documentation",
                    self.file_path, element.element_type, element.name
                ),
                suggested_fix: Some("Add JSDoc comment describing the function/class".to_string()),
                fix_confidence: 0.9,
                is_auto_fixable: false,
            });
        }
    }

    fn extract_jsdoc(&self, span: Span) -> Option<String> {
        let start_line = self.get_line_number(span);
        if start_line == 0 {
            return None;
        }

        let lines: Vec<&str> = self.content.lines().collect();
        let mut docs = Vec::new();
        let mut found_jsdoc = false;

        for i in (0..start_line.saturating_sub(1) as usize).rev() {
            let line = lines.get(i)?;
            let trimmed = line.trim();

            if trimmed.starts_with("*/") {
                found_jsdoc = true;
                continue;
            }

            if found_jsdoc {
                if trimmed.starts_with("/**") {
                    docs.reverse();
                    return Some(docs.join("\n"));
                }
                if trimmed.starts_with("*") {
                    docs.push(trimmed.trim_start_matches('*').trim().to_string());
                }
            }

            if !trimmed.starts_with("//") && !trimmed.is_empty() && !found_jsdoc {
                break;
            }
        }

        None
    }

    // REMOVED: extract_object_field, extract_websocket_call (Phase 1 - WebSocket tracking deleted)
}

impl<'a> Visit for JavaScriptAnalyzer<'a> {
    fn visit_fn_decl(&mut self, node: &FnDecl) {
        let name = node.ident.sym.to_string();
        self.current_function = Some(name.clone());

        let full_path = self.get_full_path(&name);
        let content = self.extract_text(node.function.span);
        let complexity = self.calculate_cyclomatic_complexity(&content);
        let is_async = node.function.is_async;
        let is_test = self.is_test_function(&name);

        if is_test {
            self.test_count += 1;
        }

        let documentation = self.extract_jsdoc(node.function.span);

        let metadata = serde_json::json!({
            "is_react_component": self.is_react_component(&name),
            "is_react_hook": self.is_react_hook(&name),
            "is_generator": node.function.is_generator,
            "param_count": node.function.params.len(),
        });

        let element = CodeElement {
            element_type: "function".to_string(),
            name: name.clone(),
            full_path: full_path.clone(),
            visibility: "public".to_string(),
            start_line: self.get_line_number(node.function.span),
            end_line: self.get_end_line_number(node.function.span),
            content: content.clone(),
            signature_hash: self.create_signature_hash(&content),
            complexity_score: complexity,
            is_test,
            is_async,
            documentation,
            metadata: Some(metadata.to_string()),
        };

        self.total_complexity += complexity;
        self.detect_quality_issues(&element);
        self.elements.push(element);

        self.current_path.push(name);
        node.function.visit_children_with(self);
        self.current_path.pop();
        self.current_function = None;
    }

    fn visit_class_decl(&mut self, node: &ClassDecl) {
        let name = node.ident.sym.to_string();
        let full_path = self.get_full_path(&name);
        let content = self.extract_text(node.class.span);
        let documentation = self.extract_jsdoc(node.class.span);

        let metadata = serde_json::json!({
            "is_react_component": self.is_react_component(&name),
            "method_count": node.class.body.len(),
        });

        let element = CodeElement {
            element_type: "class".to_string(),
            name: name.clone(),
            full_path: full_path.clone(),
            visibility: "public".to_string(),
            start_line: self.get_line_number(node.class.span),
            end_line: self.get_end_line_number(node.class.span),
            content: content.clone(),
            signature_hash: self.create_signature_hash(&content),
            complexity_score: 0,
            is_test: false,
            is_async: false,
            documentation,
            metadata: Some(metadata.to_string()),
        };

        self.elements.push(element);

        self.current_path.push(name);
        node.class.visit_children_with(self);
        self.current_path.pop();
    }

    fn visit_import_decl(&mut self, node: &ImportDecl) {
        let import_path = node.src.value.to_string();
        let mut imported_symbols = Vec::new();

        for specifier in &node.specifiers {
            match specifier {
                swc_ecma_ast::ImportSpecifier::Named(named) => {
                    imported_symbols.push(named.local.sym.to_string());
                }
                swc_ecma_ast::ImportSpecifier::Default(default) => {
                    imported_symbols.push(default.local.sym.to_string());
                }
                swc_ecma_ast::ImportSpecifier::Namespace(ns) => {
                    imported_symbols.push(format!("* as {}", ns.local.sym));
                }
            }
        }

        let dependency_type = if import_path.starts_with('.') || import_path.starts_with('/') {
            "local_import"
        } else if import_path.starts_with('@') || import_path.contains('/') {
            "npm_package"
        } else {
            "npm_package"
        };

        self.dependencies.push(ExternalDependency {
            import_path,
            imported_symbols,
            dependency_type: dependency_type.to_string(),
        });
    }

    fn visit_var_decl(&mut self, node: &VarDecl) {
        for decl in &node.decls {
            if let Pat::Ident(ident) = &decl.name {
                let name = ident.sym.to_string();

                if let Some(init) = &decl.init {
                    let is_function = match &**init {
                        Expr::Fn(_) | Expr::Arrow(_) => true,
                        _ => false,
                    };

                    if is_function {
                        self.current_function = Some(name.clone());

                        let content = self.extract_text(init.span());
                        let complexity = self.calculate_cyclomatic_complexity(&content);
                        let is_test = self.is_test_function(&name);

                        if is_test {
                            self.test_count += 1;
                        }

                        let is_async = match &**init {
                            Expr::Fn(f) => f.function.is_async,
                            Expr::Arrow(a) => a.is_async,
                            _ => false,
                        };

                        let documentation = self.extract_jsdoc(init.span());

                        let metadata = serde_json::json!({
                            "is_react_component": self.is_react_component(&name),
                            "is_react_hook": self.is_react_hook(&name),
                            "is_arrow_function": matches!(&**init, Expr::Arrow(_)),
                        });

                        let element = CodeElement {
                            element_type: "function".to_string(),
                            name: name.clone(),
                            full_path: self.get_full_path(&name),
                            visibility: "public".to_string(),
                            start_line: self.get_line_number(init.span()),
                            end_line: self.get_end_line_number(init.span()),
                            content: content.clone(),
                            signature_hash: self.create_signature_hash(&content),
                            complexity_score: complexity,
                            is_test,
                            is_async,
                            documentation,
                            metadata: Some(metadata.to_string()),
                        };

                        self.total_complexity += complexity;
                        self.detect_quality_issues(&element);
                        self.elements.push(element);

                        self.current_function = None;
                    }
                }
            }
        }
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        // REMOVED: WebSocket call detection (Phase 1 - WebSocket tracking deleted)
        call.visit_children_with(self);
    }
}
