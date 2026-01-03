<script lang="ts">
  import { slide } from 'svelte/transition';

  const API_BASE = 'http://localhost:3000';

  interface SessionInfo {
    id: string;
    project_id: number;
    status: string;
    summary: string | null;
    started_at: string;
    last_activity: string;
    tool_count: number;
    top_tools: string[];
  }

  interface ToolHistoryEntry {
    id: number;
    session_id: string;
    tool_name: string;
    arguments: string | null;
    result_summary: string | null;
    success: boolean;
    created_at: string;
  }

  // Props
  let {
    session,
    isFirst = false,
    isLast = false,
    isExpanded = false,
    onToggle,
    formatDate,
    formatTime
  }: {
    session: SessionInfo;
    isFirst?: boolean;
    isLast?: boolean;
    isExpanded?: boolean;
    onToggle: () => void;
    formatDate: (d: string) => string;
    formatTime: (d: string) => string;
  } = $props();

  // State
  let toolHistory = $state<ToolHistoryEntry[]>([]);
  let loadingHistory = $state(false);
  let exporting = $state(false);

  // Load tool history when expanded
  $effect(() => {
    if (isExpanded && toolHistory.length === 0 && !loadingHistory) {
      loadHistory();
    }
  });

  async function loadHistory() {
    loadingHistory = true;
    try {
      const res = await fetch(`${API_BASE}/api/sessions/${session.id}/history`);
      if (res.ok) {
        const data = await res.json();
        toolHistory = data.data || [];
      }
    } catch (e) {
      console.error('Failed to load history:', e);
    }
    loadingHistory = false;
  }

  async function exportSession() {
    exporting = true;
    try {
      const res = await fetch(`${API_BASE}/api/sessions/${session.id}/export`);
      if (res.ok) {
        const data = await res.json();
        const blob = new Blob([JSON.stringify(data.data, null, 2)], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `session-${session.id.slice(0, 8)}.json`;
        a.click();
        URL.revokeObjectURL(url);
      }
    } catch (e) {
      console.error('Failed to export:', e);
    }
    exporting = false;
  }

  // Tool abbreviations
  const toolAbbrev: Record<string, string> = {
    'remember': 'MEM',
    'recall': 'RCL',
    'semantic_code_search': 'SCS',
    'get_symbols': 'SYM',
    'session_start': 'SES',
    'set_project': 'PRJ',
    'index': 'IDX',
    'goal': 'GOL',
    'task': 'TSK',
    'summarize_codebase': 'SUM',
  };

  function getToolIcon(name: string): string {
    return toolAbbrev[name] || name.slice(0, 3).toUpperCase();
  }

  function truncate(str: string | null, len: number): string {
    if (!str) return '';
    return str.length > len ? str.slice(0, len) + '...' : str;
  }

  function parseArgs(argsStr: string | null): Record<string, any> | null {
    if (!argsStr) return null;
    try {
      return JSON.parse(argsStr);
    } catch {
      return null;
    }
  }
</script>

<div class="session-entry" class:expanded={isExpanded}>
  <!-- Timeline connector -->
  <div class="timeline-track">
    <div class="track-line" class:first={isFirst} class:last={isLast}></div>
    <div class="track-node" class:active={session.status === 'active'}></div>
  </div>

  <!-- Session card -->
  <div class="session-card">
    <button class="session-header" onclick={onToggle}>
      <div class="header-main">
        <div class="session-meta">
          <span class="session-time">{formatTime(session.started_at)}</span>
          <span class="session-date">{formatDate(session.started_at)}</span>
        </div>

        <div class="session-id">
          <span class="id-label">ID</span>
          <code class="id-value">{session.id.slice(0, 8)}</code>
        </div>
      </div>

      <div class="header-stats">
        {#if session.tool_count > 0}
          <div class="tool-badge">
            <span class="badge-count">{session.tool_count}</span>
            <span class="badge-label">tools</span>
          </div>
        {/if}

        {#if session.top_tools.length > 0}
          <div class="top-tools">
            {#each session.top_tools.slice(0, 3) as tool}
              <span class="tool-chip" title={tool}>
                {getToolIcon(tool)}
              </span>
            {/each}
          </div>
        {/if}

        <svg
          class="expand-icon"
          class:rotated={isExpanded}
          width="16"
          height="16"
          viewBox="0 0 16 16"
          fill="currentColor"
        >
          <path d="M4 6l4 4 4-4"/>
        </svg>
      </div>
    </button>

    {#if session.summary}
      <div class="session-summary">
        <span>{session.summary}</span>
      </div>
    {/if}

    <!-- Expanded content -->
    {#if isExpanded}
      <div class="session-details" transition:slide={{ duration: 200 }}>
        <div class="details-header">
          <span class="details-title">Tool History</span>
          <button
            class="export-btn"
            onclick={exportSession}
            disabled={exporting}
          >
            {#if exporting}
              <div class="mini-spinner"></div>
            {:else}
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
                <path d="M8 2v8M4 6l4 4 4-4M2 12v2h12v-2"/>
              </svg>
            {/if}
            <span>Export</span>
          </button>
        </div>

        {#if loadingHistory}
          <div class="loading-history">
            <div class="mini-spinner"></div>
            <span>Loading history...</span>
          </div>
        {:else if toolHistory.length === 0}
          <div class="empty-history">
            <span>No tool calls recorded</span>
          </div>
        {:else}
          <div class="tool-history">
            {#each toolHistory as entry (entry.id)}
              <div class="tool-entry" class:failed={!entry.success}>
                <div class="tool-header">
                  <span class="tool-icon">{getToolIcon(entry.tool_name)}</span>
                  <span class="tool-name">{entry.tool_name}</span>
                  <span class="tool-time">{formatTime(entry.created_at)}</span>
                  <span class="tool-status" class:success={entry.success}>
                    {entry.success ? '✓' : '✗'}
                  </span>
                </div>

                {#if entry.arguments}
                  {@const args = parseArgs(entry.arguments)}
                  {#if args}
                    <div class="tool-args">
                      {#each Object.entries(args) as [key, value]}
                        <div class="arg-row">
                          <span class="arg-key">{key}:</span>
                          <span class="arg-value">{truncate(String(value), 80)}</span>
                        </div>
                      {/each}
                    </div>
                  {/if}
                {/if}

                {#if entry.result_summary}
                  <div class="tool-result">
                    <pre>{entry.result_summary}</pre>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .session-entry {
    display: flex;
    gap: 1.25rem;
    margin-bottom: 1rem;
  }

  /* Timeline Track */
  .timeline-track {
    position: relative;
    width: 20px;
    flex-shrink: 0;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 1.125rem;
  }

  .track-line {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 2px;
    background: linear-gradient(180deg, var(--surface1) 0%, var(--surface0) 100%);
    transform: translateX(-50%);
  }

  .track-line.first {
    top: 1.125rem;
  }

  .track-line.last {
    bottom: calc(100% - 1.125rem - 10px);
  }

  .track-node {
    position: relative;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--surface1);
    border: 2px solid var(--base);
    z-index: 1;
    transition: all 0.2s;
  }

  .track-node.active {
    background: var(--green);
    box-shadow: 0 0 8px var(--green);
  }

  .session-entry:hover .track-node {
    background: var(--blue);
  }

  .session-entry.expanded .track-node {
    background: var(--mauve);
    box-shadow: 0 0 10px var(--mauve);
  }

  /* Session Card */
  .session-card {
    flex: 1;
    background: var(--surface0);
    border: 1px solid var(--glass-border);
    border-radius: 12px;
    overflow: hidden;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .session-entry:hover .session-card {
    border-color: rgba(137, 180, 250, 0.2);
  }

  .session-entry.expanded .session-card {
    border-color: rgba(203, 166, 247, 0.3);
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.2);
  }

  .session-header {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.875rem 1rem;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 0.15s;
  }

  .session-header:hover {
    background: var(--surface1);
  }

  .header-main {
    display: flex;
    align-items: center;
    gap: 1.5rem;
  }

  .session-meta {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  .session-time {
    font-family: var(--font-mono);
    font-size: 0.9375rem;
    font-weight: 600;
    color: var(--foreground);
  }

  .session-date {
    font-size: 0.75rem;
    color: var(--overlay0);
  }

  .session-id {
    display: flex;
    align-items: center;
    gap: 0.375rem;
  }

  .id-label {
    font-size: 0.625rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--overlay0);
  }

  .id-value {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--subtext0);
    padding: 0.125rem 0.375rem;
    background: var(--mantle);
    border-radius: 4px;
  }

  .header-stats {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .tool-badge {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.25rem 0.625rem;
    background: var(--accent-faded);
    border-radius: 6px;
  }

  .badge-count {
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--blue);
  }

  .badge-label {
    font-size: 0.6875rem;
    color: var(--blue);
    opacity: 0.8;
  }

  .top-tools {
    display: flex;
    gap: 0.25rem;
  }

  .tool-chip {
    font-family: var(--font-mono);
    font-size: 0.5625rem;
    font-weight: 600;
    padding: 0.125rem 0.25rem;
    background: var(--surface1);
    border-radius: 3px;
    color: var(--subtext0);
  }

  .expand-icon {
    color: var(--overlay0);
    transition: transform 0.2s ease;
  }

  .expand-icon.rotated {
    transform: rotate(180deg);
  }

  .session-summary {
    padding: 0 1rem 0.75rem;
    font-size: 0.8125rem;
    color: var(--subtext0);
    border-top: 1px solid var(--glass-border);
    margin-top: -0.25rem;
    padding-top: 0.75rem;
  }

  /* Expanded Details */
  .session-details {
    border-top: 1px solid var(--glass-border);
    background: var(--mantle);
  }

  .details-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--glass-border);
  }

  .details-title {
    font-size: 0.6875rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--mauve);
  }

  .export-btn {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--subtext0);
    background: var(--surface0);
    border: 1px solid var(--glass-border);
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .export-btn:hover:not(:disabled) {
    background: var(--surface1);
    color: var(--foreground);
    border-color: var(--blue);
  }

  .export-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .mini-spinner {
    width: 12px;
    height: 12px;
    border: 1.5px solid var(--surface1);
    border-top-color: var(--mauve);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .loading-history,
  .empty-history {
    padding: 2rem;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    color: var(--overlay0);
    font-size: 0.8125rem;
  }

  /* Tool History */
  .tool-history {
    padding: 0.5rem;
  }

  .tool-entry {
    padding: 0.75rem;
    margin-bottom: 0.375rem;
    background: var(--surface0);
    border-radius: 8px;
    border: 1px solid transparent;
    transition: all 0.15s;
  }

  .tool-entry:hover {
    border-color: var(--glass-border);
  }

  .tool-entry.failed {
    border-left: 3px solid var(--red);
  }

  .tool-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .tool-icon {
    font-family: var(--font-mono);
    font-size: 0.5625rem;
    font-weight: 600;
    padding: 0.125rem 0.25rem;
    background: var(--surface1);
    border-radius: 3px;
    color: var(--blue);
  }

  .tool-name {
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    font-weight: 500;
    color: var(--foreground);
  }

  .tool-time {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--overlay0);
  }

  .tool-status {
    font-size: 0.75rem;
    color: var(--green);
  }

  .tool-status:not(.success) {
    color: var(--red);
  }

  .tool-args {
    margin-top: 0.5rem;
    padding: 0.5rem;
    background: var(--mantle);
    border-radius: 6px;
    font-size: 0.75rem;
  }

  .arg-row {
    display: flex;
    gap: 0.5rem;
    padding: 0.125rem 0;
  }

  .arg-key {
    font-family: var(--font-mono);
    color: var(--blue);
    flex-shrink: 0;
  }

  .arg-value {
    font-family: var(--font-mono);
    color: var(--subtext0);
    word-break: break-all;
  }

  .tool-result {
    margin-top: 0.5rem;
    padding: 0.5rem;
    background: var(--crust);
    border-radius: 6px;
    border: 1px solid var(--glass-border);
  }

  .tool-result pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--subtext0);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 200px;
    overflow-y: auto;
  }
</style>
