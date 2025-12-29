<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  interface BuildError {
    id: number;
    category: string | null;
    severity: string;
    message: string;
    file_path: string | null;
    line_number: number | null;
    resolved: boolean;
    created_at: number;
  }

  interface HealthInfo {
    db_size_bytes: number;
    db_size_human: string;
    recent_errors: BuildError[];
    active_session_id: string | null;
    active_session_status: string | null;
    mcp_session_id: string | null;
  }

  let health = $state<HealthInfo | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  async function fetchHealth() {
    try {
      const res = await fetch('/api/health');
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      health = await res.json();
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to fetch';
    } finally {
      loading = false;
    }
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts * 1000);
    return date.toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function getStatusColor(status: string | null): string {
    switch (status) {
      case 'running': return 'var(--term-success)';
      case 'starting': return 'var(--term-accent)';
      case 'paused': return 'var(--term-warning)';
      case 'failed': return 'var(--term-error)';
      default: return 'var(--term-text-dim)';
    }
  }

  onMount(() => {
    fetchHealth();
    pollInterval = setInterval(fetchHealth, 10000); // Poll every 10s
  });

  onDestroy(() => {
    if (pollInterval) {
      clearInterval(pollInterval);
    }
  });
</script>

<div class="health-monitor">
  <div class="health-header">
    <span class="title">Health Monitor</span>
    <button class="refresh-btn" onclick={fetchHealth} disabled={loading}>
      {loading ? '...' : 'Refresh'}
    </button>
  </div>

  {#if error}
    <div class="error-state">
      <span class="error-icon">!</span>
      <span>{error}</span>
    </div>
  {:else if health}
    <div class="health-grid">
      <!-- Database Size -->
      <div class="stat-card">
        <span class="stat-label">DB Size</span>
        <span class="stat-value">{health.db_size_human}</span>
      </div>

      <!-- Active Session -->
      <div class="stat-card">
        <span class="stat-label">Claude Session</span>
        {#if health.active_session_id}
          <span class="stat-value session-active" style="color: {getStatusColor(health.active_session_status)}">
            {health.active_session_id.slice(0, 8)}...
            <span class="session-status">{health.active_session_status}</span>
          </span>
        {:else}
          <span class="stat-value dim">None</span>
        {/if}
      </div>

      <!-- MCP Session -->
      <div class="stat-card">
        <span class="stat-label">MCP Session</span>
        {#if health.mcp_session_id}
          <span class="stat-value mono">{health.mcp_session_id.slice(0, 12)}...</span>
        {:else}
          <span class="stat-value dim">None</span>
        {/if}
      </div>
    </div>

    <!-- Recent Errors -->
    <div class="errors-section">
      <span class="section-label">Recent Build Errors ({health.recent_errors.length})</span>
      {#if health.recent_errors.length === 0}
        <div class="no-errors">No recent errors</div>
      {:else}
        <div class="error-list">
          {#each health.recent_errors as err (err.id)}
            <div class="error-item" class:resolved={err.resolved}>
              <div class="error-header">
                <span class="error-severity" class:warning={err.severity === 'warning'}>
                  {err.severity === 'warning' ? 'W' : 'E'}
                </span>
                {#if err.category}
                  <span class="error-category">{err.category}</span>
                {/if}
                {#if err.resolved}
                  <span class="resolved-badge">resolved</span>
                {/if}
                <span class="error-time">{formatTimestamp(err.created_at)}</span>
              </div>
              <div class="error-message">{err.message}</div>
              {#if err.file_path}
                <div class="error-location">
                  {err.file_path}{err.line_number ? `:${err.line_number}` : ''}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div class="loading-state">Loading...</div>
  {/if}
</div>

<style>
  .health-monitor {
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    padding: 12px;
    margin-bottom: 12px;
  }

  .health-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-accent);
    letter-spacing: 0.5px;
  }

  .refresh-btn {
    padding: 4px 10px;
    font-size: 11px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .refresh-btn:hover:not(:disabled) {
    border-color: var(--term-accent);
    color: var(--term-accent);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .health-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
    margin-bottom: 12px;
  }

  .stat-card {
    display: flex;
    flex-direction: column;
    padding: 8px 10px;
    background: var(--term-bg-secondary);
    border-radius: 4px;
  }

  .stat-label {
    font-size: 10px;
    text-transform: uppercase;
    color: var(--term-text-dim);
    margin-bottom: 4px;
  }

  .stat-value {
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
  }

  .stat-value.dim {
    color: var(--term-text-dim);
    font-style: italic;
  }

  .stat-value.mono {
    font-family: monospace;
    font-size: 11px;
  }

  .session-active {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .session-status {
    font-size: 10px;
    text-transform: uppercase;
    opacity: 0.8;
  }

  .errors-section {
    border-top: 1px solid var(--term-border);
    padding-top: 10px;
  }

  .section-label {
    display: block;
    font-size: 11px;
    font-weight: 500;
    color: var(--term-text-dim);
    margin-bottom: 8px;
  }

  .no-errors {
    font-size: 12px;
    color: var(--term-success);
    padding: 8px;
    text-align: center;
    background: rgba(0, 200, 100, 0.1);
    border-radius: 4px;
  }

  .error-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 200px;
    overflow-y: auto;
  }

  .error-item {
    padding: 8px;
    background: rgba(255, 80, 80, 0.08);
    border: 1px solid rgba(255, 80, 80, 0.2);
    border-radius: 4px;
  }

  .error-item.resolved {
    opacity: 0.6;
    background: rgba(100, 100, 100, 0.08);
    border-color: var(--term-border);
  }

  .error-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
    font-size: 11px;
  }

  .error-severity {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    font-size: 10px;
    font-weight: bold;
    background: var(--term-error);
    color: white;
    border-radius: 3px;
  }

  .error-severity.warning {
    background: var(--term-warning);
    color: black;
  }

  .error-category {
    font-family: monospace;
    color: var(--term-accent);
  }

  .resolved-badge {
    padding: 1px 5px;
    font-size: 9px;
    text-transform: uppercase;
    background: var(--term-success);
    color: white;
    border-radius: 3px;
  }

  .error-time {
    margin-left: auto;
    color: var(--term-text-dim);
  }

  .error-message {
    font-size: 12px;
    color: var(--term-text);
    line-height: 1.4;
    word-break: break-word;
  }

  .error-location {
    font-size: 10px;
    font-family: monospace;
    color: var(--term-text-dim);
    margin-top: 4px;
  }

  .error-state {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px;
    background: rgba(255, 80, 80, 0.1);
    border-radius: 4px;
    color: var(--term-error);
    font-size: 12px;
  }

  .error-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    font-weight: bold;
    background: var(--term-error);
    color: white;
    border-radius: 50%;
  }

  .loading-state {
    padding: 20px;
    text-align: center;
    color: var(--term-text-dim);
    font-size: 12px;
  }
</style>
