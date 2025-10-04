// src/memory/features/code_intelligence/websocket_analyzer.rs
// Separate analyzer for WebSocket patterns - keeps parsers clean

use anyhow::Result;
use syn::{Expr, Stmt, Pat, visit::{self, Visit}, spanned::Spanned, Member};
use super::types::{WebSocketCall, WebSocketHandler, WebSocketResponse};
use quote::quote;

/// Analyzes Rust code for WebSocket message patterns
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
    
    /// Analyze a Rust file for WebSocket patterns
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
            if let Stmt::Expr(Expr::Match(match_expr), _) = stmt {
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
    
    fn extract_method_from_arm(&self, expr: &Expr) -> Option<String> {
        if let Expr::Block(block_expr) = expr {
            for stmt in &block_expr.block.stmts {
                if let Stmt::Expr(Expr::Match(inner_match), _) = stmt {
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
    
    fn scan_for_websocket_responses(&mut self, block: &syn::Block) {
        for stmt in &block.stmts {
            // Changed from Stmt::Semi to Stmt::Expr - Semi doesn't exist in current syn
            if let Stmt::Expr(Expr::MethodCall(method_call), _) = stmt {
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
    
    fn extract_websocket_response(&mut self, expr: &Expr, line: usize) {
        if let Expr::Struct(struct_expr) = expr {
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
            // Check if this field is named "data_type"
            if let Member::Named(ident) = &field.member {
                if ident == "data_type" {
                    // Extract the string literal value
                    if let Expr::Lit(expr_lit) = &field.expr {
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

/// Analyzes TypeScript/JavaScript for WebSocket send() calls
pub struct TypeScriptWebSocketAnalyzer;

impl TypeScriptWebSocketAnalyzer {
    pub fn analyze(_content: &str, _file_path: &str) -> Result<Vec<WebSocketCall>> {
        // TODO: Implement TypeScript/JavaScript WebSocket call detection
        // For now, return empty - this keeps TypeScript parser clean too
        Ok(Vec::new())
    }
}
