// src/memory/features/code_intelligence/parser.rs
// Rust parser using syn crate for real AST analysis

use anyhow::{Result, Context};
use syn::{ItemFn, ItemStruct, ItemEnum, ItemUse, ItemMod, Visibility, visit::{self, Visit}};
use crate::memory::features::code_intelligence::types::*;
use sha2::{Sha256, Digest};

/// Rust AST parser using syn crate
pub struct RustParser {
    max_complexity: u32,
}

impl RustParser {
    pub fn new() -> Self {
        Self {
            max_complexity: 10,
        }
    }

    pub fn with_max_complexity(max_complexity: u32) -> Self {
        Self {
            max_complexity,
        }
    }
}

impl LanguageParser for RustParser {
    async fn parse_file(&self, content: &str, file_path: &str) -> Result<FileAnalysis> {
        let syntax_tree = syn::parse_file(content)
            .with_context(|| format!("Failed to parse Rust file: {}", file_path))?;

        let mut analyzer = RustAnalyzer::new(self.max_complexity);
        analyzer.visit_file(&syntax_tree);

        // Fix the borrow issue by cloning all Vec fields
        Ok(FileAnalysis {
            elements: analyzer.elements.clone(), // Clone this too
            dependencies: analyzer.dependencies.clone(), 
            quality_issues: analyzer.quality_issues.clone(),
            complexity_score: analyzer.total_complexity,
            test_count: analyzer.test_count,
            doc_coverage: analyzer.calculate_doc_coverage(),
        })
    }

    fn can_parse(&self, _content: &str, file_path: Option<&str>) -> bool {
        file_path.map_or(false, |path| path.ends_with(".rs"))
    }

    fn language(&self) -> &'static str {
        "rust"
    }
}

/// AST visitor that extracts elements during traversal
struct RustAnalyzer {
    max_complexity: u32,
    elements: Vec<CodeElement>,
    dependencies: Vec<ExternalDependency>,
    quality_issues: Vec<QualityIssue>,
    total_complexity: u32,
    test_count: u32,
    current_module_path: Vec<String>,
}

impl RustAnalyzer {
    fn new(max_complexity: u32) -> Self {
        Self {
            max_complexity,
            elements: Vec::new(),
            dependencies: Vec::new(),
            quality_issues: Vec::new(),
            total_complexity: 0,
            test_count: 0,
            current_module_path: Vec::new(),
        }
    }

    fn calculate_doc_coverage(&self) -> f64 {
        if self.elements.is_empty() {
            return 1.0;
        }
        let documented = self.elements.iter()
            .filter(|e| e.documentation.is_some())
            .count();
        documented as f64 / self.elements.len() as f64
    }

    fn get_visibility_string(&self, vis: &Visibility) -> String {
        match vis {
            Visibility::Public(_) => "public".to_string(),
            Visibility::Restricted(_) => "restricted".to_string(),
            Visibility::Inherited => "private".to_string(),
        }
    }

    fn extract_documentation(&self, attrs: &[syn::Attribute]) -> Option<String> {
        let mut docs = Vec::new();
        for attr in attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(doc_str), .. }) = &meta.value {
                        docs.push(doc_str.value());
                    }
                }
            }
        }
        if docs.is_empty() { None } else { Some(docs.join("\n")) }
    }

    fn create_signature_hash(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())[..16].to_string()
    }

    fn calculate_function_complexity(&self, block: &syn::Block) -> u32 {
        let complexity = 1; // Base complexity
        
        struct ComplexityVisitor {
            complexity: u32,
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

impl<'ast> Visit<'ast> for RustAnalyzer {
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
        
        let full_path = if self.current_module_path.is_empty() {
            func.sig.ident.to_string()
        } else {
            format!("{}::{}", self.current_module_path.join("::"), func.sig.ident)
        };

        // Generate quality issue if complexity is too high
        if complexity > self.max_complexity {
            self.quality_issues.push(QualityIssue {
                issue_type: "complexity".to_string(),
                severity: if complexity > self.max_complexity * 2 { "high".to_string() } else { "medium".to_string() },
                title: format!("High complexity in function '{}'", func.sig.ident),
                description: format!("Cyclomatic complexity of {} exceeds recommended limit of {}", 
                                   complexity, self.max_complexity),
                suggested_fix: Some("Consider breaking this function into smaller functions".to_string()),
                fix_confidence: 0.7,
                is_auto_fixable: false,
            });
        }

        self.elements.push(CodeElement {
            element_type: "function".to_string(),
            name: func.sig.ident.to_string(),
            full_path,
            visibility,
            start_line: 0, // NOTE: Line numbers from spans require proc-macro2 features we don't need yet
            end_line: 0,   // Will be enhanced in Phase 2 if needed
            content: content.clone(),
            signature_hash: self.create_signature_hash(&content),
            complexity_score: complexity,
            is_test,
            is_async,
            documentation,
            metadata: None, // Will be enhanced with generics info in Phase 2
        });

        visit::visit_item_fn(self, func);
    }

    fn visit_item_struct(&mut self, struct_item: &'ast ItemStruct) {
        let visibility = self.get_visibility_string(&struct_item.vis);
        let documentation = self.extract_documentation(&struct_item.attrs);
        let content = quote::quote!(#struct_item).to_string();

        let full_path = if self.current_module_path.is_empty() {
            struct_item.ident.to_string()
        } else {
            format!("{}::{}", self.current_module_path.join("::"), struct_item.ident)
        };

        self.elements.push(CodeElement {
            element_type: "struct".to_string(),
            name: struct_item.ident.to_string(),
            full_path,
            visibility,
            start_line: 0,
            end_line: 0,
            content: content.clone(),
            signature_hash: self.create_signature_hash(&content),
            complexity_score: struct_item.fields.len() as u32, // Field count as complexity metric
            is_test: false,
            is_async: false,
            documentation,
            metadata: Some(format!("{{\"field_count\": {}}}", struct_item.fields.len())),
        });

        visit::visit_item_struct(self, struct_item);
    }

    fn visit_item_enum(&mut self, enum_item: &'ast ItemEnum) {
        let visibility = self.get_visibility_string(&enum_item.vis);
        let documentation = self.extract_documentation(&enum_item.attrs);
        let content = quote::quote!(#enum_item).to_string();

        let full_path = if self.current_module_path.is_empty() {
            enum_item.ident.to_string()
        } else {
            format!("{}::{}", self.current_module_path.join("::"), enum_item.ident)
        };

        self.elements.push(CodeElement {
            element_type: "enum".to_string(),
            name: enum_item.ident.to_string(),
            full_path,
            visibility,
            start_line: 0,
            end_line: 0,
            content: content.clone(),
            signature_hash: self.create_signature_hash(&content),
            complexity_score: enum_item.variants.len() as u32,
            is_test: false,
            is_async: false,
            documentation,
            metadata: Some(format!("{{\"variant_count\": {}}}", enum_item.variants.len())),
        });

        visit::visit_item_enum(self, enum_item);
    }

    fn visit_item_use(&mut self, use_item: &'ast ItemUse) {
        let path = quote::quote!(#use_item.tree).to_string();
        
        // Parse the use statement to extract import info
        let import_path = path.replace(" ", "");
        let symbols = if import_path.contains("{") && import_path.contains("}") {
            // Handle: use std::collections::{HashMap, BTreeMap};
            let start = import_path.find('{').unwrap() + 1;
            let end = import_path.find('}').unwrap();
            let symbols_str = &import_path[start..end];
            symbols_str.split(',').map(|s| s.trim().to_string()).collect()
        } else {
            // Handle: use std::collections::HashMap;
            vec![import_path.split("::").last().unwrap_or("").to_string()]
        };

        self.dependencies.push(ExternalDependency {
            import_path,
            imported_symbols: symbols,
            dependency_type: "crate".to_string(),
        });

        visit::visit_item_use(self, use_item);
    }

    fn visit_item_mod(&mut self, mod_item: &'ast ItemMod) {
        self.current_module_path.push(mod_item.ident.to_string());
        
        if let Some((_, items)) = &mod_item.content {
            for item in items {
                visit::visit_item(self, item);
            }
        }
        
        self.current_module_path.pop();
    }
}
