<script lang="ts">
  import { marked } from 'marked';
  import type { Message, MessageBlock } from '$lib/api/client';
  import TerminalToolCall from './TerminalToolCall.svelte';

  interface Props {
    messages: Message[];
    streamingMessage?: { id: string; blocks: MessageBlock[] } | null;
    onLoadMore?: () => void;
    hasMore?: boolean;
    loadingMore?: boolean;
  }

  let {
    messages,
    streamingMessage = null,
    onLoadMore,
    hasMore = false,
    loadingMore = false,
  }: Props = $props();

  let containerEl: HTMLElement;

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });

  function renderMarkdown(content: string): string {
    try {
      return marked.parse(content) as string;
    } catch {
      return content;
    }
  }

  function handleScroll(event: Event) {
    const target = event.target as HTMLElement;
    if (target.scrollTop < 100 && hasMore && !loadingMore && onLoadMore) {
      onLoadMore();
    }
  }

  function isToolCallLoading(block: MessageBlock): boolean {
    return block.type === 'tool_call' && !block.result;
  }

  export function scrollToBottom() {
    if (containerEl) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  }
</script>

<div
  bind:this={containerEl}
  onscroll={handleScroll}
  class="terminal-view relative flex-1 overflow-y-auto terminal-scroll bg-[var(--term-bg)] p-4 font-mono text-sm"
>
  {#if loadingMore}
    <div class="text-center py-2 text-[var(--term-text-dim)]">
      Loading...
    </div>
  {/if}

  {#if hasMore && !loadingMore}
    <button
      onclick={onLoadMore}
      class="block mx-auto mb-4 text-[var(--term-accent)] hover:underline"
    >
      [load more]
    </button>
  {/if}

  {#if messages.length === 0 && !streamingMessage}
    <div class="flex flex-col items-center justify-center h-full text-[var(--term-text-dim)]">
      <pre class="text-[var(--term-accent)] mb-4">
 __  __ _
|  \/  (_)_ __ __ _
| |\/| | | '__/ _` |
| |  | | | | | (_| |
|_|  |_|_|_|  \__,_|
      </pre>
      <p>Ready. Type a message to begin.</p>
      <p class="text-xs mt-2">GPT-5.2 powered coding assistant</p>
    </div>
  {:else}
    <!-- Messages -->
    {#each messages as message (message.id)}
      <div class="mb-4">
        {#if message.role === 'user'}
          <!-- User message -->
          <div class="flex items-start gap-2">
            <span class="text-[var(--term-prompt)] font-bold select-none">{'>'}</span>
            <div class="text-[var(--term-text)] whitespace-pre-wrap">{message.blocks[0]?.content || ''}</div>
          </div>
        {:else}
          <!-- Assistant message -->
          <div class="pl-4 border-l-2 border-[var(--term-border)]">
            {#each message.blocks as block}
              {#if block.type === 'text'}
                <div class="terminal-prose text-[var(--term-text)]">
                  {@html renderMarkdown(block.content || '')}
                </div>
              {:else if block.type === 'tool_call'}
                <TerminalToolCall
                  name={block.name || 'unknown'}
                  arguments={block.arguments || {}}
                  result={block.result}
                  isLoading={isToolCallLoading(block)}
                />
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {/each}

    <!-- Streaming message -->
    {#if streamingMessage}
      <div class="mb-4">
        <div class="pl-4 border-l-2 border-[var(--term-accent)]">
          {#if streamingMessage.blocks.length === 0}
            <span class="text-[var(--term-accent)] animate-pulse">_</span>
          {:else}
            {#each streamingMessage.blocks as block}
              {#if block.type === 'text'}
                <div class="terminal-prose text-[var(--term-text)]">
                  {@html renderMarkdown(block.content || '')}
                </div>
              {:else if block.type === 'tool_call'}
                <TerminalToolCall
                  name={block.name || 'unknown'}
                  arguments={block.arguments || {}}
                  result={block.result}
                  isLoading={isToolCallLoading(block)}
                />
              {/if}
            {/each}
            <span class="text-[var(--term-accent)] animate-pulse">_</span>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  /* Terminal-specific prose styles */
  .terminal-prose :global(p) {
    margin: 0.5em 0;
  }

  .terminal-prose :global(p:first-child) {
    margin-top: 0;
  }

  .terminal-prose :global(code) {
    background: var(--term-bg-secondary);
    padding: 0.1em 0.3em;
    border-radius: 3px;
    font-size: 0.9em;
  }

  .terminal-prose :global(pre) {
    background: var(--term-bg-secondary);
    padding: 0.75em;
    border-radius: 4px;
    overflow-x: auto;
    margin: 0.5em 0;
  }

  .terminal-prose :global(pre code) {
    background: none;
    padding: 0;
  }

  .terminal-prose :global(a) {
    color: var(--term-accent);
    text-decoration: underline;
  }

  .terminal-prose :global(strong) {
    color: var(--term-text);
    font-weight: 600;
  }

  .terminal-prose :global(ul), .terminal-prose :global(ol) {
    margin: 0.5em 0;
    padding-left: 1.5em;
  }

  .terminal-prose :global(li) {
    margin: 0.25em 0;
  }

  .terminal-prose :global(h1),
  .terminal-prose :global(h2),
  .terminal-prose :global(h3),
  .terminal-prose :global(h4) {
    color: var(--term-accent);
    font-weight: 600;
    margin: 1em 0 0.5em;
  }

  .terminal-prose :global(blockquote) {
    border-left: 2px solid var(--term-border);
    padding-left: 1em;
    color: var(--term-text-dim);
    margin: 0.5em 0;
  }
</style>
