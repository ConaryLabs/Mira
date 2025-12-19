<script lang="ts">
  import { marked } from 'marked';
  import { PROVIDER_DISPLAY_NAMES } from '$lib/types/content';
  import { isExpanded, setExpanded } from '$lib/stores/expansionState';

  interface Props {
    id: string;
    provider: string;
    response: string;
    defaultExpanded?: boolean;
    previewLines?: number;
  }

  let {
    id,
    provider,
    response,
    defaultExpanded = false,
    previewLines = 2,
  }: Props = $props();

  // Use persisted expansion state, fallback to default
  let expanded = $state(isExpanded(id) || defaultExpanded);
  let copied = $state(false);

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });

  const displayName = $derived(PROVIDER_DISPLAY_NAMES[provider] || provider);
  const lines = $derived(response.split('\n'));
  const previewText = $derived(lines.slice(0, previewLines).join('\n'));
  const hasMore = $derived(lines.length > previewLines);

  function renderMarkdown(content: string): string {
    try {
      return marked.parse(content) as string;
    } catch {
      return content;
    }
  }

  function toggle() {
    expanded = !expanded;
    setExpanded(id, expanded); // Persist state
  }

  async function copyResponse() {
    try {
      await navigator.clipboard.writeText(response);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }
</script>

<div class="provider-card border border-[var(--term-border)] rounded overflow-hidden bg-[var(--term-bg)]">
  <!-- Header -->
  <div class="flex items-center gap-2 w-full px-3 py-2">
    <button
      type="button"
      onclick={toggle}
      class="flex items-center gap-2 text-left hover:text-[var(--term-accent)] transition-colors"
    >
      <span class="text-[var(--term-text-dim)] font-mono text-xs select-none">
        [{expanded ? '▼' : '▶'}]
      </span>
      <span class="text-[var(--term-accent)] font-semibold text-sm">{displayName}</span>
    </button>
    <div class="flex-1"></div>
    <button
      type="button"
      onclick={copyResponse}
      class="text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
    >
      {copied ? 'Copied!' : 'Copy'}
    </button>
  </div>

  <!-- Content -->
  <div class="px-3 pb-3 pt-1 text-sm">
    {#if expanded}
      <div class="provider-prose text-[var(--term-text)]">
        {@html renderMarkdown(response)}
      </div>
    {:else}
      <div class="text-[var(--term-text-dim)] line-clamp-2">
        {previewText}{hasMore ? '...' : ''}
      </div>
    {/if}
  </div>
</div>

<style>
  .line-clamp-2 {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .provider-prose :global(p) {
    margin: 0.5em 0;
  }

  .provider-prose :global(p:first-child) {
    margin-top: 0;
  }

  .provider-prose :global(code) {
    background: var(--term-bg-secondary);
    padding: 0.1em 0.3em;
    border-radius: 3px;
    font-size: 0.9em;
  }

  .provider-prose :global(pre) {
    background: var(--term-bg-secondary);
    padding: 0.75em;
    border-radius: 4px;
    overflow-x: auto;
    margin: 0.5em 0;
  }

  .provider-prose :global(pre code) {
    background: none;
    padding: 0;
  }

  .provider-prose :global(strong) {
    color: var(--term-text);
    font-weight: 600;
  }

  .provider-prose :global(ul), .provider-prose :global(ol) {
    margin: 0.5em 0;
    padding-left: 1.5em;
  }

  .provider-prose :global(li) {
    margin: 0.25em 0;
  }
</style>
