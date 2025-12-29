<script lang="ts">
  /**
   * LogStream - Real-time terminal-style log viewer for Claude Code sessions
   * Renders ToolCall, Output (including Bash), and status events from SSE stream
   */
  import { onMount, onDestroy } from 'svelte';
  import ToolInvocation from './ToolInvocation.svelte';
  import type { ToolCallResult, ToolCategory } from '$lib/api/client';

  interface LogEntry {
    id: string;
    type: 'tool_call' | 'tool_result' | 'output' | 'status' | 'error' | 'started' | 'ended' | 'heartbeat';
    timestamp: number;
    toolName?: string;
    toolId?: string;
    toolArgs?: Record<string, unknown>;
    toolSummary?: string;
    toolCategory?: ToolCategory;
    toolResult?: ToolCallResult;
    inputPreview?: string;
    content?: string;
    chunkType?: string;
    status?: string;
    phase?: string;
    summary?: string;
    exitCode?: number;
  }

  interface Props {
    sessionId: string;
    apiBase?: string;
    maxEntries?: number;
  }

  let {
    sessionId,
    apiBase = '',
    maxEntries = 500,
  }: Props = $props();

  let entries = $state<LogEntry[]>([]);
  let connected = $state(false);
  let error = $state<string | null>(null);
  let containerEl: HTMLElement;
  let eventSource: EventSource | null = null;
  let autoScroll = $state(true);

  // Auto-scroll to bottom when new entries arrive
  $effect(() => {
    if (entries.length > 0 && containerEl && autoScroll) {
      // Use requestAnimationFrame to ensure DOM has updated
      requestAnimationFrame(() => {
        containerEl.scrollTop = containerEl.scrollHeight;
      });
    }
  });

  function connect() {
    if (eventSource) {
      eventSource.close();
    }

    const url = `${apiBase}/api/sessions/${sessionId}/stream`;
    eventSource = new EventSource(url);

    eventSource.onopen = () => {
      connected = true;
      error = null;
    };

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        const entry = parseEvent(data);
        if (entry) {
          entries = [...entries.slice(-(maxEntries - 1)), entry];
        }
      } catch (e) {
        console.error('Failed to parse SSE event:', e);
      }
    };

    eventSource.onerror = () => {
      connected = false;
      error = 'Connection lost. Reconnecting...';
      // EventSource auto-reconnects
    };
  }

  function parseEvent(data: any): LogEntry | null {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const timestamp = Date.now();

    switch (data.type) {
      case 'init':
        return {
          id,
          type: 'status',
          timestamp,
          status: data.status,
          content: `Session ${data.session_id} - Status: ${data.status}`,
        };

      case 'started':
        return {
          id,
          type: 'started',
          timestamp,
          content: `Session started: ${data.initial_prompt?.slice(0, 100) || 'No prompt'}`,
        };

      case 'status_changed':
        return {
          id,
          type: 'status',
          timestamp,
          status: data.status,
          phase: data.phase,
          content: data.phase ? `${data.status} (${data.phase})` : data.status,
        };

      case 'output':
        return {
          id,
          type: 'output',
          timestamp,
          chunkType: data.chunk_type,
          content: data.content,
        };

      case 'tool_call':
        return {
          id,
          type: 'tool_call',
          timestamp,
          toolName: data.tool_name,
          toolId: data.tool_id,
          toolArgs: data.arguments || {},
          toolSummary: data.summary || data.input_preview,
          toolCategory: data.category,
          inputPreview: data.input_preview,
        };

      case 'tool_result':
        // Update the corresponding tool_call entry with results
        const toolEntry = entries.find(e => e.toolId === data.tool_id && e.type === 'tool_call');
        if (toolEntry) {
          toolEntry.toolResult = {
            success: data.success,
            output: data.output || '',
            duration_ms: data.duration_ms || 0,
            truncated: data.truncated || false,
            total_bytes: data.total_bytes || 0,
            diff: data.diff,
            output_ref: data.output_ref,
            exit_code: data.exit_code,
            stderr: data.stderr,
          };
          // Force reactivity by updating the entries array
          entries = [...entries];
        }
        return null; // Don't create a separate entry for results

      case 'ended':
        return {
          id,
          type: 'ended',
          timestamp,
          status: data.status,
          exitCode: data.exit_code,
          summary: data.summary,
          content: data.summary || `Session ended with status: ${data.status}`,
        };

      case 'heartbeat':
        // Don't show heartbeats in the log
        return null;

      case 'error':
        return {
          id,
          type: 'error',
          timestamp,
          content: data.message || 'Unknown error',
        };

      default:
        // Unknown event type, show raw
        return {
          id,
          type: 'output',
          timestamp,
          content: JSON.stringify(data),
        };
    }
  }

  function handleScroll() {
    if (!containerEl) return;
    const { scrollTop, scrollHeight, clientHeight } = containerEl;
    // If user scrolled up more than 50px from bottom, disable auto-scroll
    autoScroll = scrollHeight - scrollTop - clientHeight < 50;
  }

  function scrollToBottom() {
    autoScroll = true;
    if (containerEl) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  }

  function getEntryIcon(entry: LogEntry): string {
    switch (entry.type) {
      case 'tool_call': return '⚙';
      case 'output':
        if (entry.chunkType === 'stderr') return '⚠';
        if (entry.chunkType === 'bash' || entry.chunkType === 'stdout') return '$';
        return '›';
      case 'status': return '●';
      case 'started': return '▶';
      case 'ended': return '■';
      case 'error': return '✕';
      default: return '·';
    }
  }

  function getEntryClass(entry: LogEntry): string {
    switch (entry.type) {
      case 'tool_call': return 'entry-tool';
      case 'output':
        if (entry.chunkType === 'stderr') return 'entry-stderr';
        if (entry.chunkType === 'bash' || entry.chunkType === 'stdout') return 'entry-bash';
        return 'entry-output';
      case 'status': return 'entry-status';
      case 'started': return 'entry-started';
      case 'ended': return entry.status === 'completed' ? 'entry-success' : 'entry-failed';
      case 'error': return 'entry-error';
      default: return '';
    }
  }

  onMount(() => {
    connect();
  });

  onDestroy(() => {
    if (eventSource) {
      eventSource.close();
    }
  });
</script>

<div class="log-stream">
  <div class="log-header">
    <span class="session-label">Session: {sessionId.slice(0, 12)}...</span>
    <span class="connection-dot" class:connected></span>
    {#if !autoScroll}
      <button class="scroll-btn" onclick={scrollToBottom}>↓ Latest</button>
    {/if}
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div
    bind:this={containerEl}
    class="log-container"
    onscroll={handleScroll}
  >
    {#if entries.length === 0}
      <div class="empty-log">Waiting for events...</div>
    {:else}
      {#each entries as entry (entry.id)}
        {#if entry.type === 'tool_call'}
          <ToolInvocation
            callId={entry.toolId || entry.id}
            name={entry.toolName || 'unknown'}
            arguments={entry.toolArgs || {}}
            summary={entry.toolSummary}
            category={entry.toolCategory}
            result={entry.toolResult}
            isLoading={!entry.toolResult}
            startTime={entry.timestamp}
          />
        {:else}
          <div class="log-entry {getEntryClass(entry)}">
            <span class="entry-icon">{getEntryIcon(entry)}</span>
            <span class="entry-time">{new Date(entry.timestamp).toLocaleTimeString()}</span>
            <span class="entry-content">{entry.content}</span>
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>

<style>
  .log-stream {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    overflow: hidden;
  }

  .log-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--term-bg-secondary);
    border-bottom: 1px solid var(--term-border);
    font-size: 11px;
  }

  .session-label {
    font-family: var(--font-mono);
    color: var(--term-text-dim);
  }

  .connection-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--term-error);
    transition: background 0.2s ease;
  }

  .connection-dot.connected {
    background: var(--term-success);
  }

  .scroll-btn {
    margin-left: auto;
    padding: 2px 8px;
    font-size: 10px;
    background: var(--term-accent);
    border: none;
    border-radius: 3px;
    color: var(--term-bg);
    cursor: pointer;
    opacity: 0.8;
  }

  .scroll-btn:hover {
    opacity: 1;
  }

  .error-banner {
    padding: 6px 12px;
    font-size: 11px;
    background: rgba(255, 100, 100, 0.1);
    border-bottom: 1px solid var(--term-error);
    color: var(--term-error);
  }

  .log-container {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.5;
  }

  .empty-log {
    color: var(--term-text-dim);
    font-style: italic;
    text-align: center;
    padding: 24px;
  }

  .log-entry {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 2px 0;
    color: var(--term-text);
  }

  .entry-icon {
    flex-shrink: 0;
    width: 12px;
    text-align: center;
    color: var(--term-text-dim);
  }

  .entry-time {
    flex-shrink: 0;
    color: var(--term-text-dim);
    opacity: 0.6;
  }

  .entry-content {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .tool-name {
    font-weight: 600;
    color: var(--term-accent);
  }

  .tool-preview {
    color: var(--term-text-dim);
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* Entry type styles */
  .entry-tool .entry-icon {
    color: var(--term-accent);
  }

  .entry-bash .entry-icon {
    color: var(--term-success);
  }

  .entry-stderr .entry-icon,
  .entry-stderr .entry-content {
    color: var(--term-warning);
  }

  .entry-status .entry-icon {
    color: var(--term-info, #60a5fa);
  }

  .entry-started .entry-icon {
    color: var(--term-success);
  }

  .entry-success .entry-icon,
  .entry-success .entry-content {
    color: var(--term-success);
  }

  .entry-failed .entry-icon,
  .entry-failed .entry-content {
    color: var(--term-error);
  }

  .entry-error .entry-icon,
  .entry-error .entry-content {
    color: var(--term-error);
  }
</style>
