//! Process management for spawned Claude Code sessions
//!
//! Handles spawning, lifecycle, and I/O for Claude Code processes.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

use super::stream::{detect_question, StreamParser};
use super::types::{SessionDetails, SessionEvent, SessionStatus, SpawnConfig, SpawnerConfig, StreamEvent};

/// Managed Claude Code process
pub struct ClaudeProcess {
    /// Session ID
    pub session_id: String,
    /// Process handle
    child: Child,
    /// Stdin for injecting messages
    stdin: Option<ChildStdin>,
    /// Current status
    pub status: SessionStatus,
    /// Unix timestamp when spawned (used for timeout checks)
    #[allow(dead_code)]
    pub spawned_at: i64,
    /// Project path (used for context/logging)
    #[allow(dead_code)]
    pub project_path: String,
    /// Instruction ID this session is executing (for completion tracking)
    #[allow(dead_code)]
    pub instruction_id: Option<String>,
}

impl ClaudeProcess {
    /// Inject a user message into the session via stdin
    pub async fn inject_message(&mut self, message: &str) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .context("Process stdin not available")?;

        // Claude Code stream-json input format
        let msg = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": message
            }
        });

        let line = format!("{}\n", serde_json::to_string(&msg)?);

        // Log the exact JSON being injected for debugging
        info!(
            session_id = %self.session_id,
            message_preview = %if message.len() > 100 { format!("{}...", &message[..100]) } else { message.to_string() },
            json_line = %line.trim(),
            "Injecting message into Claude Code stdin"
        );

        stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write to stdin")?;
        stdin.flush().await.context("Failed to flush stdin")?;

        debug!(session_id = %self.session_id, "Message injected successfully");
        Ok(())
    }

    /// Check if process is still running
    #[allow(dead_code)]
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().map(|s| s.is_none()).unwrap_or(false)
    }

    /// Wait for process to complete
    pub async fn wait(&mut self) -> Result<i32> {
        let status = self.child.wait().await.context("Failed to wait on child")?;
        Ok(status.code().unwrap_or(-1))
    }

    /// Kill the process
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.context("Failed to kill process")
    }
}

/// Spawner for Claude Code processes
pub struct ClaudeCodeSpawner {
    /// Database connection
    db: SqlitePool,
    /// Active processes by session ID
    processes: Arc<RwLock<HashMap<String, ClaudeProcess>>>,
    /// Event broadcaster for SSE
    event_tx: broadcast::Sender<SessionEvent>,
    /// Configuration
    config: SpawnerConfig,
}

impl ClaudeCodeSpawner {
    pub fn new(db: SqlitePool, config: SpawnerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            db,
            processes: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            config,
        }
    }

    /// Subscribe to session events
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }

    /// Start heartbeat task that sends periodic keepalive events
    pub fn start_heartbeat(&self, interval_secs: u64) {
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));

            loop {
                interval.tick().await;
                let ts = chrono::Utc::now().timestamp();
                // Ignore send errors (no subscribers)
                let _ = event_tx.send(SessionEvent::Heartbeat { ts });
            }
        });

        info!(interval_secs, "Started SSE heartbeat task");
    }

    /// Spawn a new Claude Code session
    pub async fn spawn(&self, config: SpawnConfig) -> Result<String> {
        // Check concurrent session limit
        let active_count = self.processes.read().await.len();
        if active_count >= self.config.max_concurrent_sessions {
            bail!(
                "Maximum concurrent sessions ({}) reached",
                self.config.max_concurrent_sessions
            );
        }

        // Generate session ID (must be valid UUID for Claude CLI)
        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        info!(session_id = %session_id, project = %config.project_path, "Spawning Claude Code session");

        // Build command
        let mut cmd = Command::new(&self.config.claude_binary);
        cmd.arg("--print")
            .arg("--verbose") // Required for stream-json output
            .arg("--output-format")
            .arg("stream-json")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--dangerously-skip-permissions")
            .arg("--session-id")
            .arg(&session_id);

        // Add MCP config if available
        if let Some(ref mcp_path) = self.config.mcp_config_path {
            cmd.arg("--mcp-config").arg(mcp_path);
        }

        // Add budget
        let budget = config.max_budget_usd.unwrap_or(self.config.default_budget_usd);
        cmd.arg("--max-budget-usd").arg(budget.to_string());

        // Add system prompt with context
        if let Some(ref snapshot) = config.context_snapshot {
            let prompt = snapshot.to_system_prompt();
            cmd.arg("--append-system-prompt").arg(&prompt);
        } else if let Some(ref sys_prompt) = config.system_prompt {
            cmd.arg("--append-system-prompt").arg(sys_prompt);
        }

        // Add allowed tools
        if let Some(ref tools) = config.allowed_tools {
            cmd.arg("--allowed-tools").arg(tools.join(","));
        }

        // Note: With --input-format stream-json, the prompt is sent via stdin, not CLI arg

        // Set working directory
        cmd.current_dir(&config.project_path);

        // Clear ANTHROPIC_API_KEY so Claude Code uses Max subscription auth instead
        cmd.env_remove("ANTHROPIC_API_KEY");

        // Configure stdio
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn process
        let mut child = cmd.spawn().context("Failed to spawn claude process")?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;

        let spawned_at = chrono::Utc::now().timestamp();

        // Create process handle
        let mut process = ClaudeProcess {
            session_id: session_id.clone(),
            child,
            stdin,
            status: SessionStatus::Starting,
            spawned_at,
            project_path: config.project_path.clone(),
            instruction_id: config.instruction_id.clone(),
        };

        // Inject the initial prompt via stdin (required for --input-format stream-json)
        process.inject_message(&config.initial_prompt).await?;

        // Keep stdin open for interactive sessions (inject_message, answer_question)
        process.status = SessionStatus::Running;

        // Store in database (initial entry)
        self.insert_session(&session_id, &config, spawned_at).await?;

        // Update DB status to running now that process is actually started
        update_session_started(&self.db, &session_id).await?;

        // Store in memory
        self.processes
            .write()
            .await
            .insert(session_id.clone(), process);

        // Broadcast start event
        let _ = self.event_tx.send(SessionEvent::Started {
            session_id: session_id.clone(),
            project_path: config.project_path.clone(),
            initial_prompt: config.initial_prompt.clone(),
        });

        // Broadcast status change to Running (process is now active)
        let _ = self.event_tx.send(SessionEvent::StatusChanged {
            session_id: session_id.clone(),
            status: SessionStatus::Running,
            phase: None,
        });

        // Spawn stdout reader (stream-json parser)
        let (stream_tx, stream_rx) = mpsc::channel(256);
        let parser = StreamParser::new(stream_tx);
        let _reader_handle = parser.spawn_reader(stdout);

        // Spawn stderr reader
        self.spawn_stderr_reader(session_id.clone(), stderr);

        // Spawn event processor
        self.spawn_event_processor(session_id.clone(), config.instruction_id.clone(), stream_rx);

        Ok(session_id)
    }

    /// Spawn background task to process stream events
    fn spawn_event_processor(&self, session_id: String, instruction_id: Option<String>, mut rx: mpsc::Receiver<StreamEvent>) {
        let event_tx = self.event_tx.clone();
        let processes = self.processes.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            debug!(session_id = %session_id, instruction_id = ?instruction_id, "Starting event processor");

            while let Some(event) = rx.recv().await {
                // Check for questions
                if let Some(q) = detect_question(&event) {
                    let question_id = format!("q_{}", uuid::Uuid::new_v4());

                    // Insert question into DB
                    if let Err(e) = insert_question(&db, &question_id, &session_id, &q).await {
                        error!(error = %e, "Failed to insert question");
                    }

                    // Broadcast question event
                    let _ = event_tx.send(SessionEvent::QuestionPending {
                        question_id,
                        session_id: session_id.clone(),
                        question: q.question,
                        options: q.options,
                    });

                    // Update status to paused
                    if let Some(proc) = processes.write().await.get_mut(&session_id) {
                        proc.status = SessionStatus::Paused;
                    }
                    let _ = event_tx.send(SessionEvent::StatusChanged {
                        session_id: session_id.clone(),
                        status: SessionStatus::Paused,
                        phase: None,
                    });
                }

                // Broadcast tool calls
                if let StreamEvent::ToolUse { name, id, input } = &event {
                    let preview = serde_json::to_string(input)
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect();

                    let _ = event_tx.send(SessionEvent::ToolCall {
                        session_id: session_id.clone(),
                        tool_name: name.clone(),
                        tool_id: id.clone(),
                        input_preview: preview,
                    });
                }

                // Broadcast assistant output
                if let StreamEvent::Assistant { message, error, .. } = &event {
                    if let Some(content) = message.content_text() {
                        let chunk_type = if error.is_some() {
                            "error"
                        } else {
                            "assistant"
                        };
                        let _ = event_tx.send(SessionEvent::Output {
                            session_id: session_id.clone(),
                            chunk_type: chunk_type.to_string(),
                            content,
                        });
                    }
                }

                // Handle User events (echoed injected messages or tool results)
                if let StreamEvent::User { message, .. } = &event {
                    // Log the user event for debugging instruction delivery
                    info!(
                        session_id = %session_id,
                        is_text = message.is_text(),
                        summary = %message.summary(),
                        "Received User event from Claude Code"
                    );

                    // If this is a text message (injected instruction), broadcast it
                    if let Some(text) = message.text() {
                        let _ = event_tx.send(SessionEvent::Output {
                            session_id: session_id.clone(),
                            chunk_type: "user".to_string(),
                            content: text.to_string(),
                        });
                    }
                }

                // Handle completion
                if let StreamEvent::Result { data } = &event {
                    // Check if this was an error result
                    let is_error = data.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                    let result_text = data.get("result").and_then(|v| v.as_str()).map(|s| s.to_string());

                    let (status, exit_code) = if is_error {
                        (SessionStatus::Failed, 1)
                    } else {
                        (SessionStatus::Completed, 0)
                    };

                    if let Some(proc) = processes.write().await.get_mut(&session_id) {
                        proc.status = status;
                    }
                    let _ = event_tx.send(SessionEvent::Ended {
                        session_id: session_id.clone(),
                        status,
                        exit_code: Some(exit_code),
                        summary: result_text.clone(),
                    });

                    // Mark instruction based on result
                    if let Some(ref instr_id) = instruction_id {
                        let (db_status, db_error) = if is_error {
                            ("failed", result_text.as_deref())
                        } else {
                            ("completed", None)
                        };
                        if let Err(e) = mark_instruction_completed(&db, instr_id, db_status, db_error).await {
                            error!(instruction_id = %instr_id, error = %e, "Failed to mark instruction {}", db_status);
                        } else {
                            info!(instruction_id = %instr_id, session_id = %session_id, "Marked instruction as {}", db_status);
                        }
                    }
                }

                // Handle errors
                if let StreamEvent::Error { error } = &event {
                    warn!(session_id = %session_id, error = %error.message, "Session error");
                    if let Some(proc) = processes.write().await.get_mut(&session_id) {
                        proc.status = SessionStatus::Failed;
                    }
                    let _ = event_tx.send(SessionEvent::Ended {
                        session_id: session_id.clone(),
                        status: SessionStatus::Failed,
                        exit_code: None,
                        summary: Some(error.message.clone()),
                    });

                    // Mark instruction as failed if this session was executing one
                    if let Some(ref instr_id) = instruction_id {
                        if let Err(e) = mark_instruction_completed(&db, instr_id, "failed", Some(&error.message)).await {
                            error!(instruction_id = %instr_id, error = %e, "Failed to mark instruction failed");
                        } else {
                            info!(instruction_id = %instr_id, session_id = %session_id, "Marked instruction as failed");
                        }
                    }
                }
            }

            debug!(session_id = %session_id, "Event processor finished");
        });
    }

    /// Spawn background task to read stderr and broadcast as output events
    fn spawn_stderr_reader(&self, session_id: String, stderr: ChildStderr) {
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();

            while let Ok(Some(line)) = reader.next_line().await {
                if line.is_empty() {
                    continue;
                }

                debug!(session_id = %session_id, line = %line, "stderr");

                let _ = event_tx.send(SessionEvent::Output {
                    session_id: session_id.clone(),
                    chunk_type: "stderr".to_string(),
                    content: line,
                });
            }

            debug!(session_id = %session_id, "Stderr reader finished");
        });
    }

    /// Inject a message into a running session
    pub async fn inject_message(&self, session_id: &str, message: &str) -> Result<()> {
        let mut processes = self.processes.write().await;
        let proc = processes
            .get_mut(session_id)
            .context("Session not found")?;

        proc.inject_message(message).await?;

        // Update status back to running if paused
        if proc.status == SessionStatus::Paused {
            proc.status = SessionStatus::Running;
            let _ = self.event_tx.send(SessionEvent::StatusChanged {
                session_id: session_id.to_string(),
                status: SessionStatus::Running,
                phase: None,
            });
        }

        Ok(())
    }

    /// Answer a pending question
    pub async fn answer_question(&self, question_id: &str, answer: &str) -> Result<()> {
        // Get session ID from question
        let session_id = get_question_session(&self.db, question_id).await?;

        // Format answer as user message
        let message = format!("User's answer: {}", answer);

        // Inject into session
        self.inject_message(&session_id, &message).await?;

        // Update question status
        mark_question_answered(&self.db, question_id, answer).await?;

        Ok(())
    }

    /// Terminate a session gracefully
    pub async fn terminate(&self, session_id: &str) -> Result<i32> {
        let mut processes = self.processes.write().await;
        let proc = processes
            .get_mut(session_id)
            .context("Session not found")?;

        // Send /quit command
        if let Err(e) = proc.inject_message("/quit").await {
            warn!(error = %e, "Failed to send quit, killing");
            proc.kill().await?;
        }

        // Wait for exit
        let code = proc.wait().await?;

        // Update status
        proc.status = if code == 0 {
            SessionStatus::Completed
        } else {
            SessionStatus::Failed
        };

        // Update database
        update_session_completed(&self.db, session_id, code).await?;

        // Broadcast end event
        let _ = self.event_tx.send(SessionEvent::Ended {
            session_id: session_id.to_string(),
            status: proc.status,
            exit_code: Some(code),
            summary: None,
        });

        // Remove from active processes
        processes.remove(session_id);

        Ok(code)
    }

    /// Get status of all active sessions with details
    pub async fn list_sessions(&self) -> Vec<SessionDetails> {
        self.processes
            .read()
            .await
            .iter()
            .map(|(id, p)| SessionDetails {
                session_id: id.clone(),
                status: p.status,
                project_path: Some(p.project_path.clone()),
                spawned_at: Some(p.spawned_at),
            })
            .collect()
    }

    /// Insert session into database
    async fn insert_session(
        &self,
        session_id: &str,
        config: &SpawnConfig,
        spawned_at: i64,
    ) -> Result<()> {
        let context_json = config
            .context_snapshot
            .as_ref()
            .map(|c| serde_json::to_string(c).ok())
            .flatten();

        sqlx::query(
            r#"
            INSERT INTO claude_sessions (id, status, initial_prompt, context_snapshot, spawned_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(session_id)
        .bind(SessionStatus::Starting.as_str())
        .bind(&config.initial_prompt)
        .bind(context_json)
        .bind(spawned_at)
        .execute(&self.db)
        .await
        .context("Failed to insert session")?;

        Ok(())
    }
}

// ============================================================================
// Database helpers
// ============================================================================

async fn insert_question(
    db: &SqlitePool,
    question_id: &str,
    session_id: &str,
    q: &super::stream::DetectedQuestion,
) -> Result<()> {
    let options_json = q
        .options
        .as_ref()
        .map(|o| serde_json::to_string(o).ok())
        .flatten();

    sqlx::query(
        r#"
        INSERT INTO question_queue (id, session_id, question, options, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(question_id)
    .bind(session_id)
    .bind(&q.question)
    .bind(options_json)
    .bind("pending")
    .bind(chrono::Utc::now().timestamp())
    .execute(db)
    .await
    .context("Failed to insert question")?;

    Ok(())
}

async fn get_question_session(db: &SqlitePool, question_id: &str) -> Result<String> {
    let row: (String,) = sqlx::query_as("SELECT session_id FROM question_queue WHERE id = $1")
        .bind(question_id)
        .fetch_one(db)
        .await
        .context("Question not found")?;
    Ok(row.0)
}

async fn mark_question_answered(db: &SqlitePool, question_id: &str, answer: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE question_queue
        SET status = 'answered', answer = $1, answered_at = $2
        WHERE id = $3
        "#,
    )
    .bind(answer)
    .bind(chrono::Utc::now().timestamp())
    .bind(question_id)
    .execute(db)
    .await
    .context("Failed to update question")?;

    Ok(())
}

/// Update session status to running and record start time
async fn update_session_started(db: &SqlitePool, session_id: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE claude_sessions
        SET status = $1, started_at = $2
        WHERE id = $3
        "#,
    )
    .bind("running")
    .bind(chrono::Utc::now().timestamp())
    .bind(session_id)
    .execute(db)
    .await
    .context("Failed to update session to running")?;

    Ok(())
}

async fn update_session_completed(db: &SqlitePool, session_id: &str, exit_code: i32) -> Result<()> {
    let status = if exit_code == 0 {
        "completed"
    } else {
        "failed"
    };

    sqlx::query(
        r#"
        UPDATE claude_sessions
        SET status = $1, exit_code = $2, completed_at = $3
        WHERE id = $4
        "#,
    )
    .bind(status)
    .bind(exit_code)
    .bind(chrono::Utc::now().timestamp())
    .bind(session_id)
    .execute(db)
    .await
    .context("Failed to update session")?;

    Ok(())
}

/// Mark an instruction as completed or failed
async fn mark_instruction_completed(
    db: &SqlitePool,
    instruction_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    if status == "failed" {
        sqlx::query(
            r#"
            UPDATE instruction_queue
            SET status = $1, completed_at = datetime('now'), error = $2
            WHERE id = $3
            "#,
        )
        .bind(status)
        .bind(error)
        .bind(instruction_id)
        .execute(db)
        .await
        .context("Failed to update instruction")?;
    } else {
        sqlx::query(
            r#"
            UPDATE instruction_queue
            SET status = $1, completed_at = datetime('now')
            WHERE id = $2
            "#,
        )
        .bind(status)
        .bind(instruction_id)
        .execute(db)
        .await
        .context("Failed to update instruction")?;
    }

    Ok(())
}
