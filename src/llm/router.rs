// src/llm/router.rs
// Smart LLM router with DeepSeek-based task classification
// DeepSeek 3.2 for code, GPT-5 for reasoning/chat

use crate::llm::provider::{LlmProvider, Message, Response, ToolResponse, ToolContext};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::gpt5::Gpt5Provider;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    Code,  // DeepSeek: Fast, cheap code generation
    Chat,  // GPT-5: Complex reasoning, multi-turn
}

/// Simple classification cache to avoid redundant API calls
struct ClassificationCache {
    cache: HashMap<String, (TaskType, Instant)>,
    ttl: Duration,
}

impl ClassificationCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            ttl: Duration::from_secs(300), // 5 minutes
        }
    }
    
    fn get(&mut self, key: &str) -> Option<TaskType> {
        if let Some((task_type, cached_at)) = self.cache.get(key) {
            if cached_at.elapsed() < self.ttl {
                debug!("Cache HIT for classification");
                return Some(*task_type);
            } else {
                // Expired
                self.cache.remove(key);
            }
        }
        None
    }
    
    fn set(&mut self, key: String, task_type: TaskType) {
        self.cache.insert(key, (task_type, Instant::now()));
    }
    
    fn hash_message(message: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        message.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

pub struct LlmRouter {
    deepseek: Arc<DeepSeekProvider>,
    gpt5: Arc<Gpt5Provider>,
    cache: Arc<RwLock<ClassificationCache>>,
}

impl LlmRouter {
    pub fn new(
        deepseek: Arc<DeepSeekProvider>, 
        gpt5: Arc<Gpt5Provider>,
    ) -> Self {
        Self { 
            deepseek, 
            gpt5,
            cache: Arc::new(RwLock::new(ClassificationCache::new())),
        }
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
    
    /// Smart task type inference using DeepSeek (fast, cheap, accurate)
    pub async fn infer_task_type(&self, message: &str, _has_project: bool) -> Result<TaskType> {
        // Check cache first
        let cache_key = ClassificationCache::hash_message(message);
        {
            let mut cache = self.cache.write().await;
            if let Some(cached_type) = cache.get(&cache_key) {
                return Ok(cached_type);
            }
        }
        
        // Use DeepSeek to classify (fast: ~200-500ms, cheap: ~$0.00001 per message)
        match self.classify_with_llm(message).await {
            Ok(task_type) => {
                // Cache the result
                let mut cache = self.cache.write().await;
                cache.set(cache_key, task_type);
                Ok(task_type)
            }
            Err(e) => {
                warn!("LLM classification failed ({}), defaulting to Chat (GPT-5)", e);
                Ok(TaskType::Chat)  // Safe default - GPT-5 handles everything
            }
        }
    }
    
    /// LLM-based classification using DeepSeek
    async fn classify_with_llm(&self, message: &str) -> Result<TaskType> {
        let classification_prompt = format!(
            r#"Classify this user message as either "code" or "chat".

Message: "{}"

Classification rules:
- "code": Programming tasks, debugging, implementation, fixing errors, refactoring, writing code
- "chat": Explanations, discussions, reasoning, opinions, advice, general questions, understanding concepts

Respond with ONLY one word: code or chat"#,
            message.chars().take(500).collect::<String>() // Truncate long messages
        );
        
        let response = self.deepseek.chat(
            vec![Message {
                role: "user".to_string(),
                content: classification_prompt,
            }],
            "You are a task classifier. Respond with only the word 'code' or 'chat'.".to_string(),
        ).await?;
        
        let classification = response.content.trim().to_lowercase();
        
        match classification.as_str() {
            c if c.contains("code") => {
                debug!("🤖 DeepSeek classified as: Code");
                Ok(TaskType::Code)
            }
            c if c.contains("chat") => {
                debug!("🤖 DeepSeek classified as: Chat");
                Ok(TaskType::Chat)
            }
            _ => {
                // Response unclear, use keywords as fallback
                Err(anyhow::anyhow!("Unclear classification response: '{}'", classification))
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
            
            // For GPT-5 (Chat tasks), set previous_response_id for next call
            if task_type == TaskType::Chat {
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
