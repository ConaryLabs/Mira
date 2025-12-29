<script lang="ts">
  /**
   * ToolInvocation - Minimalist collapsible badge for tool calls
   * Shows: Tool Name, Spinner/Status, Duration
   * Expands to: Syntax-highlighted arguments and results
   */
  import type { ToolCallResult, ToolCategory } from '$lib/api/client';
  import { highlight } from '$lib/utils/highlight';

  interface Props {
    callId: string;
    name: string;
    arguments: Record<string, unknown>;
    summary?: string;
    category?: ToolCategory;
    result?: ToolCallResult;
    isLoading?: boolean;
    startTime?: number;
  }

  let {
    callId,
    name,
    arguments: args,
    summary,
    category,
    result,
    isLoading = false,
    startTime,
  }: Props = $props();

  let expanded = $state(false);
  let elapsedMs = $state(0);
  let intervalId: ReturnType<typeof setInterval> | null = null;
  let copiedArgs = $state(false);
  let copiedOutput = $state(false);

  // Track elapsed time while loading
  $effect(() => {
    if (isLoading && startTime) {
      elapsedMs = Date.now() - startTime;
      intervalId = setInterval(() => {
        elapsedMs = Date.now() - startTime;
      }, 100);
    } else if (intervalId) {
      clearInterval(intervalId);
      intervalId = null;
    }

    return () => {
      if (intervalId) clearInterval(intervalId);
    };
  });

  function getCategoryIcon(cat?: ToolCategory): string {
    const icons: Record<ToolCategory, string> = {
      file: '📄',
      shell: '💻',
      memory: '🧠',
      web: '🌐',
      git: '',
      mira: '✨',
      other: '⚙',
    };
    return cat ? icons[cat] : '⚙';
  }

  function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    return `${(ms / 60000).toFixed(1)}m`;
  }

  function getDisplayDuration(): string {
    if (result?.duration_ms) {
      return formatDuration(result.duration_ms);
    }
    if (isLoading && elapsedMs > 0) {
      return formatDuration(elapsedMs);
    }
    return '';
  }

  function formatJson(obj: unknown): string {
    try {
      return JSON.stringify(obj, null, 2);
    } catch {
      return String(obj);
    }
  }

  function truncateOutput(output: string, maxLen = 2000): { text: string; truncated: boolean } {
    if (output.length <= maxLen) {
      return { text: output, truncated: false };
    }
    return { text: output.slice(0, maxLen), truncated: true };
  }

  function getStatusClass(): string {
    if (isLoading) return 'status-loading';
    if (!result) return 'status-pending';
    return result.success ? 'status-success' : 'status-error';
  }

  async function copyArgs() {
    try {
      await navigator.clipboard.writeText(formatJson(args));
      copiedArgs = true;
      setTimeout(() => { copiedArgs = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }

  async function copyOutput() {
    if (!result?.output) return;
    try {
      await navigator.clipboard.writeText(result.output);
      copiedOutput = true;
      setTimeout(() => { copiedOutput = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }
</script>

<div class="tool-invocation" class:expanded>
  <!-- Minimalist Badge Header -->
  <button
    class="badge"
    onclick={() => expanded = !expanded}
    aria-expanded={expanded}
    aria-controls="tool-details-{callId}"
  >
    <span class="badge-icon">{getCategoryIcon(category)}</span>
    <span class="badge-name">{name}</span>

    {#if summary && !expanded}
      <span class="badge-summary">{summary}</span>
    {/if}

    <span class="badge-status {getStatusClass()}">
      {#if isLoading}
        <span class="spinner"></span>
      {:else if result}
        <span class="status-dot"></span>
      {/if}
    </span>

    {#if getDisplayDuration()}
      <span class="badge-duration">{getDisplayDuration()}</span>
    {/if}

    <span class="badge-chevron">{expanded ? '−' : '+'}</span>
  </button>

  <!-- Expandable Details -->
  {#if expanded}
    <div id="tool-details-{callId}" class="details">
      <!-- Arguments Section -->
      <div class="details-section">
        <div class="section-header">
          Arguments
          <button class="copy-btn" onclick={copyArgs} title="Copy arguments">
            {copiedArgs ? 'Copied!' : 'Copy'}
          </button>
        </div>
        <pre class="code-block"><code>{@html highlight(formatJson(args))}</code></pre>
      </div>

      <!-- Result Section -->
      {#if result}
        <div class="details-section">
          <div class="section-header">
            Result
            {#if result.truncated}
              <span class="truncated-badge">truncated ({result.total_bytes} bytes)</span>
            {/if}
            {#if result.exit_code !== undefined}
              <span class="exit-code" class:error={result.exit_code !== 0}>
                exit {result.exit_code}
              </span>
            {/if}
            {#if result.output && !result.diff}
              <button class="copy-btn" onclick={copyOutput} title="Copy output">
                {copiedOutput ? 'Copied!' : 'Copy'}
              </button>
            {/if}
          </div>

          {#if result.diff}
            <!-- Diff output -->
            <div class="diff-preview">
              <div class="diff-path">{result.diff.path}</div>
              {#if result.diff.is_new_file}
                <span class="new-file-badge">new file</span>
              {/if}
            </div>
          {:else}
            {@const outputInfo = truncateOutput(result.output)}
            <pre class="code-block" class:error={!result.success}><code>{@html highlight(outputInfo.text)}{#if outputInfo.truncated || result.truncated}
... (output truncated){/if}</code></pre>
          {/if}

          {#if result.stderr}
            <div class="section-header stderr-header">Stderr</div>
            <pre class="code-block stderr"><code>{@html highlight(result.stderr)}</code></pre>
          {/if}
        </div>
      {:else if isLoading}
        <div class="details-section">
          <div class="loading-placeholder">
            <span class="spinner large"></span>
            <span>Executing...</span>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tool-invocation {
    margin: 4px 0;
    border-radius: 6px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    overflow: hidden;
    transition: border-color 0.15s ease;
  }

  .tool-invocation:hover {
    border-color: var(--term-accent);
  }

  .tool-invocation.expanded {
    border-color: var(--term-accent);
  }

  /* Badge Header */
  .badge {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--term-text);
    text-align: left;
    transition: background 0.1s ease;
  }

  .badge:hover {
    background: rgba(255, 255, 255, 0.03);
  }

  .badge-icon {
    flex-shrink: 0;
    font-size: 14px;
  }

  .badge-name {
    flex-shrink: 0;
    font-weight: 600;
    color: var(--term-accent);
  }

  .badge-summary {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--term-text-dim);
    font-size: 11px;
  }

  .badge-status {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .status-loading .spinner {
    width: 12px;
    height: 12px;
  }

  .status-success .status-dot {
    background: var(--term-success);
    box-shadow: 0 0 4px var(--term-success);
  }

  .status-error .status-dot {
    background: var(--term-error);
    box-shadow: 0 0 4px var(--term-error);
  }

  .status-pending .status-dot {
    background: var(--term-text-dim);
  }

  .badge-duration {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--term-text-dim);
    padding: 2px 6px;
    background: var(--term-bg);
    border-radius: 4px;
  }

  .badge-chevron {
    flex-shrink: 0;
    width: 16px;
    text-align: center;
    color: var(--term-text-dim);
    font-size: 14px;
  }

  /* Spinner Animation */
  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid var(--term-border);
    border-top-color: var(--term-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .spinner.large {
    width: 16px;
    height: 16px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Expandable Details */
  .details {
    border-top: 1px solid var(--term-border);
    padding: 12px;
    background: var(--term-bg);
  }

  .details-section {
    margin-bottom: 12px;
  }

  .details-section:last-child {
    margin-bottom: 0;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--term-text-dim);
    margin-bottom: 6px;
  }

  .stderr-header {
    color: var(--term-warning);
    margin-top: 8px;
  }

  .truncated-badge {
    font-size: 9px;
    font-weight: normal;
    text-transform: none;
    letter-spacing: normal;
    padding: 1px 6px;
    background: var(--term-warning);
    color: var(--term-bg);
    border-radius: 3px;
  }

  .exit-code {
    font-size: 9px;
    font-weight: normal;
    text-transform: none;
    letter-spacing: normal;
    padding: 1px 6px;
    background: var(--term-success);
    color: var(--term-bg);
    border-radius: 3px;
  }

  .exit-code.error {
    background: var(--term-error);
  }

  /* Code Block */
  .code-block {
    margin: 0;
    padding: 10px 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.5;
    color: var(--term-text);
    overflow-x: auto;
    max-height: 300px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .code-block code {
    background: none;
    padding: 0;
    color: inherit;
  }

  .code-block.error {
    border-color: var(--term-error);
    color: var(--term-error);
  }

  .code-block.stderr {
    border-color: var(--term-warning);
    color: var(--term-warning);
  }

  /* Diff Preview */
  .diff-preview {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 4px;
  }

  .diff-path {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--term-accent);
  }

  .new-file-badge {
    font-size: 9px;
    padding: 1px 6px;
    background: var(--term-success);
    color: var(--term-bg);
    border-radius: 3px;
  }

  /* Loading Placeholder */
  .loading-placeholder {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 16px;
    color: var(--term-text-dim);
    font-size: 11px;
  }

  /* Copy Button */
  .copy-btn {
    margin-left: auto;
    padding: 2px 8px;
    background: transparent;
    border: 1px solid var(--term-border);
    border-radius: 3px;
    color: var(--term-text-dim);
    font-size: 9px;
    font-family: var(--font-mono);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .copy-btn:hover {
    background: var(--term-bg-secondary);
    color: var(--term-text);
    border-color: var(--term-accent);
  }

  /* Prism Syntax Highlighting */
  .code-block :global(.token.comment),
  .code-block :global(.token.prolog),
  .code-block :global(.token.doctype),
  .code-block :global(.token.cdata) {
    color: var(--prism-comment, var(--term-text-dim));
    font-style: italic;
  }

  .code-block :global(.token.punctuation) {
    color: var(--term-text);
  }

  .code-block :global(.token.property),
  .code-block :global(.token.tag),
  .code-block :global(.token.boolean),
  .code-block :global(.token.number),
  .code-block :global(.token.constant),
  .code-block :global(.token.symbol),
  .code-block :global(.token.deleted) {
    color: var(--prism-number);
  }

  .code-block :global(.token.selector),
  .code-block :global(.token.attr-name),
  .code-block :global(.token.string),
  .code-block :global(.token.char),
  .code-block :global(.token.builtin),
  .code-block :global(.token.inserted) {
    color: var(--prism-string);
  }

  .code-block :global(.token.operator),
  .code-block :global(.token.entity),
  .code-block :global(.token.url) {
    color: var(--prism-operator);
  }

  .code-block :global(.token.atrule),
  .code-block :global(.token.attr-value),
  .code-block :global(.token.keyword) {
    color: var(--prism-keyword);
  }

  .code-block :global(.token.function),
  .code-block :global(.token.class-name) {
    color: var(--prism-function);
  }

  .code-block :global(.token.regex),
  .code-block :global(.token.important),
  .code-block :global(.token.variable) {
    color: var(--prism-variable);
  }

  .code-block :global(.token.bold) {
    font-weight: bold;
  }

  .code-block :global(.token.italic) {
    font-style: italic;
  }

  .code-block :global(.token.macro),
  .code-block :global(.token.attribute) {
    color: var(--prism-attribute);
  }
</style>
