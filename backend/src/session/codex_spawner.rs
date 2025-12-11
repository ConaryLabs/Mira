// backend/src/session/codex_spawner.rs
// Spawns and manages background Codex sessions with compaction support

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::llm::provider::openai::OpenAIProvider;
use crate::llm::provider::{Message, ToolCallInfo};
use crate::operations::engine::tool_router::ToolRouter;
use crate::operations::tools;
use crate::session::injection::InjectionService;
use crate::session::manager::SessionManager;
use crate::session::types::*;

/// Maximum iterations for a Codex session before forced completion
const MAX_ITERATIONS: u32 = 1000;

/// Events emitted during Codex session execution
#[derive(Debug, Clone)]
pub enum CodexEvent {
    /// Codex session spawned
    Spawned {
        voice_session_id: String,
        codex_session_id: String,
        task_description: String,
    },
    /// Progress update
    Progress {
        voice_session_id: String,
        codex_session_id: String,
        iteration: u32,
        current_activity: String,
        tokens_used: i64,
    },
    /// Tool was executed
    ToolExecuted {
        codex_session_id: String,
        tool_name: String,
        success: bool,
    },
    /// Compaction was triggered by OpenAI
    CompactionTriggered {
        codex_session_id: String,
        iteration: u32,
    },
    /// Codex session completed
    Completed {
        voice_session_id: String,
        codex_session_id: String,
        summary: String,
        files_changed: Vec<String>,
        duration_seconds: i64,
    },
    /// Codex session failed
    Failed {
        voice_session_id: String,
        codex_session_id: String,
        error: String,
    },
}

/// Spawner for background Codex sessions
pub struct CodexSpawner {
    provider: Arc<OpenAIProvider>,
    tool_router: Arc<ToolRouter>,
    session_manager: Arc<SessionManager>,
    injection_service: Arc<InjectionService>,
}

impl CodexSpawner {
    pub fn new(
        provider: Arc<OpenAIProvider>,
        tool_router: Arc<ToolRouter>,
        session_manager: Arc<SessionManager>,
        injection_service: Arc<InjectionService>,
    ) -> Self {
        Self {
            provider,
            tool_router,
            session_manager,
            injection_service,
        }
    }

    /// Spawn a background Codex session
    ///
    /// Returns the Codex session ID and an event channel for monitoring.
    pub async fn spawn(
        &self,
        voice_session_id: &str,
        task_description: &str,
        trigger: CodexSpawnTrigger,
        voice_context_summary: Option<String>,
        project_path: Option<String>,
    ) -> Result<(String, mpsc::Receiver<CodexEvent>)> {
        // Create Codex session in database
        let codex_session_id = self
            .session_manager
            .spawn_codex_session(
                voice_session_id,
                task_description,
                &trigger,
                voice_context_summary.as_deref(),
            )
            .await
            .context("Failed to spawn Codex session")?;

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel::<CodexEvent>(100);

        // Clone for the spawned task
        let provider = Arc::clone(&self.provider);
        let tool_router = Arc::clone(&self.tool_router);
        let session_manager = Arc::clone(&self.session_manager);
        let injection_service = Arc::clone(&self.injection_service);
        let voice_id = voice_session_id.to_string();
        let codex_id = codex_session_id.clone();
        let task = task_description.to_string();
        let context = voice_context_summary.clone();

        // Emit spawned event
        let _ = event_tx
            .send(CodexEvent::Spawned {
                voice_session_id: voice_id.clone(),
                codex_session_id: codex_id.clone(),
                task_description: task.clone(),
            })
            .await;

        // Spawn background task
        tokio::spawn(async move {
            let result = run_codex_session(
                &provider,
                &tool_router,
                &session_manager,
                &injection_service,
                &voice_id,
                &codex_id,
                &task,
                context.as_deref(),
                project_path.as_deref(),
                event_tx.clone(),
            )
            .await;

            if let Err(e) = result {
                error!(
                    codex_session_id = %codex_id,
                    error = %e,
                    "Codex session failed"
                );

                // Mark session as failed
                if let Err(fail_err) = session_manager
                    .fail_codex_session(&codex_id, &e.to_string())
                    .await
                {
                    error!("Failed to mark Codex session as failed: {}", fail_err);
                }

                // Inject error notification
                if let Err(inj_err) = injection_service
                    .inject_codex_error(&voice_id, &codex_id, &e.to_string(), &task)
                    .await
                {
                    error!("Failed to inject error notification: {}", inj_err);
                }

                let _ = event_tx
                    .send(CodexEvent::Failed {
                        voice_session_id: voice_id,
                        codex_session_id: codex_id,
                        error: e.to_string(),
                    })
                    .await;
            }
        });

        info!(
            voice_session_id = %voice_session_id,
            codex_session_id = %codex_session_id,
            task = %task_description,
            "Spawned background Codex session"
        );

        Ok((codex_session_id, event_rx))
    }
}

/// Run a Codex session with tool loop and compaction
async fn run_codex_session(
    provider: &OpenAIProvider,
    tool_router: &ToolRouter,
    session_manager: &SessionManager,
    injection_service: &InjectionService,
    voice_session_id: &str,
    codex_session_id: &str,
    task_description: &str,
    voice_context: Option<&str>,
    project_path: Option<&str>,
    event_tx: mpsc::Sender<CodexEvent>,
) -> Result<()> {
    let start_time = Instant::now();

    // Build system prompt for Codex session
    let system_prompt = build_codex_system_prompt(task_description, voice_context, project_path);

    // Build initial messages
    let mut messages = vec![Message::user(task_description.to_string())];

    // Get available tools
    let tools = tools::get_llm_tools();

    // Track metrics
    let mut total_tokens_input: i64 = 0;
    let mut total_tokens_output: i64 = 0;
    let compaction_count: u32 = 0;
    let mut files_changed: Vec<String> = Vec::new();
    let mut accumulated_response = String::new();
    let mut previous_response_id: Option<String> = None;

    // Tool-calling loop with compaction
    for iteration in 1..=MAX_ITERATIONS {
        debug!(
            codex_session_id = %codex_session_id,
            iteration = iteration,
            "Codex iteration"
        );

        // Emit progress event every 5 iterations
        if iteration % 5 == 1 {
            let _ = event_tx
                .send(CodexEvent::Progress {
                    voice_session_id: voice_session_id.to_string(),
                    codex_session_id: codex_session_id.to_string(),
                    iteration,
                    current_activity: format!("Iteration {}", iteration),
                    tokens_used: total_tokens_input + total_tokens_output,
                })
                .await;
        }

        // Call LLM with compaction support
        let response = provider
            .chat_with_tools_continuing(
                messages.clone(),
                system_prompt.clone(),
                tools.clone(),
                previous_response_id.clone(),
                None, // No tool forcing for Codex sessions - let model decide
            )
            .await
            .context("LLM call failed")?;

        // Check for compaction (response ID changed significantly or context was pruned)
        let new_response_id = response.id.clone();
        if previous_response_id.is_some() {
            // Heuristic: if response ID prefix changes, compaction may have occurred
            // OpenAI doesn't explicitly signal compaction, but we can track usage patterns
            debug!(
                codex_session_id = %codex_session_id,
                new_response_id = %new_response_id,
                "Response ID for compaction tracking"
            );
        }
        previous_response_id = Some(new_response_id.clone());

        // Update response ID in database for crash recovery
        if let Err(e) = session_manager
            .update_response_id(codex_session_id, &new_response_id)
            .await
        {
            warn!("Failed to update response_id: {}", e);
        }

        // Track tokens
        total_tokens_input += response.tokens.input;
        total_tokens_output += response.tokens.output;

        // Accumulate text response
        if !response.text_output.is_empty() {
            accumulated_response.push_str(&response.text_output);
            accumulated_response.push('\n');
        }

        // No more tool calls - we're done
        if response.function_calls.is_empty() {
            debug!(
                codex_session_id = %codex_session_id,
                "No more tool calls, completing"
            );
            break;
        }

        // Build assistant message with tool calls
        let tool_calls_info: Vec<ToolCallInfo> = response
            .function_calls
            .iter()
            .map(|tc| ToolCallInfo {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            })
            .collect();

        messages.push(Message::assistant_with_tool_calls(
            response.text_output.clone(),
            tool_calls_info,
        ));

        // Execute tool calls
        for tool_call in &response.function_calls {
            debug!(
                codex_session_id = %codex_session_id,
                tool = %tool_call.name,
                "Executing tool"
            );

            // Track file changes
            if tool_call.name == "write_file" || tool_call.name == "create_file" {
                if let Some(path) = tool_call.arguments.get("path").and_then(|p| p.as_str()) {
                    if !files_changed.contains(&path.to_string()) {
                        files_changed.push(path.to_string());
                    }
                }
            }

            let result = tool_router
                .route_tool_call_with_context(
                    &tool_call.name,
                    tool_call.arguments.clone(),
                    project_path,
                    codex_session_id,
                )
                .await;

            let (result_value, success) = match result {
                Ok(v) => (v, true),
                Err(e) => (serde_json::json!({"error": e.to_string()}), false),
            };

            // Emit tool executed event
            let _ = event_tx
                .send(CodexEvent::ToolExecuted {
                    codex_session_id: codex_session_id.to_string(),
                    tool_name: tool_call.name.clone(),
                    success,
                })
                .await;

            // Add tool result to messages
            messages.push(Message::tool_result(
                tool_call.id.clone(),
                tool_call.name.clone(),
                serde_json::to_string(&result_value).unwrap_or_else(|_| result_value.to_string()),
            ));
        }

        // Update usage stats periodically
        if iteration % 10 == 0 {
            let _ = session_manager
                .update_codex_usage(
                    codex_session_id,
                    response.tokens.input,
                    response.tokens.output,
                    0.0, // Cost calculated separately
                    false,
                )
                .await;
        }
    }

    // Calculate duration and cost
    let duration_seconds = start_time.elapsed().as_secs() as i64;
    let cost_usd = provider.calculate_cost(&crate::llm::provider::TokenUsage {
        input: total_tokens_input,
        output: total_tokens_output,
        reasoning: 0,
        cached: 0,
    });

    // Generate completion summary
    let summary = generate_completion_summary(
        task_description,
        &accumulated_response,
        &files_changed,
        duration_seconds,
    );

    // Complete the session
    let _voice_id = session_manager
        .complete_codex_session(
            codex_session_id,
            &summary,
            total_tokens_input,
            total_tokens_output,
            cost_usd,
            compaction_count as i32,
        )
        .await?;

    // Inject completion summary into Voice session
    let metadata = CodexCompletionMetadata {
        files_changed: files_changed.clone(),
        duration_seconds,
        tokens_total: total_tokens_input + total_tokens_output,
        cost_usd,
        tool_calls_count: 0, // Would need to track this
        compaction_count,
        key_actions: vec![task_description.to_string()],
    };

    injection_service
        .inject_codex_completion(voice_session_id, codex_session_id, &summary, metadata)
        .await?;

    // Emit completed event
    let _ = event_tx
        .send(CodexEvent::Completed {
            voice_session_id: voice_session_id.to_string(),
            codex_session_id: codex_session_id.to_string(),
            summary: summary.clone(),
            files_changed: files_changed.clone(),
            duration_seconds,
        })
        .await;

    info!(
        codex_session_id = %codex_session_id,
        duration_seconds = duration_seconds,
        tokens = total_tokens_input + total_tokens_output,
        files_changed = files_changed.len(),
        "Codex session completed"
    );

    Ok(())
}

/// Build system prompt for Codex session
fn build_codex_system_prompt(
    task_description: &str,
    voice_context: Option<&str>,
    project_path: Option<&str>,
) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are Mira's Codex agent, specialized for autonomous code tasks.\n");
    prompt.push_str("Execute the given task completely, using available tools as needed.\n");
    prompt.push_str("When the task is complete, stop making tool calls and provide a brief summary.\n\n");

    if let Some(project) = project_path {
        prompt.push_str(&format!("Working directory: {}\n\n", project));
    }

    if let Some(context) = voice_context {
        prompt.push_str("## Context from Voice Session\n");
        prompt.push_str(context);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Task\n");
    prompt.push_str(task_description);
    prompt.push_str("\n\n");

    prompt.push_str("## Guidelines\n");
    prompt.push_str("- Work autonomously without user interaction\n");
    prompt.push_str("- Make all necessary file changes to complete the task\n");
    prompt.push_str("- Test your changes when possible\n");
    prompt.push_str("- When done, provide a concise summary of what was accomplished\n");

    prompt
}

/// Generate a completion summary for Voice session injection
fn generate_completion_summary(
    task_description: &str,
    accumulated_response: &str,
    files_changed: &[String],
    duration_seconds: i64,
) -> String {
    let mut summary = String::new();

    summary.push_str(&format!("Completed: {}\n\n", task_description));

    if !files_changed.is_empty() {
        summary.push_str("Files modified:\n");
        for file in files_changed.iter().take(10) {
            summary.push_str(&format!("- {}\n", file));
        }
        if files_changed.len() > 10 {
            summary.push_str(&format!("... and {} more\n", files_changed.len() - 10));
        }
        summary.push('\n');
    }

    // Extract last meaningful paragraph from accumulated response
    let paragraphs: Vec<&str> = accumulated_response
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .collect();

    if let Some(last) = paragraphs.last() {
        if last.len() < 500 {
            summary.push_str("Summary: ");
            summary.push_str(last.trim());
            summary.push('\n');
        }
    }

    let minutes = duration_seconds / 60;
    let seconds = duration_seconds % 60;
    summary.push_str(&format!("\nDuration: {}m {}s", minutes, seconds));

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_codex_system_prompt() {
        let prompt = build_codex_system_prompt(
            "Implement feature X",
            Some("User wants a new feature for authentication"),
            Some("/home/user/project"),
        );

        assert!(prompt.contains("Mira's Codex agent"));
        assert!(prompt.contains("Implement feature X"));
        assert!(prompt.contains("/home/user/project"));
        assert!(prompt.contains("authentication"));
    }

    #[test]
    fn test_generate_completion_summary() {
        let summary = generate_completion_summary(
            "Add login feature",
            "Created login form\n\nImplemented validation\n\nAll tests pass.",
            &vec!["src/login.rs".to_string(), "src/auth.rs".to_string()],
            125,
        );

        assert!(summary.contains("Add login feature"));
        assert!(summary.contains("src/login.rs"));
        assert!(summary.contains("2m 5s"));
    }
}
