// src/memory/features/code_intelligence/parser.rs
use anyhow::Result;
use syn::{ItemFn, ItemStruct, ItemEnum, ItemImpl, Visibility, visit::{self, Visit}, Attribute};
use crate::memory::features::code_intelligence::types::*;
use sha2::{Sha256, Digest};

#[derive(Clone)]
pub struct RustParser {
    max_complexity: i64,
}

impl RustParser {
    pub fn new() -> Self {
        Self { max_complexity: 10 }
    }

    pub fn with_max_complexity(max_complexity: i64) -> Self {
        Self { max_complexity }
    }
}

impl LanguageParser for RustParser {
    async fn parse_file(&self, content: &str, file_path: &str) -> Result<FileAnalysis> {
        let syntax_tree = syn::parse_file(content)?;
        let mut analyzer = RustAnalyzer::new(self.max_complexity, file_path);
        analyzer.visit_file(&syntax_tree);

        let (elements, dependencies, quality_issues, total_complexity, test_count) = analyzer.finalize();
        let doc_coverage = Self::calculate_doc_coverage(&elements);

        Ok(FileAnalysis {
            elements,
            dependencies,
            quality_issues,
            complexity_score: total_complexity,
            test_count,
            doc_coverage,
            // REMOVED: websocket_calls (Phase 1 - WebSocket tracking deleted)
        })
    }

    fn can_parse(&self, _content: &str, file_path: Option<&str>) -> bool {
        file_path.map_or(false, |path| path.ends_with(".rs"))
    }

    fn language(&self) -> &'static str {
        "rust"
    }
}

impl RustParser {
    fn calculate_doc_coverage(elements: &[CodeElement]) -> f64 {
        if elements.is_empty() {
            return 1.0;
        }
        let documented = elements.iter()
            .filter(|e| e.documentation.is_some())
            .count();
        documented as f64 / elements.len() as f64
    }
}

struct RustAnalyzer<'content> {
    max_complexity: i64,
    elements: Vec<CodeElement>,
    dependencies: Vec<ExternalDependency>,
    quality_issues: Vec<QualityIssue>,
    total_complexity: i64,
    test_count: i64,
    current_module_path: Vec<String>,
    file_path: &'content str,
}

impl<'content> RustAnalyzer<'content> {
    fn new(max_complexity: i64, file_path: &'content str) -> Self {
        Self {
            max_complexity,
            elements: Vec::new(),
            dependencies: Vec::new(),
            quality_issues: Vec::new(),
            total_complexity: 0,
            test_count: 0,
            current_module_path: Vec::new(),
            file_path,
        }
    }

    fn finalize(self) -> (Vec<CodeElement>, Vec<ExternalDependency>, Vec<QualityIssue>, i64, i64) {
        (self.elements, self.dependencies, self.quality_issues, self.total_complexity, self.test_count)
    }

    fn get_visibility_string(&self, vis: &Visibility) -> String {
        match vis {
            Visibility::Public(_) => "public".to_string(),
            Visibility::Restricted(_) => "restricted".to_string(),
            Visibility::Inherited => "private".to_string(),
        }
    }

    fn extract_documentation(&self, attrs: &[Attribute]) -> Option<String> {
        let mut docs = Vec::new();
        
        for attr in attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let syn::Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            docs.push(lit_str.value().trim().to_string());
                        }
                    }
                }
            }
        }
        
        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }

    fn create_signature_hash(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        Digest::update(&mut hasher, content.as_bytes());
        format!("{:x}", hasher.finalize())[..16].to_string()
    }

    fn extract_line_numbers<T: syn::spanned::Spanned>(&self, item: &T) -> (i64, i64) {
        let span = item.span();
        let start = span.start();
        let end = span.end();
        
        (start.line as i64, end.line as i64)
    }

    fn build_full_path(&self, element_name: &str) -> String {
        let module_path = if self.current_module_path.is_empty() {
            element_name.to_string()
        } else {
            format!("{}::{}", self.current_module_path.join("::"), element_name)
        };

        let clean_file_path = self.file_path.replace("\\", "/");
        format!("{}::{}", clean_file_path, module_path)
    }

    fn calculate_function_complexity(&self, block: &syn::Block) -> i64 {
        let complexity = 1;
        
        struct ComplexityVisitor {
            complexity: i64,
        }
        
        impl<'ast> Visit<'ast> for ComplexityVisitor {
            fn visit_expr(&mut self, expr: &'ast syn::Expr) {
                match expr {
                    syn::Expr::If(_) | syn::Expr::Match(_) | syn::Expr::While(_) | 
                    syn::Expr::ForLoop(_) | syn::Expr::Loop(_) => {
                        self.complexity += 1;
                    }
                    syn::Expr::Try(_) => {
                        self.complexity += 1;
                    }
                    _ => {}
                }
                visit::visit_expr(self, expr);
            }
        }
        
        let mut visitor = ComplexityVisitor { complexity };
        visitor.visit_block(block);
        visitor.complexity
    }
}

impl<'ast, 'content> Visit<'ast> for RustAnalyzer<'content> {
    fn visit_item_fn(&mut self, func: &'ast ItemFn) {
        let visibility = self.get_visibility_string(&func.vis);
        let documentation = self.extract_documentation(&func.attrs);
        let complexity = self.calculate_function_complexity(&func.block);
        self.total_complexity += complexity;

        let is_test = func.attrs.iter().any(|attr| {
            attr.path().is_ident("test") || 
            attr.path().segments.iter().any(|seg| seg.ident == "test")
        });
        
        if is_test {
            self.test_count += 1;
        }

        let is_async = func.sig.asyncness.is_some();
        let content = quote::quote!(#func).to_string();
        
        let full_path = self.build_full_path(&func.sig.ident.to_string());
        let (start_line, end_line) = self.extract_line_numbers(func);

        if complexity > self.max_complexity {
            self.quality_issues.push(QualityIssue {
                issue_type: "complexity".to_string(),
                severity: if complexity > self.max_complexity * 2 { "high".to_string() } else { "medium".to_string() },
                title: format!("High cyclomatic complexity ({})", complexity),
                description: format!(
                    "Function '{}' has complexity {} (threshold: {})",
                    func.sig.ident, complexity, self.max_complexity
                ),
                suggested_fix: Some("Consider breaking this function into smaller parts".to_string()),
                fix_confidence: 0.7,
                is_auto_fixable: false,
            });
        }

        if visibility == "public" && documentation.is_none() {
            self.quality_issues.push(QualityIssue {
                issue_type: "documentation".to_string(),
                severity: "low".to_string(),
                title: "Missing documentation".to_string(),
                description: format!("Public function '{}' lacks documentation", func.sig.ident),
                suggested_fix: Some("Add documentation comment using ///".to_string()),
                fix_confidence: 0.9,
                is_auto_fixable: false,
            });
        }

        self.elements.push(CodeElement {
            element_type: "function".to_string(),
            name: func.sig.ident.to_string(),
            full_path,
            visibility,
            start_line,
            end_line,
            content,
            signature_hash: self.create_signature_hash(&quote::quote!(#func.sig).to_string()),
            complexity_score: complexity,
            is_test,
            is_async,
            documentation,
            metadata: None,
        });

        visit::visit_item_fn(self, func);
    }

    fn visit_item_struct(&mut self, struct_item: &'ast ItemStruct) {
        let visibility = self.get_visibility_string(&struct_item.vis);
        let documentation = self.extract_documentation(&struct_item.attrs);
        let content = quote::quote!(#struct_item).to_string();
        let full_path = self.build_full_path(&struct_item.ident.to_string());
        let (start_line, end_line) = self.extract_line_numbers(struct_item);

        self.elements.push(CodeElement {
            element_type: "struct".to_string(),
            name: struct_item.ident.to_string(),
            full_path,
            visibility,
            start_line,
            end_line,
            content,
            signature_hash: self.create_signature_hash(&quote::quote!(#struct_item).to_string()),
            complexity_score: 0,
            is_test: false,
            is_async: false,
            documentation,
            metadata: None,
        });

        visit::visit_item_struct(self, struct_item);
    }

    fn visit_item_enum(&mut self, enum_item: &'ast ItemEnum) {
        let visibility = self.get_visibility_string(&enum_item.vis);
        let documentation = self.extract_documentation(&enum_item.attrs);
        let content = quote::quote!(#enum_item).to_string();
        let full_path = self.build_full_path(&enum_item.ident.to_string());
        let (start_line, end_line) = self.extract_line_numbers(enum_item);

        self.elements.push(CodeElement {
            element_type: "enum".to_string(),
            name: enum_item.ident.to_string(),
            full_path,
            visibility,
            start_line,
            end_line,
            content,
            signature_hash: self.create_signature_hash(&quote::quote!(#enum_item).to_string()),
            complexity_score: 0,
            is_test: false,
            is_async: false,
            documentation,
            metadata: None,
        });

        visit::visit_item_enum(self, enum_item);
    }

    fn visit_item_impl(&mut self, impl_item: &'ast ItemImpl) {
        if let Some((_, trait_path, _)) = &impl_item.trait_ {
            let trait_name = quote::quote!(#trait_path).to_string();
            let type_name = quote::quote!(#impl_item.self_ty).to_string();
            
            self.dependencies.push(ExternalDependency {
                import_path: trait_name.clone(),
                imported_symbols: vec![trait_name.clone()],
                dependency_type: "trait_impl".to_string(),
            });

            let full_path = format!("{}::impl_{}", self.build_full_path(&type_name), trait_name);
            let (start_line, end_line) = self.extract_line_numbers(impl_item);
            let content = quote::quote!(#impl_item).to_string();

            self.elements.push(CodeElement {
                element_type: "impl".to_string(),
                name: format!("{} for {}", trait_name, type_name),
                full_path,
                visibility: "public".to_string(),
                start_line,
                end_line,
                content,
                signature_hash: self.create_signature_hash(&quote::quote!(#impl_item).to_string()),
                complexity_score: 0,
                is_test: false,
                is_async: false,
                documentation: None,
                metadata: None,
            });
        }

        visit::visit_item_impl(self, impl_item);
    }

    fn visit_item_use(&mut self, use_item: &'ast syn::ItemUse) {
        let import_path = quote::quote!(#use_item.tree).to_string();
        
        let mut imported_symbols = Vec::new();
        Self::extract_use_symbols(&use_item.tree, &mut imported_symbols);

        let dependency_type = if import_path.starts_with("crate") {
            "local_crate"
        } else if import_path.starts_with("super") || import_path.starts_with("self") {
            "local_module"
        } else if import_path.starts_with("std") {
            "std_lib"
        } else {
            "external_crate"
        };

        self.dependencies.push(ExternalDependency {
            import_path,
            imported_symbols,
            dependency_type: dependency_type.to_string(),
        });

        visit::visit_item_use(self, use_item);
    }
}

impl RustAnalyzer<'_> {
    fn extract_use_symbols(tree: &syn::UseTree, symbols: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                Self::extract_use_symbols(&path.tree, symbols);
            }
            syn::UseTree::Name(name) => {
                symbols.push(name.ident.to_string());
            }
            syn::UseTree::Rename(rename) => {
                symbols.push(rename.rename.to_string());
            }
            syn::UseTree::Glob(_) => {
                symbols.push("*".to_string());
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    Self::extract_use_symbols(item, symbols);
                }
            }
        }
    }
}
