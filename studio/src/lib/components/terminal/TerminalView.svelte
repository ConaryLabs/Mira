<script lang="ts">
  import type { Message, MessageBlock, UsageInfo } from '$lib/api/client';
  import type { CouncilResponses } from '$lib/types/content';
  import TerminalToolCall from './TerminalToolCall.svelte';
  import { ContentBlock, StreamingTextBlock, CodeBlock, CouncilView } from '$lib/components/content';
  import { parseTextContent } from '$lib/parser/contentParser';

  // Convert MessageBlock council fields to CouncilResponses format
  function toCouncilResponses(block: MessageBlock): CouncilResponses {
    return {
      'gpt-5.2': block.gpt,
      'opus-4.5': block.opus,
      'gemini-3-pro': block.gemini,
    };
  }

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

  function isToolCallLoading(block: MessageBlock): boolean {
    return block.type === 'tool_call' && !block.result;
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
            {#each message.blocks as block, blockIndex}
              {#if block.type === 'text'}
                <!-- Parse completed text blocks into rich content (isStreaming=false for caching) -->
                {@const parsed = parseTextContent(block.content || '', `${message.id}-${blockIndex}`, false)}
                {#each parsed.segments as segment (segment.id)}
                  <ContentBlock content={segment} />
                {/each}
              {:else if block.type === 'code_block'}
                <!-- Typed code block from backend - no parsing needed -->
                <CodeBlock
                  id={`${message.id}-${blockIndex}`}
                  language={block.language || ''}
                  code={block.code || ''}
                  filename={block.filename}
                />
              {:else if block.type === 'council'}
                <!-- Typed council from backend - no parsing needed -->
                <CouncilView
                  id={`${message.id}-${blockIndex}`}
                  responses={toCouncilResponses(block)}
                />
              {:else if block.type === 'tool_call'}
                <TerminalToolCall
                  name={block.name || 'unknown'}
                  arguments={block.arguments || {}}
                  result={block.result}
                  isLoading={isToolCallLoading(block)}
                />
              {/if}
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
            {#each streamingMessage.blocks as block, blockIndex}
              {#if block.type === 'text'}
                <!-- Debounced streaming parser - only parses when content stabilizes -->
                <StreamingTextBlock
                  content={block.content || ''}
                  blockId={`streaming-${blockIndex}`}
                />
              {:else if block.type === 'code_block'}
                <!-- Streaming code block from backend -->
                <CodeBlock
                  id={`streaming-${blockIndex}`}
                  language={block.language || ''}
                  code={block.code || ''}
                  filename={block.filename}
                />
              {:else if block.type === 'council'}
                <!-- Streaming council from backend -->
                <CouncilView
                  id={`streaming-${blockIndex}`}
                  responses={toCouncilResponses(block)}
                />
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
    {/if}
  {/if}
</div>

<style>
  /* Styles moved to individual content components */
</style>
