<script lang="ts">
  import { toolActivityStore, type ToolCall } from '$lib/stores/toolActivity.svelte';

  interface Props {
    call: ToolCall;
  }

  let { call }: Props = $props();

  let isExpanded = $derived(toolActivityStore.expandedIds.has(call.callId));

  // Category icons
  const categoryIcons: Record<string, string> = {
    file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6',
    shell: 'M4 17l6-6-6-6 M12 19h8',
    memory: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M12 6v6l4 2',
    web: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M2 12h20 M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z',
    git: 'M12 1L1 12l11 11 11-11z',
    mira: 'M12 2L2 7l10 5 10-5-10-5z M2 17l10 5 10-5 M2 12l10 5 10-5',
    other: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z',
  };

  function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  }
</script>

<div class="timeline-card {call.status}" onclick={() => toolActivityStore.toggleExpanded(call.callId)}>
  <div class="card-header">
    <!-- Status indicator -->
    <div class="status-indicator {call.status}">
      {#if call.status === 'running'}
        <div class="spinner"></div>
      {:else if call.status === 'done'}
        <svg class="status-icon success" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M20 6L9 17l-5-5" />
        </svg>
      {:else}
        <svg class="status-icon error" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10" />
          <path d="M15 9l-6 6M9 9l6 6" />
        </svg>
      {/if}
    </div>

    <!-- Tool info -->
    <div class="tool-info">
      <div class="tool-name-row">
        <span class="tool-name">{call.name}</span>
        <span class="category-badge">{call.category}</span>
      </div>
      <p class="tool-summary">{call.summary}</p>
    </div>

    <!-- Duration / Running indicator -->
    <div class="card-meta">
      {#if call.status === 'running'}
        <span class="running-text">running...</span>
      {:else if call.durationMs !== undefined}
        <span class="duration">{formatDuration(call.durationMs)}</span>
      {/if}
    </div>

    <!-- Expand icon -->
    <div class="expand-icon {isExpanded ? 'expanded' : ''}">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </div>
  </div>

  {#if isExpanded}
    <div class="card-details">
      <!-- Actions row -->
      <div class="actions-row">
        <button
          class="action-btn"
          onclick={(e) => { e.stopPropagation(); toolActivityStore.scrollToChat(call.callId); }}
        >
          <svg class="action-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          </svg>
          Jump to chat
        </button>
      </div>

      <!-- Arguments -->
      {#if Object.keys(call.arguments).length > 0}
        <div class="detail-section">
          <h4 class="detail-label">Arguments</h4>
          <pre class="detail-code">{JSON.stringify(call.arguments, null, 2)}</pre>
        </div>
      {/if}

      <!-- Output -->
      {#if call.output}
        <div class="detail-section">
          <div class="detail-header">
            <h4 class="detail-label">Output</h4>
            {#if call.truncated}
              <span class="truncated-badge">
                truncated ({formatSize(call.totalBytes ?? 0)} total)
              </span>
            {/if}
          </div>
          <pre class="detail-code output">{call.output}</pre>
        </div>
      {/if}

      <!-- Stderr (if present) -->
      {#if call.stderr}
        <div class="detail-section">
          <h4 class="detail-label error">Stderr</h4>
          <pre class="detail-code stderr">{call.stderr}</pre>
        </div>
      {/if}

      <!-- Exit code (for shell) -->
      {#if call.exitCode !== undefined}
        <div class="detail-row">
          <span class="detail-key">Exit code:</span>
          <span class="detail-value {call.exitCode === 0 ? 'success' : 'error'}">
            {call.exitCode}
          </span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .timeline-card {
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 8px;
    margin: 8px;
    overflow: hidden;
    cursor: pointer;
    transition: border-color 0.15s ease;
  }

  .timeline-card:hover {
    border-color: var(--term-text-dim);
  }

  .timeline-card.running {
    border-color: var(--term-accent);
  }

  .timeline-card.error {
    border-color: var(--term-error);
  }

  .card-header {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px;
  }

  .status-indicator {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--term-border);
    border-top-color: var(--term-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .status-icon {
    width: 16px;
    height: 16px;
  }

  .status-icon.success {
    color: var(--term-success);
  }

  .status-icon.error {
    color: var(--term-error);
  }

  .tool-info {
    flex: 1;
    min-width: 0;
  }

  .tool-name-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .tool-name {
    font-family: var(--font-mono);
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
  }

  .category-badge {
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--term-bg-secondary);
    color: var(--term-text-dim);
  }

  .tool-summary {
    font-size: 12px;
    color: var(--term-text-dim);
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-meta {
    flex-shrink: 0;
    text-align: right;
  }

  .running-text {
    font-size: 11px;
    color: var(--term-accent);
    font-style: italic;
  }

  .duration {
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--term-text-dim);
  }

  .expand-icon {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    color: var(--term-text-dim);
    transition: transform 0.15s ease;
  }

  .expand-icon.expanded {
    transform: rotate(180deg);
  }

  .expand-icon svg {
    width: 16px;
    height: 16px;
  }

  .card-details {
    border-top: 1px solid var(--term-border);
    padding: 12px;
    background: var(--term-bg-secondary);
  }

  .detail-section {
    margin-bottom: 12px;
  }

  .detail-section:last-child {
    margin-bottom: 0;
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .detail-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-text-dim);
    margin: 0 0 6px 0;
  }

  .detail-label.error {
    color: var(--term-error);
  }

  .truncated-badge {
    font-size: 10px;
    padding: 2px 6px;
    background: var(--term-warning-faded);
    color: var(--term-warning);
    border-radius: 4px;
  }

  .detail-code {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--term-bg);
    padding: 8px;
    border-radius: 4px;
    margin: 0;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 200px;
    overflow-y: auto;
    color: var(--term-text);
  }

  .detail-code.output {
    color: var(--term-text-dim);
  }

  .detail-code.stderr {
    color: var(--term-error);
    background: var(--term-error-faded);
  }

  .detail-row {
    display: flex;
    gap: 8px;
    font-size: 12px;
  }

  .detail-key {
    color: var(--term-text-dim);
  }

  .detail-value {
    font-family: var(--font-mono);
    color: var(--term-text);
  }

  .detail-value.success {
    color: var(--term-success);
  }

  .detail-value.error {
    color: var(--term-error);
  }

  /* Actions row */
  .actions-row {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--term-border);
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text-dim);
    font-size: 11px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .action-btn:hover {
    border-color: var(--term-accent);
    color: var(--term-accent);
  }

  .action-icon {
    width: 14px;
    height: 14px;
  }
</style>
