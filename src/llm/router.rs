// src/llm/router.rs
// Smart LLM router with embedding-based task classification
// DeepSeek 3.2 for code, GPT-5 for reasoning/chat

use crate::llm::provider::{LlmProvider, Message, Response, ToolResponse, ToolContext, OpenAiEmbeddings};
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
    embedding_client: Arc<OpenAiEmbeddings>,
}

impl LlmRouter {
    pub fn new(
        deepseek: Arc<DeepSeekProvider>, 
        gpt5: Arc<Gpt5Provider>,
        embedding_client: Arc<OpenAiEmbeddings>,
    ) -> Self {
        Self { deepseek, gpt5, embedding_client }
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
    
    /// Smart task type inference using embeddings + keywords
    pub async fn infer_task_type(&self, message: &str, has_project: bool) -> Result<TaskType> {
        // Step 1: Try embedding-based classification
        match self.classify_with_embeddings(message).await {
            Ok(task_type) => {
                debug!("🧠 Embedding-based classification: {:?}", task_type);
                return Ok(task_type);
            }
            Err(e) => {
                debug!("Embedding classification failed, falling back to keywords: {}", e);
            }
        }
        
        // Step 2: Fallback to keyword-based classification
        Ok(self.classify_with_keywords(message, has_project))
    }
    
    /// Embedding-based classification using prototype examples
    async fn classify_with_embeddings(&self, message: &str) -> Result<TaskType> {
        // Prototype examples for each task type
        let code_prototypes = vec![
            "Fix this compilation error in the function",
            "Implement a method to handle user authentication",
            "Debug this stack trace and find the bug",
            "Refactor this code to use async/await",
            "error[E0308]: mismatched types in main.rs",
        ];
        
        let chat_prototypes = vec![
            "Explain how this algorithm works",
            "What do you think about this approach?",
            "Help me understand the trade-offs here",
            "Walk me through the architecture decisions",
            "Discuss the pros and cons of microservices",
        ];
        
        // Embed the user message
        let message_embedding = self.embedding_client.embed(message).await?;
        
        // Calculate average similarity to code prototypes
        let mut code_similarities = Vec::new();
        for prototype in &code_prototypes {
            let proto_embedding = self.embedding_client.embed(prototype).await?;
            let similarity = cosine_similarity(&message_embedding, &proto_embedding);
            code_similarities.push(similarity);
        }
        let avg_code_sim = code_similarities.iter().sum::<f32>() / code_similarities.len() as f32;
        
        // Calculate average similarity to chat prototypes
        let mut chat_similarities = Vec::new();
        for prototype in &chat_prototypes {
            let proto_embedding = self.embedding_client.embed(prototype).await?;
            let similarity = cosine_similarity(&message_embedding, &proto_embedding);
            chat_similarities.push(similarity);
        }
        let avg_chat_sim = chat_similarities.iter().sum::<f32>() / chat_similarities.len() as f32;
        
        // Decision threshold: need clear signal (>0.05 difference)
        let diff = (avg_code_sim - avg_chat_sim).abs();
        
        if diff < 0.05 {
            // Too close to call - let keywords decide
            return Err(anyhow::anyhow!("Ambiguous: code_sim={:.3}, chat_sim={:.3}", avg_code_sim, avg_chat_sim));
        }
        
        if avg_code_sim > avg_chat_sim {
            debug!("Code prototypes match better: {:.3} vs {:.3}", avg_code_sim, avg_chat_sim);
            Ok(TaskType::Code)
        } else {
            debug!("Chat prototypes match better: {:.3} vs {:.3}", avg_chat_sim, avg_code_sim);
            Ok(TaskType::Chat)
        }
    }
    
    /// Keyword-based classification (fallback)
    fn classify_with_keywords(&self, message: &str, has_project: bool) -> TaskType {
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
            // Tie: check for explicit code patterns
            if lower.contains("```") || lower.contains("error[") || lower.contains("fix this") {
                debug!("Detected Code task (code block/error pattern)");
                TaskType::Code
            } else {
                // Project context can hint at code work
                if has_project && code_score >= 1 {
                    debug!("Defaulting to Code (has project context)");
                    TaskType::Code
                } else {
                    // Default to Chat (GPT-5 for quality)
                    debug!("Defaulting to Chat (ambiguous, prefer quality)");
                    TaskType::Chat
                }
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
            if matches!(self.route(task_type).as_ref(), gpt5) {
                context = Some(ToolContext::Gpt5 {
                    previous_response_id: response.id.clone(),
                });
            }
            
            // Add assistant response to history
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: response.text_output.clone(),
            });
            
            // Add tool results to history (simplified - real impl would execute tools)
            current_messages.push(Message {
                role: "user".to_string(),
                content: "[tool results]".to_string(),
            });
        }
        
        Err(anyhow::anyhow!("Max iterations reached without completion"))
    }
}

/// Calculate cosine similarity between two embeddings
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (magnitude_a * magnitude_b)
}
