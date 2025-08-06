// src/services/chat.rs

use crate::llm::client::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::llm::schema::{ChatResponse, MiraStructuredReply};
use crate::tools::web_search::{
    web_search_tool_definition, 
    WebSearchConfig,
    ToolCall,
};
use crate::tools::WebSearchHandler;  // Import from tools module directly
use anyhow::{Result, Context};
use serde_json::{json, Value};
use reqwest::Method;
use std::sync::Arc;

pub const DEFAULT_LLM_MODEL: &str = "gpt-4.1";  // Keep as gpt-4.1

#[derive(Clone)]
pub struct ChatService {
    pub llm_client: Arc<OpenAIClient>,
    pub web_search_handler: Option<Arc<WebSearchHandler>>,
}

impl ChatService {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        // Initialize web search if API key is available
        // Load .env if not already loaded (for tests and main app)
        dotenv::dotenv().ok();
        
        let web_search_handler = if let Ok(api_key) = std::env::var("TAVILY_API_KEY") {
            let config = WebSearchConfig {
                provider: crate::tools::web_search::SearchProvider::Tavily,
                api_key: Some(api_key),
                ..Default::default()
            };
            
            match WebSearchHandler::new(config) {
                Ok(handler) => {
                    eprintln!("✅ Web search tool initialized with Tavily");
                    Some(Arc::new(handler))
                },
                Err(e) => {
                    eprintln!("⚠️ Failed to initialize web search: {:?}", e);
                    None
                }
            }
        } else {
            eprintln!("ℹ️ Web search disabled (no TAVILY_API_KEY)");
            None
        };

        Self { 
            llm_client,
            web_search_handler,
        }
    }

    /// Process message with optional tool support
    pub async fn process_message(
        &self,
        _session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        eprintln!("🎭 ChatService using persona: {}", persona);
        
        // Build system prompt with Mira's personality
        let mut system_prompt = String::new();
        system_prompt.push_str(persona.prompt());
        system_prompt.push_str("\n\n");
        
        // Add web search guidance if available
        if self.web_search_handler.is_some() {
            system_prompt.push_str("You have access to a web_search tool. Use it when:\n");
            system_prompt.push_str("- The user asks about current events, news, or recent happenings\n");
            system_prompt.push_str("- You need information from after January 2025 (your knowledge cutoff)\n");
            system_prompt.push_str("- The question involves real-time data (prices, scores, weather, etc.)\n");
            system_prompt.push_str("- You're unsure if your information is current or accurate\n");
            system_prompt.push_str("- The user explicitly asks you to search or look something up\n\n");
            system_prompt.push_str("Don't search for:\n");
            system_prompt.push_str("- Historical facts that don't change\n");
            system_prompt.push_str("- Basic knowledge (math, science concepts, programming)\n");
            system_prompt.push_str("- Personal advice or creative tasks\n\n");
        }
        
        // Add structured output requirements for final response
        system_prompt.push_str("When providing your FINAL response (not during tool use), format it as a JSON object with these fields:\n");
        system_prompt.push_str("- output: Your actual reply to the user (string)\n");
        system_prompt.push_str("- persona: The persona overlay in use (string)\n");
        system_prompt.push_str("- mood: The emotional tone of your reply (string)\n");
        system_prompt.push_str("- salience: How emotionally important this reply is (integer 0-10)\n");
        system_prompt.push_str("- summary: Short summary of your reply/context (string or null)\n");
        system_prompt.push_str("- memory_type: \"feeling\", \"fact\", \"joke\", \"promise\", \"event\", or \"other\" (string)\n");
        system_prompt.push_str("- tags: List of context/mood tags (array of strings)\n");
        system_prompt.push_str("- intent: Your intent in this reply (string)\n");
        system_prompt.push_str("- monologue: Your private inner thoughts, not shown to user (string or null)\n");
        system_prompt.push_str("- reasoning_summary: Your reasoning/chain-of-thought, if any (string or null)\n");

        // Include project context if available
        let user_message = if let Some(proj_id) = project_id {
            format!("[Project: {}]\n{}", proj_id, content)
        } else {
            content.to_string()
        };

        // Build messages
        let mut messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": user_message}),
        ];

        // Build tools array if web search is available
        let tools = if self.web_search_handler.is_some() {
            vec![web_search_tool_definition()]
        } else {
            vec![]
        };

        // Call OpenAI with tools
        let response = if !tools.is_empty() {
            self.process_with_tools(&mut messages, tools, persona.to_string()).await?
        } else {
            // Fallback to simple chat without tools
            self.process_without_tools(&messages, persona.to_string()).await?
        };

        Ok(response)
    }

    /// Process chat with tool support
    async fn process_with_tools(
        &self,
        messages: &mut Vec<Value>,
        tools: Vec<Value>,
        persona: String,
    ) -> Result<ChatResponse> {
        eprintln!("🔧 Processing with tools enabled");
        
        // First API call - model may request tool use
        let first_response = self.llm_client
            .chat_with_tools(
                messages.clone(),
                tools.clone(),
                None, // Let model decide
                Some(DEFAULT_LLM_MODEL),
            )
            .await?;

        // Check if model wants to use tools
        if let Some(tool_calls) = first_response["choices"][0]["message"]["tool_calls"].as_array() {
            eprintln!("🔍 Model requested {} tool call(s)", tool_calls.len());
            
            // Add assistant's message with tool calls to history
            messages.push(first_response["choices"][0]["message"].clone());
            
            // Process each tool call
            for tool_call_json in tool_calls {
                let tool_call: ToolCall = serde_json::from_value(tool_call_json.clone())
                    .context("Failed to parse tool call")?;
                
                if tool_call.function.name == "web_search" {
                    if let Some(handler) = &self.web_search_handler {
                        eprintln!("🌐 Executing web search: {}", tool_call.function.arguments);
                        
                        match handler.handle_tool_call(&tool_call).await {
                            Ok(result) => {
                                // Add tool result to messages
                                messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call.id,
                                    "content": result.content,
                                }));
                            }
                            Err(e) => {
                                eprintln!("❌ Tool call failed: {:?}", e);
                                messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call.id,
                                    "content": format!("Search failed: {}", e),
                                }));
                            }
                        }
                    }
                }
            }
            
            // Second API call with tool results - request JSON response
            eprintln!("📤 Sending tool results back to model");
            
            // Add a user message to ensure JSON format
            messages.push(json!({
                "role": "user",
                "content": "Now provide your complete response based on the search results. Format as the specified JSON object."
            }));
            
            let final_response = self.llm_client
                .chat_with_tools(
                    messages.clone(),
                    vec![], // No tools this time
                    None,
                    Some(DEFAULT_LLM_MODEL),
                )
                .await?;
            
            self.parse_llm_response(final_response, persona)
        } else {
            // Model didn't use tools, parse direct response
            self.parse_llm_response(first_response, persona)
        }
    }

    /// Process without tools (backward compatibility)
    async fn process_without_tools(
        &self,
        messages: &Vec<Value>,
        persona: String,
    ) -> Result<ChatResponse> {
        let payload = json!({
            "model": DEFAULT_LLM_MODEL,
            "messages": messages,
            "temperature": 0.9,
            "response_format": { "type": "json_object" },
        });

        let res = self.llm_client
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to call OpenAI chat API")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }

        let response_json: Value = res.json().await
            .context("Failed to parse OpenAI response")?;

        self.parse_llm_response(response_json, persona)
    }

    /// Parse LLM response into ChatResponse
    fn parse_llm_response(&self, response_json: Value, persona: String) -> Result<ChatResponse> {
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");

        eprintln!("📥 Raw LLM response: {}", content);

        // Try to parse as JSON
        let chat_response = serde_json::from_str::<MiraStructuredReply>(content)
            .map(|reply| ChatResponse {
                output: reply.output,
                persona: reply.persona,
                mood: reply.mood,
                salience: reply.salience,
                summary: reply.summary,
                memory_type: reply.memory_type,
                tags: reply.tags,
                intent: reply.intent,
                monologue: reply.monologue,
                reasoning_summary: reply.reasoning_summary,
                aside_intensity: None,
            })
            .unwrap_or_else(|e| {
                eprintln!("⚠️ Failed to parse JSON response: {:?}", e);
                eprintln!("Raw content was: {}", content);
                
                // Fallback response
                ChatResponse {
                    output: content.to_string(),
                    persona,
                    mood: "confused".to_string(),
                    salience: 5,
                    summary: Some("Failed to parse structured response".to_string()),
                    memory_type: "other".to_string(),
                    tags: vec!["fallback".to_string()],
                    intent: "chat".to_string(),
                    monologue: Some("My JSON formatting got messed up, but I'm still here!".to_string()),
                    reasoning_summary: None,
                    aside_intensity: None,
                }
            });

        Ok(chat_response)
    }

    /// LLM-powered helper: Use GPT-4 to route a document upload
    pub async fn run_routing_inference(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let payload = json!({
            "model": DEFAULT_LLM_MODEL,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.3,
            "stream": false,
        });

        let res = self.llm_client
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to call OpenAI chat API for routing")?
            .error_for_status()
            .context("Non-2xx from OpenAI chat/completions for routing")?
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse OpenAI routing response")?;

        let output = res["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(output)
    }
}
