// src/memory/features/code_intelligence/websocket_analyzer.rs

use anyhow::Result;
use syn::{Expr as SynExpr, Stmt, Pat, visit::{self, Visit}, spanned::Spanned, Member};
use super::types::{WebSocketCall, WebSocketHandler, WebSocketResponse};
use quote::quote;

use swc_common::{sync::Lrc, SourceMap, FileName};
use swc_ecma_ast::*;
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_visit::{Visit as SwcVisit, VisitWith};

pub struct WebSocketAnalyzer {
    handlers: Vec<WebSocketHandler>,
    responses: Vec<WebSocketResponse>,
    current_function: String,
}

impl WebSocketAnalyzer {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            responses: Vec::new(),
            current_function: String::new(),
        }
    }
    
    pub fn analyze_rust_file(content: &str) -> Result<WebSocketAnalysis> {
        let syntax_tree = syn::parse_file(content)?;
        let mut analyzer = Self::new();
        analyzer.visit_file(&syntax_tree);
        
        Ok(WebSocketAnalysis {
            handlers: analyzer.handlers,
            responses: analyzer.responses,
        })
    }
    
    fn scan_for_websocket_handlers(&mut self, block: &syn::Block) {
        for stmt in &block.stmts {
            if let Stmt::Expr(SynExpr::Match(match_expr), _) = stmt {
                self.extract_websocket_match(match_expr);
            }
        }
    }
    
    fn extract_websocket_match(&mut self, match_expr: &syn::ExprMatch) {
        for arm in &match_expr.arms {
            if let Pat::TupleStruct(pat_tuple) = &arm.pat {
                let path_str = quote!(#pat_tuple.path).to_string();
                
                if path_str.contains("WsClientMessage::") {
                    let message_type = path_str
                        .split("::")
                        .last()
                        .unwrap_or("")
                        .to_string();
                    
                    let method = self.extract_method_from_arm(&arm.body);
                    
                    self.handlers.push(WebSocketHandler {
                        message_type,
                        method,
                        handler_function: self.current_function.clone(),
                        line_number: arm.span().start().line,
                    });
                }
            }
        }
    }
    
    fn extract_method_from_arm(&self, expr: &SynExpr) -> Option<String> {
        if let SynExpr::Block(block_expr) = expr {
            for stmt in &block_expr.block.stmts {
                if let Stmt::Expr(SynExpr::Match(inner_match), _) = stmt {
                    for inner_arm in &inner_match.arms {
                        if let Pat::Lit(expr_lit) = &inner_arm.pat {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                return Some(lit_str.value());
                            }
                        }
                    }
                }
            }
        }
        None
    }
    
    fn scan_for_method_handlers(&mut self, func: &syn::ItemFn) {
        let has_method_param = func.sig.inputs.iter().any(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(ident) = &*pat_type.pat {
                    return ident.ident == "method";
                }
            }
            false
        });

        if !has_method_param {
            return;
        }

        self.scan_block_for_method_match(&func.block);
    }

    fn scan_block_for_method_match(&mut self, block: &syn::Block) {
        for stmt in &block.stmts {
            if let Stmt::Expr(SynExpr::Match(match_expr), _) = stmt {
                if let SynExpr::Path(path) = &*match_expr.expr {
                    if path.path.segments.last().map(|s| s.ident == "method").unwrap_or(false) {
                        for arm in &match_expr.arms {
                            if let Pat::Lit(expr_lit) = &arm.pat {
                                if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                    let method_str = lit_str.value();
                                    
                                    let message_type = if method_str.starts_with("git.") {
                                        "GitCommand"
                                    } else if method_str.starts_with("project.") {
                                        "ProjectCommand"
                                    } else if method_str.starts_with("memory.") {
                                        "MemoryCommand"
                                    } else if method_str.starts_with("file.") {
                                        "FileSystemCommand"
                                    } else if method_str.starts_with("code.") || method_str.starts_with("dependencies.") {
                                        "CodeIntelligenceCommand"
                                    } else if method_str.starts_with("document.") {
                                        "DocumentCommand"
                                    } else {
                                        "Unknown"
                                    };

                                    self.handlers.push(WebSocketHandler {
                                        message_type: message_type.to_string(),
                                        method: Some(method_str),
                                        handler_function: self.current_function.clone(),
                                        line_number: arm.span().start().line,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    fn scan_for_websocket_responses(&mut self, block: &syn::Block) {
        for stmt in &block.stmts {
            if let Stmt::Expr(SynExpr::MethodCall(method_call), _) = stmt {
                if let syn::Expr::Path(_) = &*method_call.receiver {
                    let method_name = method_call.method.to_string();
                    
                    if method_name == "send_message" || method_name == "send" {
                        if let Some(arg) = method_call.args.first() {
                            self.extract_websocket_response(arg, method_call.span().start().line);
                        }
                    }
                }
            }
        }
    }
    
    fn extract_websocket_response(&mut self, expr: &SynExpr, line: usize) {
        if let SynExpr::Struct(struct_expr) = expr {
            let path_str = quote!(#struct_expr.path).to_string();
            
            if path_str.contains("WsServerMessage::") {
                let response_type = path_str
                    .split("::")
                    .last()
                    .unwrap_or("")
                    .to_string();
                
                let data_type = if response_type == "Data" {
                    self.extract_data_type_field(&struct_expr.fields)
                } else {
                    None
                };
                
                self.responses.push(WebSocketResponse {
                    response_type,
                    data_type,
                    sending_function: self.current_function.clone(),
                    line_number: line,
                });
            }
        }
    }
    
    fn extract_data_type_field(&self, fields: &syn::punctuated::Punctuated<syn::FieldValue, syn::token::Comma>) -> Option<String> {
        for field in fields {
            if let Member::Named(ident) = &field.member {
                if ident == "data_type" {
                    if let SynExpr::Lit(expr_lit) = &field.expr {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            return Some(lit_str.value());
                        }
                    }
                }
            }
        }
        None
    }
}

impl<'ast> Visit<'ast> for WebSocketAnalyzer {
    fn visit_item_fn(&mut self, func: &'ast syn::ItemFn) {
        self.current_function = func.sig.ident.to_string();
        
        self.scan_for_websocket_handlers(&func.block);
        self.scan_for_method_handlers(func);
        self.scan_for_websocket_responses(&func.block);
        self.current_function.clear();
        
        visit::visit_item_fn(self, func);
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketAnalysis {
    pub handlers: Vec<WebSocketHandler>,
    pub responses: Vec<WebSocketResponse>,
}

pub struct TypeScriptWebSocketAnalyzer {
    calls: Vec<WebSocketCall>,
    current_function: Option<String>,
}

impl TypeScriptWebSocketAnalyzer {
    pub fn analyze(content: &str, file_path: &str) -> Result<Vec<WebSocketCall>> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Custom(file_path.to_string())),
            content.to_string(),
        );

        let lexer = Lexer::new(
            Syntax::Typescript(TsSyntax {
                tsx: true,
                decorators: true,
                ..Default::default()
            }),
            EsVersion::latest(),
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);
        let module = parser.parse_module()
            .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;

        let mut analyzer = Self {
            calls: Vec::new(),
            current_function: None,
        };

        module.visit_with(&mut analyzer);
        Ok(analyzer.calls)
    }

    fn extract_message_type(&self, obj: &ObjectLit) -> Option<String> {
        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(kv) = &**prop {
                    if let PropName::Ident(ident) = &kv.key {
                        if ident.sym.to_string() == "type" {
                            if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(str_lit)) = &*kv.value {
                                return Some(str_lit.value.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn extract_method(&self, obj: &ObjectLit) -> Option<String> {
        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(kv) = &**prop {
                    if let PropName::Ident(ident) = &kv.key {
                        if ident.sym.to_string() == "method" {
                            if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(str_lit)) = &*kv.value {
                                return Some(str_lit.value.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

impl SwcVisit for TypeScriptWebSocketAnalyzer {
    fn visit_function(&mut self, func: &Function) {
        let prev_function = self.current_function.clone();
        self.current_function = None;
        
        func.visit_children_with(self);
        self.current_function = prev_function;
    }

    fn visit_fn_decl(&mut self, func: &FnDecl) {
        let prev_function = self.current_function.clone();
        self.current_function = Some(func.ident.sym.to_string());
        
        func.visit_children_with(self);
        self.current_function = prev_function;
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        let is_send_call = match &call.callee {
            Callee::Expr(expr) => match &**expr {
                swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string() == "send",
                swc_ecma_ast::Expr::Member(member) => {
                    if let MemberProp::Ident(ident) = &member.prop {
                        ident.sym.to_string() == "send"
                    } else {
                        false
                    }
                }
                _ => false,
            },
            _ => false,
        };

        if is_send_call && !call.args.is_empty() {
            let ExprOrSpread { expr, .. } = &call.args[0];
            if let swc_ecma_ast::Expr::Object(obj) = &**expr {
                if let Some(message_type) = self.extract_message_type(obj) {
                    let method = self.extract_method(obj);
                    
                    self.calls.push(WebSocketCall {
                        message_type,
                        method,
                        line_number: call.span.lo.0 as usize,
                        element: self.current_function.clone().unwrap_or_else(|| "anonymous".to_string()),
                    });
                }
            }
        }

        call.visit_children_with(self);
    }
}
