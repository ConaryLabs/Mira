<script lang="ts">
  import { toolActivityStore, type ToolCall } from '$lib/stores/toolActivity.svelte';
  import type { UsageInfo } from '$lib/api/client';

  interface Props {
    usage?: UsageInfo | null;
    hasBlocks?: boolean;
  }

  let { usage = null, hasBlocks = false }: Props = $props();

  // Get currently running tools
  const runningTools = $derived(
    Array.from(toolActivityStore.calls.values())
      .filter(t => t.status === 'running')
      .slice(0, 3) // Show max 3 running tools
  );

  function formatTokens(n: number): string {
    if (n >= 1000) {
      return (n / 1000).toFixed(1) + 'k';
    }
    return n.toString();
  }

  // Derive current activity state
  const activityState = $derived.by(() => {
    if (runningTools.length > 0) {
      return 'running';
    }
    if (usage && usage.reasoning_tokens > 0 && !hasBlocks) {
      return 'thinking';
    }
    if (hasBlocks) {
      return 'streaming';
    }
    return 'connecting';
  });

  // Get short tool name
  function shortToolName(name: string): string {
    // Remove mcp__ prefix if present
    const stripped = name.replace(/^mcp__\w+__/, '');
    // Truncate long names
    return stripped.length > 20 ? stripped.slice(0, 20) + '...' : stripped;
  }
</script>

<div class="streaming-status">
  {#if activityState === 'connecting'}
    <span class="status-indicator connecting">
      <span class="dot"></span>
      <span class="label">connecting...</span>
    </span>
  {:else if activityState === 'thinking'}
    <span class="status-indicator thinking">
      <span class="icon">🧠</span>
      <span class="label">thinking</span>
      {#if usage}
        <span class="tokens">↑{formatTokens(usage.reasoning_tokens)}</span>
      {/if}
    </span>
  {:else if activityState === 'running'}
    <div class="running-tools">
      {#each runningTools as tool (tool.callId)}
        <span class="status-indicator running">
          <span class="spinner"></span>
          <span class="label">{shortToolName(tool.name)}</span>
        </span>
      {/each}
      {#if usage}
        <span class="token-count">↑{formatTokens(usage.output_tokens)}</span>
      {/if}
    </div>
  {:else}
    <span class="status-indicator streaming">
      <span class="cursor">_</span>
      <span class="label">streaming</span>
      {#if usage}
        <span class="tokens">↑{formatTokens(usage.output_tokens)}</span>
      {/if}
    </span>
  {/if}
</div>

<style>
  .streaming-status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--term-text-dim);
    min-height: 20px;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .running-tools {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  /* Connecting state */
  .connecting .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--term-warning);
    animation: pulse 1s ease-in-out infinite;
  }

  .connecting .label {
    color: var(--term-warning);
  }

  /* Thinking state */
  .thinking .icon {
    font-size: 12px;
  }

  .thinking .label {
    color: var(--term-accent);
  }

  .thinking .tokens {
    color: var(--term-text-dim);
  }

  /* Running state */
  .running {
    background: var(--term-bg);
    padding: 2px 8px;
    border-radius: 4px;
    border: 1px solid var(--term-border);
  }

  .running .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid var(--term-accent-faded);
    border-top-color: var(--term-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .running .label {
    color: var(--term-accent);
  }

  .token-count {
    color: var(--term-text-dim);
    font-size: 10px;
  }

  /* Streaming state */
  .streaming .cursor {
    color: var(--term-accent);
    animation: blink 0.7s step-end infinite;
  }

  .streaming .label {
    color: var(--term-text-dim);
  }

  .streaming .tokens {
    color: var(--term-accent);
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0; }
  }
</style>
