//! Process management for spawned Claude Code sessions
//!
//! Handles spawning, lifecycle, and I/O for Claude Code processes.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use sqlx::SqlitePool;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

use super::stream::{detect_question, StreamParser};
use super::types::{SessionEvent, SessionStatus, SpawnConfig, SpawnerConfig, StreamEvent};

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
    /// Unix timestamp when spawned
    pub spawned_at: i64,
    /// Project path
    pub project_path: String,
}

impl ClaudeProcess {
    /// Inject a user message into the session via stdin
    pub async fn inject_message(&mut self, message: &str) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .context("Process stdin not available")?;

        let msg = serde_json::json!({
            "type": "user",
            "content": message
        });

        let line = format!("{}\n", serde_json::to_string(&msg)?);
        stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write to stdin")?;
        stdin.flush().await.context("Failed to flush stdin")?;

        debug!(session_id = %self.session_id, "Injected message into session");
        Ok(())
    }

    /// Check if process is still running
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

        // Generate session ID
        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| format!("cc_{}", uuid::Uuid::new_v4()));

        info!(session_id = %session_id, project = %config.project_path, "Spawning Claude Code session");

        // Build command
        let mut cmd = Command::new(&self.config.claude_binary);
        cmd.arg("--print")
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

        // Add initial prompt
        cmd.arg(&config.initial_prompt);

        // Set working directory
        cmd.current_dir(&config.project_path);

        // Configure stdio
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn process
        let mut child = cmd.spawn().context("Failed to spawn claude process")?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let _stderr = child.stderr.take(); // TODO: handle stderr

        let spawned_at = chrono::Utc::now().timestamp();

        // Create process handle
        let process = ClaudeProcess {
            session_id: session_id.clone(),
            child,
            stdin,
            status: SessionStatus::Starting,
            spawned_at,
            project_path: config.project_path.clone(),
        };

        // Store in database
        self.insert_session(&session_id, &config, spawned_at).await?;

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

        // Spawn output reader
        let (stream_tx, stream_rx) = mpsc::channel(256);
        let parser = StreamParser::new(stream_tx);
        let _reader_handle = parser.spawn_reader(stdout);

        // Spawn event processor
        self.spawn_event_processor(session_id.clone(), stream_rx);

        Ok(session_id)
    }

    /// Spawn background task to process stream events
    fn spawn_event_processor(&self, session_id: String, mut rx: mpsc::Receiver<StreamEvent>) {
        let event_tx = self.event_tx.clone();
        let processes = self.processes.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            debug!(session_id = %session_id, "Starting event processor");

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
                if let StreamEvent::Assistant { message } = &event {
                    if let Some(content) = &message.content {
                        let _ = event_tx.send(SessionEvent::Output {
                            session_id: session_id.clone(),
                            chunk_type: "assistant".to_string(),
                            content: content.clone(),
                        });
                    }
                }

                // Handle completion
                if let StreamEvent::Result { .. } = &event {
                    if let Some(proc) = processes.write().await.get_mut(&session_id) {
                        proc.status = SessionStatus::Completed;
                    }
                    let _ = event_tx.send(SessionEvent::Ended {
                        session_id: session_id.clone(),
                        status: SessionStatus::Completed,
                        exit_code: Some(0),
                        summary: None,
                    });
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
                }
            }

            debug!(session_id = %session_id, "Event processor finished");
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

    /// Get status of all active sessions
    pub async fn list_sessions(&self) -> Vec<(String, SessionStatus)> {
        self.processes
            .read()
            .await
            .iter()
            .map(|(id, p)| (id.clone(), p.status))
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
