// src/llm/router.rs
// Smart LLM router for task-based provider selection
// DeepSeek 3.2 for code, GPT-5 for reasoning/chat

use crate::llm::provider::{LlmProvider, Message, Response, ToolResponse, ToolContext};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::gpt5::Gpt5Provider;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, debug};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    Code,  // DeepSeek: Fast, cheap code generation
    Chat,  // GPT-5: Complex reasoning, multi-turn
}

pub struct LlmRouter {
    deepseek: Arc<DeepSeekProvider>,
    gpt5: Arc<Gpt5Provider>,
}

impl LlmRouter {
    pub fn new(deepseek: Arc<DeepSeekProvider>, gpt5: Arc<Gpt5Provider>) -> Self {
        Self { deepseek, gpt5 }
    }
    
    /// Route to appropriate provider based on task type
    pub fn route(&self, task_type: TaskType) -> Arc<dyn LlmProvider> {
        match task_type {
            TaskType::Code => {
                debug!("Routing to DeepSeek for code task");
                self.deepseek.clone() as Arc<dyn LlmProvider>
            }
            TaskType::Chat => {
                debug!("Routing to GPT-5 for chat task");
                self.gpt5.clone() as Arc<dyn LlmProvider>
            }
        }
    }
    
    /// Get provider name for logging
    pub fn provider_name(&self, task_type: TaskType) -> &'static str {
        match task_type {
            TaskType::Code => "DeepSeek 3.2",
            TaskType::Chat => "GPT-5",
        }
    }
    
    /// Infer task type from user message
    pub fn infer_task_type(message: &str) -> TaskType {
        let lower = message.to_lowercase();
        
        // Code indicators (strong signals)
        let code_keywords = [
            "error[", "error:", "warning:",  // Compiler errors
            "fix", "refactor", "implement", "function", "method",
            "class", "struct", "enum", "trait", "impl",
            "bug", "compile", "syntax", "type error",
            "import", "export", "async", "await", "return",
            "fn ", "let ", "const ", "var ", "def ", "func ",
            "cargo", "npm", "pip", "go build",
            "undefined reference", "cannot find", "expected", "found",
            "stack trace", "panic", "segfault",
        ];
        
        // Chat indicators (reasoning/explanation)
        let chat_keywords = [
            "explain", "what is", "why does", "how does", "when should",
            "tell me about", "describe", "discuss", "analyze",
            "compare", "evaluate", "consider", "think about",
            "what do you think", "your opinion", "advice",
            "help me understand", "walk me through",
        ];
        
        // Count matches for each category
        let code_score = code_keywords.iter()
            .filter(|kw| lower.contains(*kw))
            .count();
        
        let chat_score = chat_keywords.iter()
            .filter(|kw| lower.contains(*kw))
            .count();
        
        // Decision logic
        if code_score > chat_score {
            debug!("Detected Code task (score: {} vs {})", code_score, chat_score);
            TaskType::Code
        } else if chat_score > code_score {
            debug!("Detected Chat task (score: {} vs {})", chat_score, code_score);
            TaskType::Chat
        } else {
            // Tie or no matches: check for code blocks or explicit patterns
            if lower.contains("```") || lower.contains("error[") || lower.contains("fix this") {
                debug!("Detected Code task (code block/error pattern)");
                TaskType::Code
            } else {
                // Default to Code (DeepSeek is faster and cheaper)
                debug!("Defaulting to Code task (ambiguous)");
                TaskType::Code
            }
        }
    }
    
    /// Call provider with automatic routing
    pub async fn chat(
        &self,
        task_type: TaskType,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Response> {
        let provider = self.route(task_type);
        provider.chat(messages, system).await
    }
    
    /// Call provider with tools and automatic routing
    pub async fn chat_with_tools(
        &self,
        task_type: TaskType,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        let provider = self.route(task_type);
        provider.chat_with_tools(messages, system, tools, context).await
    }
    
    /// Multi-turn tool calling with automatic provider selection
    pub async fn call_with_tools(
        &self,
        task_type: TaskType,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        max_iterations: usize,
    ) -> Result<ToolResponse> {
        let provider = self.route(task_type);
        
        let mut current_messages = messages;
        let mut iteration = 0;
        let mut context: Option<ToolContext> = None;
        
        loop {
            iteration += 1;
            
            if iteration > max_iterations {
                info!("Max iterations ({}) reached, returning final response", max_iterations);
                break;
            }
            
            // Call provider with tools
            let response = provider.chat_with_tools(
                current_messages.clone(),
                system.clone(),
                tools.clone(),
                context.clone(),
            ).await?;
            
            // If no function calls, we're done
            if response.function_calls.is_empty() {
                debug!("No function calls in response, iteration complete");
                return Ok(response);
            }
            
            debug!("Processing {} function calls", response.function_calls.len());
            
            // For GPT-5, set previous_response_id for next call
            if matches!(task_type, TaskType::Chat) {
                context = Some(ToolContext::Gpt5 {
                    previous_response_id: response.id.clone(),
                });
            }
            
            // Add assistant response to messages
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: response.text_output.clone(),
            });
            
            // In a real implementation, you'd execute the tool calls here
            // and add their results to messages. For now, return the response.
            return Ok(response);
        }
        
        // Should not reach here, but return empty response if we do
        Err(anyhow::anyhow!("Tool calling loop completed without result"))
    }
}
