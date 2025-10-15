// src/llm/provider/deepseek.rs
// DeepSeek provider - specialized for cheap code generation with JSON mode
// Owns its own context building for codegen
// FIXED: Removed ALL arbitrary content truncation

use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info, error};

use crate::config::CONFIG;
use crate::llm::provider::Message;
use crate::memory::features::recall_engine::RecallContext;
use crate::api::ws::message::MessageMetadata;

pub struct DeepSeekProvider {
    client: Client,
}

impl DeepSeekProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
    
    /// Generate code artifact using DeepSeek reasoner with JSON mode
    /// Builds its own rich context from provided materials
    pub async fn generate_code_artifact(
        &self,
        tool_input: &Value,
        messages: &[Message],
        context: &RecallContext,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        previous_tool_results: &[(String, Value)],
    ) -> Result<Value> {
        info!("DeepSeek: Generating code artifact via reasoner");
        
        // Build rich context for code generation
        let codegen_context = self.build_context(
            messages,
            context,
            metadata,
            project_id,
            tool_input,
            previous_tool_results,
        );
        
        debug!("DeepSeek context built: {} chars", codegen_context.len());
        
        // Extract parameters from tool input
        let description = tool_input.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let path = tool_input.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let language = tool_input.get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("typescript");
        
        // Build prompt for code generation
        let system_prompt = format!(
            "You are a code generation specialist. Generate clean, working code based on the user's requirements.\n\
            Output ONLY valid JSON with this exact structure:\n\
            {{\n  \
              \"path\": \"file/path/here\",\n  \
              \"content\": \"complete file content here\",\n  \
              \"language\": \"typescript|rust|python|javascript\"\n\
            }}\n\n\
            CRITICAL:\n\
            - Generate COMPLETE files, never use '...' or placeholders\n\
            - Include ALL imports, functions, types, and closing braces\n\
            - The content field must contain the entire working file\n\
            - Use proper {} language syntax and best practices",
            language
        );
        
        let user_prompt = format!(
            "Generate a {} file at path: {}\n\n\
            Description: {}\n\n\
            Additional context:\n{}\n\n\
            Remember: Output ONLY the JSON object, no other text.",
            language, path, description, codegen_context
        );
        
        // Call DeepSeek API with JSON mode
        let request_body = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "response_format": {"type": "json_object"},
            "temperature": 0.7,
            "max_tokens": 32000,  // DeepSeek Reasoner supports up to 32K output tokens
        });
        
        let response = self.client
            .post("https://api.deepseek.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", CONFIG.deepseek_api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                error!("DeepSeek API request failed: {}", e);
                anyhow::anyhow!("DeepSeek API request failed: {}", e)
            })?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            error!("DeepSeek API error {}: {}", status, error_text);
            return Err(anyhow::anyhow!("DeepSeek API error {}: {}", status, error_text));
        }
        
        let response_json: Value = response.json().await
            .map_err(|e| {
                error!("Failed to parse DeepSeek response: {}", e);
                anyhow::anyhow!("Failed to parse DeepSeek response: {}", e)
            })?;
        
        debug!("DeepSeek raw response: {}", serde_json::to_string_pretty(&response_json).unwrap_or_default());
        
        // Extract content from response
        let content_str = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                error!("Invalid DeepSeek response structure: {:?}", response_json);
                anyhow::anyhow!("Invalid DeepSeek response structure")
            })?;
        
        // Parse the JSON content
        let artifact_json: Value = serde_json::from_str(content_str)
            .map_err(|e| {
                error!("Failed to parse DeepSeek artifact JSON: {} - Content: {}", e, content_str);
                anyhow::anyhow!("Failed to parse DeepSeek artifact JSON: {}", e)
            })?;
        
        // Validate required fields
        if artifact_json.get("path").is_none() || 
           artifact_json.get("content").is_none() || 
           artifact_json.get("language").is_none() {
            error!("DeepSeek artifact missing required fields: {:?}", artifact_json);
            return Err(anyhow::anyhow!("DeepSeek artifact missing required fields"));
        }
        
        info!("DeepSeek: Successfully generated artifact at {}", 
              artifact_json["path"].as_str().unwrap_or("unknown"));
        
        Ok(artifact_json)
    }
    
    /// Build rich context for DeepSeek code generation
    /// Includes: generation intent, project info, tool results, conversation, and memory
    /// FIXED: No arbitrary truncation - let token limits handle content size naturally
    fn build_context(
        &self,
        messages: &[Message],
        context: &RecallContext,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        tool_arguments: &Value,
        previous_tool_results: &[(String, Value)],
    ) -> String {
        let mut ctx = String::new();
        
        // Generation intent - what the user is asking for
        ctx.push_str("=== GENERATION REQUEST ===\n");
        if let Some(desc) = tool_arguments.get("description").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Task: {}\n", desc));
        }
        if let Some(path) = tool_arguments.get("path").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Target file: {}\n", path));
        }
        if let Some(lang) = tool_arguments.get("language").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Language: {}\n", lang));
        }
        ctx.push_str("\n");
        
        // Project context from metadata
        if let Some(meta) = metadata {
            ctx.push_str("=== PROJECT INFO ===\n");
            if let Some(project_name) = &meta.project_name {
                ctx.push_str(&format!("Name: {}\n", project_name));
                
                if meta.has_repository == Some(true) {
                    ctx.push_str("Type: Git repository\n");
                    if let Some(branch) = &meta.branch {
                        ctx.push_str(&format!("Branch: {}\n", branch));
                    }
                    if let Some(root) = &meta.repo_root {
                        ctx.push_str(&format!("Root: {}\n", root));
                    }
                }
            }
            
            // Current file context (if editing existing file)
            // FIXED: Show full content, no truncation
            if let Some(file_path) = &meta.file_path {
                ctx.push_str(&format!("\nCurrent file: {}\n", file_path));
                if let Some(content) = &meta.file_content {
                    ctx.push_str(&format!("Content:\n```\n{}\n```\n", content));
                }
            }
            ctx.push_str("\n");
        } else if let Some(pid) = project_id {
            ctx.push_str(&format!("=== PROJECT INFO ===\nID: {}\n\n", pid));
        }
        
        // Previous tool results - critical for multi-step codegen
        // FIXED: Show full content from tool results, no truncation
        if !previous_tool_results.is_empty() {
            ctx.push_str("=== PREVIOUS TOOL RESULTS ===\n");
            for (tool_name, result) in previous_tool_results {
                ctx.push_str(&format!("Tool: {}\n", tool_name));
                
                // Format based on tool type for readability
                match tool_name.as_str() {
                    "read_file" => {
                        if let Some(content) = result.get("content").and_then(|c| c.as_str()) {
                            ctx.push_str(&format!("Content:\n```\n{}\n```\n", content));
                        }
                    },
                    "search_code" => {
                        if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                            ctx.push_str(&format!("Found {} matches:\n", results.len()));
                            
                            // Show up to 5 results with full details
                            for (i, r) in results.iter().take(5).enumerate() {
                                let element_type = r.get("element_type").and_then(|t| t.as_str()).unwrap_or("unknown");
                                let name = r.get("name").and_then(|n| n.as_str()).unwrap_or("unnamed");
                                let full_path = r.get("full_path").and_then(|p| p.as_str()).unwrap_or("unknown path");
                                
                                ctx.push_str(&format!("\n  {}. {} '{}' in {}\n", i + 1, element_type, name, full_path));
                                
                                // Show line range
                                if let (Some(start), Some(end)) = (
                                    r.get("start_line").and_then(|s| s.as_i64()),
                                    r.get("end_line").and_then(|e| e.as_i64())
                                ) {
                                    ctx.push_str(&format!("     Lines {}-{}\n", start, end));
                                }
                                
                                // Show visibility and flags
                                if let Some(visibility) = r.get("visibility").and_then(|v| v.as_str()) {
                                    let mut flags = vec![visibility];
                                    if r.get("is_async").and_then(|a| a.as_bool()).unwrap_or(false) {
                                        flags.push("async");
                                    }
                                    if r.get("is_test").and_then(|t| t.as_bool()).unwrap_or(false) {
                                        flags.push("test");
                                    }
                                    ctx.push_str(&format!("     Attributes: {}\n", flags.join(", ")));
                                }
                                
                                // Show complexity for functions
                                if element_type == "function" {
                                    if let Some(complexity) = r.get("complexity_score").and_then(|c| c.as_i64()) {
                                        if complexity > 0 {
                                            ctx.push_str(&format!("     Complexity: {}\n", complexity));
                                        }
                                    }
                                }
                                
                                // FIXED: Show full documentation, no truncation
                                if let Some(doc) = r.get("documentation").and_then(|d| d.as_str()) {
                                    ctx.push_str(&format!("     Doc: {}\n", doc));
                                }
                                
                                // FIXED: Show full code snippet, no truncation
                                if let Some(content) = r.get("content").and_then(|c| c.as_str()) {
                                    ctx.push_str(&format!("     Code:\n```\n{}\n```\n", content));
                                }
                            }
                            
                            if results.len() > 5 {
                                ctx.push_str(&format!("\n... and {} more matches (showing top 5)\n", results.len() - 5));
                            }
                            ctx.push_str("\n");
                        }
                    },
                    _ => {
                        // Generic result formatting
                        ctx.push_str(&format!("{}\n", serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())));
                    }
                }
                ctx.push_str("\n");
            }
        }
        
        // Recent conversation (last 5 messages)
        // FIXED: Show full message content, no truncation
        if messages.len() > 1 {
            ctx.push_str("=== RECENT CONVERSATION ===\n");
            for msg in messages.iter().rev().take(5).rev() {
                let role = match msg.role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    _ => "System",
                };
                ctx.push_str(&format!("{}: {}\n", role, msg.content));
            }
            ctx.push_str("\n");
        }
        
        // Session summary from memory
        if let Some(summary) = &context.session_summary {
            ctx.push_str("=== SESSION SUMMARY ===\n");
            ctx.push_str(&format!("{}\n\n", summary));
        }
        
        // High-salience recent memories (top 3)
        // Helps maintain consistency with patterns user has established
        if !context.recent.is_empty() {
            let high_salience: Vec<_> = context.recent.iter()
                .filter(|m| m.salience.unwrap_or(0.0) > 0.7)
                .take(3)
                .collect();
                
            if !high_salience.is_empty() {
                ctx.push_str("=== HIGH-SALIENCE CONTEXT ===\n");
                for mem in high_salience {
                    if let Some(summary) = &mem.summary {
                        ctx.push_str(&format!("- {}\n", summary));
                    }
                }
                ctx.push_str("\n");
            }
        }
        
        ctx
    }
    
    /// Check if DeepSeek is configured and available
    pub fn is_available() -> bool {
        CONFIG.use_deepseek_codegen && !CONFIG.deepseek_api_key.is_empty()
    }
}
