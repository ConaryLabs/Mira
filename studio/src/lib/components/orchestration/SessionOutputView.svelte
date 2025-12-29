<script lang="ts">
  /**
   * Live output viewer for Claude Code sessions
   * Shows streaming chunks from session SSE events
   */
  interface OutputChunk {
    session_id: string;
    content: string;
    timestamp: number;
  }

  interface Props {
    chunks: OutputChunk[];
    maxHeight?: string;
  }

  let {
    chunks,
    maxHeight = '300px',
  }: Props = $props();

  let containerEl: HTMLElement;

  // Auto-scroll to bottom when new chunks arrive
  $effect(() => {
    if (chunks.length > 0 && containerEl) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  });
</script>

<div
  bind:this={containerEl}
  class="session-output"
  style="max-height: {maxHeight};"
>
  {#if chunks.length === 0}
    <div class="empty-output">Waiting for output...</div>
  {:else}
    {#each chunks as chunk (chunk.timestamp)}
      <div class="output-chunk">{chunk.content}</div>
    {/each}
  {/if}
</div>

<style>
  .session-output {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    padding: 8px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--term-text);
    margin-top: 8px;
  }

  .empty-output {
    color: var(--term-text-dim);
    font-style: italic;
    text-align: center;
    padding: 12px;
  }

  .output-chunk {
    line-height: 1.4;
  }
</style>
