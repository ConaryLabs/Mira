<script lang="ts">
  import type { Message, MessageBlock, UsageInfo } from '$lib/api/client';
  import BlockRenderer from '$lib/components/chat/BlockRenderer.svelte';
  import StreamingStatus from './StreamingStatus.svelte';

  interface Props {
    messages: Message[];
    streamingMessage?: { id: string; blocks: MessageBlock[]; usage?: UsageInfo } | null;
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

  function formatTokens(n: number): string {
    if (n >= 1000) {
      return (n / 1000).toFixed(1) + 'k';
    }
    return n.toString();
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts * 1000);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  let containerEl: HTMLElement;
  let isAtBottom = $state(true);

  function handleScroll(event: Event) {
    const target = event.target as HTMLElement;
    // Check if at bottom for sticky scroll
    const threshold = 50;
    isAtBottom = target.scrollHeight - target.scrollTop - target.clientHeight < threshold;

    // Load more when scrolled near top
    if (target.scrollTop < 100 && hasMore && !loadingMore && onLoadMore) {
      onLoadMore();
    }
  }

  export function scrollToBottom() {
    if (containerEl && isAtBottom) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  }

  // Force scroll to bottom (ignoring isAtBottom)
  export function forceScrollToBottom() {
    if (containerEl) {
      containerEl.scrollTop = containerEl.scrollHeight;
      isAtBottom = true;
    }
  }
</script>

<div
  bind:this={containerEl}
  onscroll={handleScroll}
  role="log"
  aria-live="polite"
  aria-busy={streamingMessage !== null}
  aria-label="Chat messages"
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
      <p class="text-xs mt-2">DeepSeek-powered coding assistant</p>
    </div>
  {:else}
    <!-- Messages -->
    {#each messages as message (message.id)}
      <div class="terminal-message mb-4 group">
        {#if message.role === 'user'}
          <!-- User message -->
          <div class="message-container">
            <div class="message-header">
              <span class="role-label role-user">[you]</span>
              <span class="message-timestamp">{formatTimestamp(message.created_at)}</span>
            </div>
            <div class="user-content">
              <span class="text-[var(--term-prompt)] font-bold select-none mr-2">{'>'}</span>
              <span class="text-[var(--term-text)] whitespace-pre-wrap">{message.blocks[0]?.content || ''}</span>
            </div>
          </div>
        {:else}
          <!-- Assistant message -->
          <div class="message-container">
            <div class="message-header">
              <span class="role-label role-assistant">[mira]</span>
              <span class="message-timestamp">{formatTimestamp(message.created_at)}</span>
            </div>
            <div class="assistant-content pl-4 border-l-2 border-[var(--term-border)]">
              {#each message.blocks as block, blockIndex}
                <BlockRenderer
                  {block}
                  blockId={`${message.id}-${blockIndex}`}
                />
              {/each}
              <!-- Usage stats -->
              {#if message.usage}
              {@const cachePct = message.usage.input_tokens > 0
                ? Math.round((message.usage.cached_tokens / message.usage.input_tokens) * 100)
                : 0}
              <div class="mt-2 text-xs text-[var(--term-text-dim)] font-mono">
                <span title="Input tokens">↓{formatTokens(message.usage.input_tokens)}</span>
                <span class="ml-2" title="Output tokens">↑{formatTokens(message.usage.output_tokens)}</span>
                {#if message.usage.reasoning_tokens > 0}
                  <span class="ml-2" title="Reasoning tokens">🧠{formatTokens(message.usage.reasoning_tokens)}</span>
                {/if}
                <span class="ml-2" class:text-[var(--term-success)]={cachePct >= 50} title="Cached tokens">⚡{cachePct}%</span>
              </div>
            {/if}
            </div>
          </div>
        {/if}
      </div>
    {/each}

    <!-- Streaming message -->
    {#if streamingMessage}
      <div class="terminal-message-streaming mb-4">
        <div class="message-container">
          <div class="message-header">
            <span class="role-label role-assistant">[mira]</span>
            <StreamingStatus
              usage={streamingMessage.usage}
              hasBlocks={streamingMessage.blocks.length > 0}
            />
          </div>
          <div class="assistant-content pl-4 border-l-2 border-[var(--term-accent)]">
            {#if streamingMessage.blocks.length === 0}
              <span class="text-[var(--term-accent)] animate-pulse">_</span>
            {:else}
            {#each streamingMessage.blocks as block, blockIndex}
              <BlockRenderer
                {block}
                blockId={`streaming-${blockIndex}`}
                isStreaming={true}
              />
            {/each}
            <span class="text-[var(--term-accent)] animate-pulse">_</span>
          {/if}
          <!-- Live usage stats during streaming -->
          {#if streamingMessage.usage}
            {@const cachePct = streamingMessage.usage.input_tokens > 0
              ? Math.round((streamingMessage.usage.cached_tokens / streamingMessage.usage.input_tokens) * 100)
              : 0}
            <div class="mt-2 text-xs text-[var(--term-text-dim)] font-mono">
              <span title="Input tokens">↓{formatTokens(streamingMessage.usage.input_tokens)}</span>
              <span class="ml-2" title="Output tokens">↑{formatTokens(streamingMessage.usage.output_tokens)}</span>
              {#if streamingMessage.usage.reasoning_tokens > 0}
                <span class="ml-2" title="Reasoning tokens">🧠{formatTokens(streamingMessage.usage.reasoning_tokens)}</span>
              {/if}
              <span class="ml-2" class:text-[var(--term-success)]={cachePct >= 50} title="Cached tokens">⚡{cachePct}%</span>
            </div>
          {/if}
          </div>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  /* Performance: allow browser to skip rendering off-screen messages */
  .terminal-message {
    content-visibility: auto;
    contain-intrinsic-size: auto 100px;
  }

  /* Don't apply to streaming message (always needs to be rendered) */
  .terminal-message-streaming {
    content-visibility: visible;
  }

  /* Message container */
  .message-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* Message header with role and timestamp */
  .message-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    font-family: var(--font-mono);
  }

  /* Role labels */
  .role-label {
    font-weight: 600;
    text-transform: lowercase;
  }

  .role-user {
    color: var(--term-prompt);
  }

  .role-assistant {
    color: var(--term-accent);
  }

  /* Timestamp - hidden by default, shown on hover */
  .message-timestamp {
    color: var(--term-text-dim);
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  .terminal-message:hover .message-timestamp,
  .group:hover .message-timestamp {
    opacity: 1;
  }

  /* User content */
  .user-content {
    display: flex;
    align-items: flex-start;
  }

  /* Assistant content */
  .assistant-content {
    margin-top: 2px;
  }
</style>
