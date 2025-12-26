<script lang="ts">
  import { onMount } from 'svelte';

  // Types for API responses
  interface McpHistoryEntry {
    id: number;
    tool_name: string;
    args_preview: string;
    result_summary: string | null;
    success: boolean;
    duration_ms: number | null;
    created_at: string;
  }

  interface InstructionEntry {
    id: string;
    instruction: string;
    context: string | null;
    priority: string;
    status: string;
    created_at: string;
    completed_at: string | null;
    result: string | null;
    error: string | null;
  }

  // State
  let mcpHistory = $state<McpHistoryEntry[]>([]);
  let instructions = $state<InstructionEntry[]>([]);
  let activeSection = $state<'activity' | 'instructions'>('instructions');
  let loading = $state(false);
  let newInstruction = $state('');
  let newPriority = $state('normal');

  // Fetch functions
  async function fetchMcpHistory() {
    try {
      const res = await fetch('/api/mcp-history?limit=50');
      if (res.ok) {
        mcpHistory = await res.json();
      }
    } catch (e) {
      console.error('Failed to fetch MCP history:', e);
    }
  }

  async function fetchInstructions() {
    try {
      const res = await fetch('/api/instructions?status=all&limit=20');
      if (res.ok) {
        instructions = await res.json();
      }
    } catch (e) {
      console.error('Failed to fetch instructions:', e);
    }
  }

  async function createInstruction() {
    if (!newInstruction.trim()) return;
    loading = true;
    try {
      const res = await fetch('/api/instructions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          instruction: newInstruction,
          priority: newPriority,
        }),
      });
      if (res.ok) {
        newInstruction = '';
        newPriority = 'normal';
        await fetchInstructions();
      }
    } catch (e) {
      console.error('Failed to create instruction:', e);
    } finally {
      loading = false;
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
    fetchInstructions();
    fetchMcpHistory();

    // Poll for updates
    const interval = setInterval(() => {
      if (activeSection === 'instructions') {
        fetchInstructions();
      } else {
        fetchMcpHistory();
      }
    }, 5000);

    return () => clearInterval(interval);
  });
</script>

<div class="orchestration-tab">
  <!-- Section toggle -->
  <div class="section-header">
    <div class="section-toggle">
      <button
        class="toggle-btn {activeSection === 'instructions' ? 'active' : ''}"
        onclick={() => { activeSection = 'instructions'; fetchInstructions(); }}
      >
        Instructions
      </button>
      <button
        class="toggle-btn {activeSection === 'activity' ? 'active' : ''}"
        onclick={() => { activeSection = 'activity'; fetchMcpHistory(); }}
      >
        Claude Activity
      </button>
    </div>
  </div>

  {#if activeSection === 'instructions'}
    <!-- New instruction form -->
    <div class="instruction-form">
      <textarea
        class="instruction-input"
        placeholder="Send instruction to Claude Code..."
        bind:value={newInstruction}
        rows="2"
      ></textarea>
      <div class="form-row">
        <select class="priority-select" bind:value={newPriority}>
          <option value="low">Low</option>
          <option value="normal">Normal</option>
          <option value="high">High</option>
          <option value="urgent">Urgent</option>
        </select>
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
    padding: 8px 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg-secondary);
  }

  .section-toggle {
    display: flex;
    gap: 4px;
  }

  .toggle-btn {
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

  .instruction-form {
    padding: 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg);
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

  .send-btn {
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
</style>
