<script lang="ts">
  /**
   * ToolCallInline - Compact inline tool call display
   *
   * Shows tool calls inline in chat with:
   * - Collapsed: icon + name + summary + status
   * - Expanded: full arguments and output
   */

  import type { ToolCallResult, ToolCategory } from '$lib/api/client';
  import { layoutStore } from '$lib/stores/layout.svelte';
  import DiffView from '../DiffView.svelte';
  import ToolArguments from './ToolArguments.svelte';

  interface Props {
    callId: string;
    name: string;
    arguments: Record<string, unknown>;
    summary?: string;
    category?: ToolCategory;
    result?: ToolCallResult;
    isLoading?: boolean;
  }

  let {
    callId,
    name,
    arguments: args,
    summary,
    category = 'other',
    result,
    isLoading = false,
  }: Props = $props();

  let isExpanded = $state(false);

  // Category colors
  const categoryColors: Record<string, string> = {
    file: 'var(--term-cyan)',
    shell: 'var(--term-yellow)',
    memory: 'var(--term-purple)',
    web: 'var(--term-blue)',
    git: 'var(--term-orange)',
    mira: 'var(--term-accent)',
    other: 'var(--term-text-dim)',
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

  function toggleExpand(e: MouseEvent) {
    // Alt+click opens in Timeline panel
    if (e.altKey) {
      layoutStore.setDrawerTab('timeline');
      return;
    }
    isExpanded = !isExpanded;
  }

  function copyOutput() {
    if (result?.output) {
      navigator.clipboard.writeText(result.output);
    }
  }

  function copyArgs() {
    navigator.clipboard.writeText(JSON.stringify(args, null, 2));
  }

  function copyFull() {
    const full = {
      name,
      arguments: args,
      result: result?.output,
      success: result?.success,
      duration_ms: result?.duration_ms,
    };
    navigator.clipboard.writeText(JSON.stringify(full, null, 2));
  }
</script>

<div
  class="tool-call-inline {isLoading ? 'loading' : ''} {result?.success === false ? 'error' : ''}"
  style="--category-color: {categoryColors[category] || categoryColors.other}"
  data-callid={callId}
  id="tool-call-{callId}"
>
  <!-- Collapsed view (always visible) -->
  <button class="tool-header" onclick={toggleExpand}>
    <!-- Status indicator -->
    <span class="status-indicator">
      {#if isLoading}
        <span class="spinner"></span>
      {:else if result?.success}
        <svg class="status-icon success" viewBox="0 0 16 16" fill="currentColor">
          <path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z"/>
        </svg>
      {:else if result?.success === false}
        <svg class="status-icon error" viewBox="0 0 16 16" fill="currentColor">
          <path d="M3.72 3.72a.75.75 0 011.06 0L8 6.94l3.22-3.22a.75.75 0 111.06 1.06L9.06 8l3.22 3.22a.75.75 0 11-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 01-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 010-1.06z"/>
        </svg>
      {:else}
        <span class="status-dot"></span>
      {/if}
    </span>

    <!-- Tool info -->
    <span class="tool-name">{name}</span>
    <span class="tool-summary">{summary || ''}</span>

    <!-- Duration -->
    {#if result?.duration_ms}
      <span class="tool-duration">{formatDuration(result.duration_ms)}</span>
    {/if}

    <!-- Expand indicator -->
    <span class="expand-indicator {isExpanded ? 'expanded' : ''}">
      <svg viewBox="0 0 16 16" fill="currentColor">
        <path d="M4.427 7.427l3.396 3.396a.25.25 0 00.354 0l3.396-3.396A.25.25 0 0011.396 7H4.604a.25.25 0 00-.177.427z"/>
      </svg>
    </span>
  </button>

  <!-- Expanded view -->
  {#if isExpanded}
    <div class="tool-details">
      <!-- Arguments -->
      {#if Object.keys(args).length > 0}
        <div class="detail-section">
          <div class="detail-header">
            <span class="detail-label">Arguments</span>
            <button class="copy-btn" onclick={copyArgs} title="Copy arguments as JSON">
              <svg viewBox="0 0 16 16" fill="currentColor">
                <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z"/>
                <path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z"/>
              </svg>
            </button>
          </div>
          <ToolArguments {args} />
        </div>
      {/if}

      <!-- Output -->
      {#if result?.output}
        <div class="detail-section">
          <div class="detail-header">
            <span class="detail-label">Output</span>
            {#if result.truncated}
              <span class="truncated-badge">truncated ({formatSize(result.total_bytes)})</span>
            {/if}
            <button class="copy-btn" onclick={copyOutput} title="Copy output">
              <svg viewBox="0 0 16 16" fill="currentColor">
                <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z"/>
                <path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z"/>
              </svg>
            </button>
          </div>
          <pre class="detail-code output">{result.output}</pre>
        </div>
      {/if}

      <!-- Diff (if present) -->
      {#if result?.diff}
        <div class="detail-section">
          <div class="detail-label">Changes</div>
          <DiffView diff={result.diff} />
        </div>
      {/if}

      <!-- Stderr (if present) -->
      {#if result?.stderr}
        <div class="detail-section">
          <div class="detail-label error">Stderr</div>
          <pre class="detail-code stderr">{result.stderr}</pre>
        </div>
      {/if}

      <!-- Exit code -->
      {#if result?.exit_code !== undefined}
        <div class="detail-row">
          <span class="detail-key">Exit code:</span>
          <span class="detail-value {result.exit_code === 0 ? 'success' : 'error'}">
            {result.exit_code}
          </span>
        </div>
      {/if}

      <!-- Copy full button -->
      <div class="detail-footer">
        <button class="copy-full-btn" onclick={copyFull} title="Copy full tool call as JSON">
          <svg viewBox="0 0 16 16" fill="currentColor">
            <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z"/>
            <path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z"/>
          </svg>
          Copy Full
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .tool-call-inline {
    margin: 8px 0;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    border-left: 3px solid var(--category-color);
    overflow: hidden;
  }

  .tool-call-inline.loading {
    border-left-color: var(--term-accent);
  }

  .tool-call-inline.error {
    border-left-color: var(--term-error);
  }

  .tool-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    color: var(--term-text);
  }

  .tool-header:hover {
    background: var(--term-bg);
  }

  .status-indicator {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--term-border);
    border-top-color: var(--term-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .status-icon {
    width: 14px;
    height: 14px;
  }

  .status-icon.success {
    color: var(--term-success);
  }

  .status-icon.error {
    color: var(--term-error);
  }

  .status-dot {
    width: 6px;
    height: 6px;
    background: var(--term-text-dim);
    border-radius: 50%;
  }

  .tool-name {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 500;
    color: var(--term-text);
    flex-shrink: 0;
  }

  .tool-summary {
    font-size: 12px;
    color: var(--term-text-dim);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .tool-duration {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--term-text-dim);
    flex-shrink: 0;
  }

  .expand-indicator {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    color: var(--term-text-dim);
    transition: transform 0.15s ease;
  }

  .expand-indicator.expanded {
    transform: rotate(180deg);
  }

  .expand-indicator svg {
    width: 16px;
    height: 16px;
  }

  .tool-details {
    padding: 12px;
    border-top: 1px solid var(--term-border);
    background: var(--term-bg);
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
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-text-dim);
    margin-bottom: 6px;
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

  .copy-btn {
    margin-left: auto;
    padding: 4px;
    background: transparent;
    border: none;
    color: var(--term-text-dim);
    cursor: pointer;
    border-radius: 4px;
  }

  .copy-btn:hover {
    background: var(--term-bg-secondary);
    color: var(--term-text);
  }

  .copy-btn svg {
    width: 14px;
    height: 14px;
  }

  .detail-code {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--term-bg-secondary);
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

  /* Detail footer with copy full button */
  .detail-footer {
    margin-top: 12px;
    padding-top: 8px;
    border-top: 1px solid var(--term-border);
    display: flex;
    justify-content: flex-end;
  }

  .copy-full-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: transparent;
    border: 1px solid var(--term-border);
    color: var(--term-text-dim);
    font-size: 11px;
    font-family: var(--font-mono);
    cursor: pointer;
    border-radius: 4px;
    transition: all 0.15s ease;
  }

  .copy-full-btn:hover {
    background: var(--term-bg-secondary);
    color: var(--term-text);
    border-color: var(--term-accent);
  }

  .copy-full-btn svg {
    width: 12px;
    height: 12px;
  }

  /* Highlight flash for bidirectional linking */
  .tool-call-inline:global(.highlight-flash) {
    animation: highlight-flash 1.5s ease-out;
  }

  @keyframes highlight-flash {
    0%, 20% {
      background: var(--term-accent-faded);
      border-color: var(--term-accent);
    }
    100% {
      background: var(--term-bg-secondary);
    }
  }
</style>
