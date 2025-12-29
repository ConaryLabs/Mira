<script lang="ts">
  import { isExpanded, setExpanded } from '$lib/stores/expansionState';
  import { artifactViewer, shouldShowViewerButton } from '$lib/stores/artifacts.svelte';
  import { highlightCode, getLanguageDisplayName, detectLanguage } from '$lib/utils/highlight';

  interface Props {
    id: string;
    language: string;
    code: string;
    filename?: string;
    previewLines?: number;
    defaultExpanded?: boolean;
  }

  let {
    id,
    language,
    code,
    filename,
    previewLines = 5,
    defaultExpanded = false,
  }: Props = $props();

  // Use persisted expansion state, fallback to default
  let expanded = $state(isExpanded(id) || defaultExpanded);
  let copied = $state(false);

  // Show "Open in viewer" button for large files
  const showViewerButton = $derived(shouldShowViewerButton(code));

  function openInViewer() {
    artifactViewer.open({ filename, language, code });
  }

  const lines = $derived(code.split('\n'));
  const previewCode = $derived(lines.slice(0, previewLines).join('\n'));
  const hasMore = $derived(lines.length > previewLines);
  const lineCount = $derived(lines.length);

  // Detect language if not provided
  const effectiveLanguage = $derived(language || detectLanguage(code));
  const displayLang = $derived(getLanguageDisplayName(effectiveLanguage));

  // Compute highlighted code (for both preview and full)
  const highlightedCode = $derived(highlightCode(code, effectiveLanguage));
  const highlightedPreview = $derived(highlightCode(previewCode, effectiveLanguage));

  function toggle() {
    expanded = !expanded;
    setExpanded(id, expanded); // Persist state
  }

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }
</script>

<div class="code-block my-2 rounded overflow-hidden bg-[var(--term-bg-secondary)]">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-1.5 bg-[var(--term-bg)] border-b border-[var(--term-border)]">
    <button
      type="button"
      onclick={toggle}
      aria-expanded={expanded}
      aria-label="{expanded ? 'Collapse' : 'Expand'} code block{filename ? `: ${filename}` : ''}"
      class="flex items-center gap-2 text-xs hover:text-[var(--term-accent)] transition-colors"
    >
      <span class="text-[var(--term-text-dim)] font-mono select-none">
        [{expanded ? '-' : '+'}]
      </span>
      <span class="text-[var(--term-accent)]">{displayLang}</span>
      {#if filename}
        <span class="text-[var(--term-text-dim)]">{filename}</span>
      {/if}
      {#if hasMore && !expanded}
        <span class="text-[var(--term-text-dim)]">({lineCount} lines)</span>
      {/if}
    </button>

    <div class="flex items-center gap-1">
      {#if showViewerButton}
        <button
          type="button"
          onclick={openInViewer}
          aria-label="Open in full viewer"
          class="text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
        >
          Expand
        </button>
      {/if}
      <button
        type="button"
        onclick={copyCode}
        aria-label={copied ? 'Code copied to clipboard' : 'Copy code to clipboard'}
        class="copy-button text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
    </div>
  </div>

  <!-- Code content with syntax highlighting -->
  <div class="p-3 overflow-x-auto">
    <pre class="code-content"><code>{@html expanded || !hasMore ? highlightedCode : highlightedPreview}</code></pre>
    {#if hasMore && !expanded}
      <button
        type="button"
        onclick={toggle}
        aria-label="Show all {lineCount} lines"
        class="mt-2 text-xs text-[var(--term-text-dim)] hover:text-[var(--term-accent)]"
      >
        ... {lineCount - previewLines} more lines
      </button>
    {/if}
  </div>
</div>

<style>
  pre.code-content {
    margin: 0;
    line-height: 1.5;
    font-family: 'JetBrains Mono', 'Berkeley Mono', 'SF Mono', Consolas, monospace;
    font-size: 0.85rem;
    white-space: pre;
    color: var(--term-text);
  }

  pre.code-content code {
    background: none;
    padding: 0;
    color: inherit;
  }

  .copy-button {
    font-family: var(--font-mono);
  }

  /* Prism token styles - using CSS custom properties for theme integration */
  pre.code-content :global(.token.comment),
  pre.code-content :global(.token.prolog),
  pre.code-content :global(.token.doctype),
  pre.code-content :global(.token.cdata) {
    color: var(--prism-comment, var(--term-text-dim));
    font-style: italic;
  }

  pre.code-content :global(.token.punctuation) {
    color: var(--term-text);
  }

  pre.code-content :global(.token.namespace) {
    opacity: 0.7;
  }

  pre.code-content :global(.token.property),
  pre.code-content :global(.token.tag),
  pre.code-content :global(.token.boolean),
  pre.code-content :global(.token.number),
  pre.code-content :global(.token.constant),
  pre.code-content :global(.token.symbol),
  pre.code-content :global(.token.deleted) {
    color: var(--prism-number);
  }

  pre.code-content :global(.token.selector),
  pre.code-content :global(.token.attr-name),
  pre.code-content :global(.token.string),
  pre.code-content :global(.token.char),
  pre.code-content :global(.token.builtin),
  pre.code-content :global(.token.inserted) {
    color: var(--prism-string);
  }

  pre.code-content :global(.token.operator),
  pre.code-content :global(.token.entity),
  pre.code-content :global(.token.url),
  pre.code-content :global(.language-css .token.string),
  pre.code-content :global(.style .token.string) {
    color: var(--prism-operator);
  }

  pre.code-content :global(.token.atrule),
  pre.code-content :global(.token.attr-value),
  pre.code-content :global(.token.keyword) {
    color: var(--prism-keyword);
  }

  pre.code-content :global(.token.function),
  pre.code-content :global(.token.class-name) {
    color: var(--prism-function);
  }

  pre.code-content :global(.token.regex),
  pre.code-content :global(.token.important),
  pre.code-content :global(.token.variable) {
    color: var(--prism-variable);
  }

  pre.code-content :global(.token.important),
  pre.code-content :global(.token.bold) {
    font-weight: bold;
  }

  pre.code-content :global(.token.italic) {
    font-style: italic;
  }

  pre.code-content :global(.token.entity) {
    cursor: help;
  }

  /* Language-specific tweaks */
  pre.code-content :global(.token.macro),
  pre.code-content :global(.token.attribute) {
    color: var(--prism-attribute);
  }

  pre.code-content :global(.token.lifetime) {
    color: var(--prism-variable);
  }
</style>
