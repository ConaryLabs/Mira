<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { orchestrationStore, type InstructionEntry, type McpHistoryEntry } from '$lib/stores/orchestration.svelte';
  import { sessionsStore, type SessionInfo, type PendingQuestion } from '$lib/stores/sessions.svelte';

  // State
  let activeSection = $state<'activity' | 'instructions' | 'sessions'>('instructions');
  let newInstruction = $state('');
  let newPriority = $state('normal');
  let newProjectPath = $state('');
  let loading = $state(false);

  // Session spawn form state
  let spawnProjectPath = $state('');
  let spawnPrompt = $state('');
  let spawnBudget = $state(5.0);
  let spawnLoading = $state(false);

  // Question answer state
  let answerText = $state('');
  let answeringQuestion = $state<string | null>(null);

  // Derived from orchestration store
  let instructions = $derived(orchestrationStore.instructionsList);
  let mcpHistory = $derived(orchestrationStore.mcpHistory);
  let connected = $derived(orchestrationStore.connected);
  let activeCount = $derived(orchestrationStore.activeCount);

  // Derived from sessions store
  let sessions = $derived(sessionsStore.sessionsList);
  let pendingQuestions = $derived(sessionsStore.questionsList);
  let sessionsConnected = $derived(sessionsStore.connected);
  let sessionCount = $derived(sessionsStore.activeCount);

  async function createInstruction() {
    if (!newInstruction.trim()) return;
    loading = true;
    try {
      const options = newProjectPath.trim()
        ? { projectPath: newProjectPath.trim() }
        : undefined;
      await orchestrationStore.createInstruction(newInstruction, newPriority, options);
      newInstruction = '';
      newPriority = 'normal';
      // Keep project path for convenience (user likely wants to send multiple instructions to same project)
    } finally {
      loading = false;
    }
  }

  async function spawnSession() {
    if (!spawnProjectPath.trim() || !spawnPrompt.trim()) return;
    spawnLoading = true;
    try {
      const result = await sessionsStore.spawnSession(spawnProjectPath, spawnPrompt, { budgetUsd: spawnBudget });
      if (result) {
        spawnPrompt = '';
      }
    } finally {
      spawnLoading = false;
    }
  }

  async function answerQuestion(questionId: string) {
    if (!answerText.trim()) return;
    answeringQuestion = questionId;
    try {
      await sessionsStore.answerQuestion(questionId, answerText);
      answerText = '';
    } finally {
      answeringQuestion = null;
    }
  }

  async function terminateSession(sessionId: string) {
    await sessionsStore.terminateSession(sessionId);
  }

  function getSessionStatusIcon(status: string): string {
    switch (status) {
      case 'starting': return '🚀';
      case 'running': return '🔄';
      case 'paused': return '⏸️';
      case 'completed': return '✅';
      case 'failed': return '❌';
      default: return '❓';
    }
  }

  function getSessionStatusClass(status: string): string {
    switch (status) {
      case 'running': return 'status-running';
      case 'starting': return 'status-starting';
      case 'paused': return 'status-paused';
      case 'completed': return 'status-completed';
      case 'failed': return 'status-failed';
      default: return '';
    }
  }

  function formatTime(isoString: string): string {
    const date = new Date(isoString);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function getStatusIcon(status: string): string {
    switch (status) {
      case 'pending': return '⏳';
      case 'delivered': return '📬';
      case 'in_progress': return '🔄';
      case 'completed': return '✅';
      case 'failed': return '❌';
      case 'cancelled': return '🚫';
      default: return '❓';
    }
  }

  function getPriorityClass(priority: string): string {
    switch (priority) {
      case 'urgent': return 'priority-urgent';
      case 'high': return 'priority-high';
      case 'normal': return 'priority-normal';
      case 'low': return 'priority-low';
      default: return '';
    }
  }

  onMount(() => {
    orchestrationStore.connect();
    sessionsStore.connect();
  });

  onDestroy(() => {
    orchestrationStore.disconnect();
    sessionsStore.disconnect();
  });
</script>

<div class="orchestration-tab">
  <!-- Section toggle -->
  <div class="section-header">
    <div class="section-toggle">
      <button
        class="toggle-btn {activeSection === 'sessions' ? 'active' : ''}"
        onclick={() => activeSection = 'sessions'}
      >
        Sessions
        {#if sessionCount > 0}
          <span class="badge">{sessionCount}</span>
        {/if}
      </button>
      <button
        class="toggle-btn {activeSection === 'instructions' ? 'active' : ''}"
        onclick={() => activeSection = 'instructions'}
      >
        Instructions
        {#if activeCount > 0}
          <span class="badge">{activeCount}</span>
        {/if}
      </button>
      <button
        class="toggle-btn {activeSection === 'activity' ? 'active' : ''}"
        onclick={() => activeSection = 'activity'}
      >
        Activity
      </button>
    </div>
    <div class="connection-status" class:connected={connected && sessionsConnected}>
      {connected && sessionsConnected ? '●' : '○'}
    </div>
  </div>

  {#if activeSection === 'sessions'}
    <!-- Pending Questions (show first if any) -->
    {#if pendingQuestions.length > 0}
      <div class="questions-section">
        <div class="section-title">Pending Questions</div>
        {#each pendingQuestions as question (question.question_id)}
          <div class="question-card">
            <div class="question-text">{question.question}</div>
            {#if question.options && question.options.length > 0}
              <div class="question-options">
                {#each question.options as opt}
                  <button
                    class="option-btn"
                    onclick={() => sessionsStore.answerQuestion(question.question_id, opt.label)}
                    disabled={answeringQuestion === question.question_id}
                  >
                    {opt.label}
                    {#if opt.description}
                      <span class="option-desc">{opt.description}</span>
                    {/if}
                  </button>
                {/each}
              </div>
            {/if}
            <div class="custom-answer">
              <input
                type="text"
                class="answer-input"
                placeholder="Or type custom answer..."
                bind:value={answerText}
                onkeydown={(e) => {
                  if (e.key === 'Enter') {
                    answerQuestion(question.question_id);
                  }
                }}
              />
              <button
                class="answer-btn"
                onclick={() => answerQuestion(question.question_id)}
                disabled={answeringQuestion === question.question_id || !answerText.trim()}
              >
                Send
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Spawn new session form -->
    <div class="spawn-form">
      <input
        type="text"
        class="spawn-input"
        placeholder="Project path (e.g., /home/user/project)"
        bind:value={spawnProjectPath}
      />
      <textarea
        class="spawn-prompt"
        placeholder="Task for Claude Code..."
        bind:value={spawnPrompt}
        rows="2"
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            spawnSession();
          }
        }}
      ></textarea>
      <div class="form-row">
        <label class="budget-label">
          Budget: $
          <input
            type="number"
            class="budget-input"
            bind:value={spawnBudget}
            min="0.5"
            max="100"
            step="0.5"
          />
        </label>
        <span class="hint">⌘+Enter to spawn</span>
        <button
          class="spawn-btn"
          onclick={spawnSession}
          disabled={spawnLoading || !spawnProjectPath.trim() || !spawnPrompt.trim()}
        >
          {spawnLoading ? 'Spawning...' : 'Spawn Session'}
        </button>
      </div>
    </div>

    <!-- Sessions list -->
    <div class="list-container">
      {#if sessions.length === 0}
        <div class="empty-state">
          <p class="empty-text">No sessions yet</p>
          <p class="empty-hint">Spawn a Claude Code session above</p>
        </div>
      {:else}
        {#each sessions as session (session.session_id)}
          <div class="session-card {getSessionStatusClass(session.status)}">
            <div class="card-header">
              <span class="status-icon">{getSessionStatusIcon(session.status)}</span>
              <span class="session-id">{session.session_id.slice(0, 12)}...</span>
              <span class="session-status">{session.status}</span>
              {#if session.status === 'running' || session.status === 'paused'}
                <button
                  class="terminate-btn"
                  onclick={() => terminateSession(session.session_id)}
                  title="Terminate session"
                >
                  ✕
                </button>
              {/if}
            </div>
            {#if session.project_path}
              <div class="card-body">
                <p class="project-path">{session.project_path}</p>
                {#if session.initial_prompt}
                  <p class="initial-prompt">{session.initial_prompt.slice(0, 100)}{session.initial_prompt.length > 100 ? '...' : ''}</p>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  {:else if activeSection === 'instructions'}
    <!-- New instruction form -->
    <div class="instruction-form">
      <input
        type="text"
        class="project-path-input"
        placeholder="Project path (optional - enables auto-spawn)"
        bind:value={newProjectPath}
      />
      <textarea
        class="instruction-input"
        placeholder="Send instruction to Claude Code..."
        bind:value={newInstruction}
        rows="2"
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            createInstruction();
          }
        }}
      ></textarea>
      <div class="form-row">
        <select class="priority-select" bind:value={newPriority}>
          <option value="low">Low</option>
          <option value="normal">Normal</option>
          <option value="high">High</option>
          <option value="urgent">Urgent</option>
        </select>
        <span class="hint">⌘+Enter to send</span>
        <button
          class="send-btn"
          onclick={createInstruction}
          disabled={loading || !newInstruction.trim()}
        >
          {loading ? 'Sending...' : 'Send'}
        </button>
      </div>
    </div>

    <!-- Instructions list -->
    <div class="list-container">
      {#if instructions.length === 0}
        <div class="empty-state">
          <p class="empty-text">No instructions yet</p>
          <p class="empty-hint">Send an instruction to Claude Code above</p>
        </div>
      {:else}
        {#each instructions as instr (instr.id)}
          <div class="instruction-card {getPriorityClass(instr.priority)}">
            <div class="card-header">
              <span class="status-icon">{getStatusIcon(instr.status)}</span>
              <span class="instruction-id">{instr.id}</span>
              <span class="priority-badge">{instr.priority}</span>
              <span class="time">{formatTime(instr.created_at)}</span>
            </div>
            <div class="card-body">
              <p class="instruction-text">{instr.instruction}</p>
              {#if instr.context}
                <p class="context-text">{instr.context}</p>
              {/if}
              {#if instr.result}
                <p class="result-text">Result: {instr.result}</p>
              {/if}
              {#if instr.error}
                <p class="error-text">Error: {instr.error}</p>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {:else}
    <!-- MCP Activity list -->
    <div class="list-container">
      {#if mcpHistory.length === 0}
        <div class="empty-state">
          <p class="empty-text">No Claude Code activity yet</p>
          <p class="empty-hint">MCP tool calls will appear here</p>
        </div>
      {:else}
        {#each mcpHistory as entry (entry.id)}
          <div class="activity-card {entry.success ? '' : 'failed'}">
            <div class="card-header">
              <span class="tool-name">{entry.tool_name}</span>
              {#if entry.duration_ms}
                <span class="duration">{entry.duration_ms}ms</span>
              {/if}
              <span class="time">{formatTime(entry.created_at)}</span>
            </div>
            {#if entry.result_summary}
              <div class="card-body">
                <p class="summary-text">{entry.result_summary}</p>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .orchestration-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg-secondary);
  }

  .section-toggle {
    display: flex;
    gap: 4px;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .toggle-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .toggle-btn.active {
    background: var(--term-accent-faded);
    color: var(--term-accent);
  }

  .badge {
    padding: 1px 6px;
    font-size: 10px;
    font-weight: 600;
    background: var(--term-accent);
    color: var(--term-bg);
    border-radius: 10px;
  }

  .connection-status {
    font-size: 10px;
    color: var(--term-error);
    transition: color 0.2s ease;
  }

  .connection-status.connected {
    color: var(--term-success);
  }

  .instruction-form {
    padding: 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg);
  }

  .project-path-input {
    width: 100%;
    padding: 8px;
    font-size: 12px;
    font-family: monospace;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
    margin-bottom: 8px;
  }

  .project-path-input:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .instruction-input {
    width: 100%;
    padding: 8px;
    font-size: 13px;
    font-family: inherit;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
    resize: none;
  }

  .instruction-input:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .form-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .priority-select {
    padding: 6px 10px;
    font-size: 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
  }

  .hint {
    font-size: 11px;
    color: var(--term-text-dim);
    opacity: 0.7;
  }

  .send-btn {
    margin-left: auto;
    padding: 6px 16px;
    font-size: 12px;
    font-weight: 500;
    background: var(--term-accent);
    border: none;
    border-radius: 4px;
    color: var(--term-bg);
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .send-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .send-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .list-container {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 200px;
    text-align: center;
    color: var(--term-text-dim);
  }

  .empty-text {
    font-size: 14px;
    font-weight: 500;
    margin: 0 0 4px 0;
  }

  .empty-hint {
    font-size: 12px;
    margin: 0;
    opacity: 0.7;
  }

  .instruction-card,
  .activity-card {
    padding: 10px 12px;
    margin-bottom: 8px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 6px;
  }

  .activity-card.failed {
    border-color: var(--term-error);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--term-text-dim);
  }

  .status-icon {
    font-size: 14px;
  }

  .instruction-id {
    font-family: monospace;
    font-size: 11px;
    opacity: 0.7;
  }

  .priority-badge {
    padding: 2px 6px;
    font-size: 10px;
    text-transform: uppercase;
    background: var(--term-bg-secondary);
    border-radius: 3px;
  }

  .priority-urgent .priority-badge {
    background: var(--term-error);
    color: white;
  }

  .priority-high .priority-badge {
    background: var(--term-warning);
    color: black;
  }

  .tool-name {
    font-weight: 500;
    color: var(--term-accent);
  }

  .duration {
    font-family: monospace;
    font-size: 11px;
  }

  .time {
    margin-left: auto;
    opacity: 0.7;
  }

  .card-body {
    margin-top: 6px;
  }

  .instruction-text {
    margin: 0;
    font-size: 13px;
    color: var(--term-text);
    line-height: 1.4;
  }

  .context-text {
    margin: 4px 0 0 0;
    font-size: 12px;
    color: var(--term-text-dim);
    font-style: italic;
  }

  .summary-text {
    margin: 0;
    font-size: 12px;
    color: var(--term-text-dim);
    line-height: 1.4;
  }

  .result-text {
    margin: 4px 0 0 0;
    font-size: 12px;
    color: var(--term-success);
  }

  .error-text {
    margin: 4px 0 0 0;
    font-size: 12px;
    color: var(--term-error);
  }

  /* Sessions styles */
  .questions-section {
    padding: 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-warning);
    background: rgba(255, 200, 0, 0.1);
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-warning);
    margin-bottom: 8px;
  }

  .question-card {
    padding: 12px;
    background: var(--term-bg);
    border: 1px solid var(--term-warning);
    border-radius: 6px;
    margin-bottom: 8px;
  }

  .question-text {
    font-size: 13px;
    color: var(--term-text);
    margin-bottom: 10px;
    line-height: 1.4;
  }

  .question-options {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }

  .option-btn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 8px 12px;
    font-size: 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .option-btn:hover:not(:disabled) {
    border-color: var(--term-accent);
    background: var(--term-accent-faded);
  }

  .option-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .option-desc {
    font-size: 10px;
    color: var(--term-text-dim);
    margin-top: 2px;
  }

  .custom-answer {
    display: flex;
    gap: 8px;
  }

  .answer-input {
    flex: 1;
    padding: 8px;
    font-size: 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
  }

  .answer-input:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .answer-btn {
    padding: 8px 16px;
    font-size: 12px;
    font-weight: 500;
    background: var(--term-accent);
    border: none;
    border-radius: 4px;
    color: var(--term-bg);
    cursor: pointer;
  }

  .answer-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spawn-form {
    padding: 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg);
  }

  .spawn-input {
    width: 100%;
    padding: 8px;
    font-size: 13px;
    font-family: monospace;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
    margin-bottom: 8px;
  }

  .spawn-input:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .spawn-prompt {
    width: 100%;
    padding: 8px;
    font-size: 13px;
    font-family: inherit;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
    resize: none;
  }

  .spawn-prompt:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .budget-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--term-text-dim);
  }

  .budget-input {
    width: 60px;
    padding: 4px 6px;
    font-size: 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text);
  }

  .spawn-btn {
    margin-left: auto;
    padding: 6px 16px;
    font-size: 12px;
    font-weight: 500;
    background: var(--term-success);
    border: none;
    border-radius: 4px;
    color: white;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .spawn-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .spawn-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .session-card {
    padding: 10px 12px;
    margin-bottom: 8px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 6px;
  }

  .session-card.status-running {
    border-color: var(--term-accent);
  }

  .session-card.status-paused {
    border-color: var(--term-warning);
  }

  .session-card.status-completed {
    border-color: var(--term-success);
    opacity: 0.7;
  }

  .session-card.status-failed {
    border-color: var(--term-error);
  }

  .session-id {
    font-family: monospace;
    font-size: 11px;
    color: var(--term-text-dim);
  }

  .session-status {
    padding: 2px 6px;
    font-size: 10px;
    text-transform: uppercase;
    background: var(--term-bg-secondary);
    border-radius: 3px;
    margin-left: auto;
  }

  .terminate-btn {
    padding: 2px 6px;
    font-size: 12px;
    background: transparent;
    border: 1px solid var(--term-error);
    border-radius: 3px;
    color: var(--term-error);
    cursor: pointer;
    margin-left: 8px;
    opacity: 0.7;
    transition: opacity 0.15s ease;
  }

  .terminate-btn:hover {
    opacity: 1;
    background: var(--term-error);
    color: white;
  }

  .project-path {
    margin: 0;
    font-size: 11px;
    font-family: monospace;
    color: var(--term-text-dim);
  }

  .initial-prompt {
    margin: 4px 0 0 0;
    font-size: 12px;
    color: var(--term-text);
    line-height: 1.4;
  }
</style>
