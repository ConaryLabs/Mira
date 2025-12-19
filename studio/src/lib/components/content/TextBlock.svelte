<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  interface Props {
    content: string;
  }

  let { content }: Props = $props();

  // Performance threshold - skip markdown for large content until expanded
  const LARGE_CONTENT_THRESHOLD = 10000; // 10KB
  const PREVIEW_LINES = 5;

  // For large content, start collapsed
  const isLarge = $derived(content.length > LARGE_CONTENT_THRESHOLD);
  let expanded = $state(false);

  // Preview for large content
  const previewContent = $derived(() => {
    if (!isLarge) return content;
    const lines = content.split('\n').slice(0, PREVIEW_LINES);
    return lines.join('\n') + '\n...';
  });

  // Configure marked - disable raw HTML for safety
  marked.setOptions({
    breaks: true,
    gfm: true,
  });

  // DOMPurify config - allow safe tags only
  const PURIFY_CONFIG = {
    ALLOWED_TAGS: [
      'p', 'br', 'strong', 'em', 'b', 'i', 'u', 's', 'del',
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'ul', 'ol', 'li',
      'blockquote', 'pre', 'code',
      'a', 'hr',
      'table', 'thead', 'tbody', 'tr', 'th', 'td',
    ],
    ALLOWED_ATTR: ['href', 'target', 'rel', 'class'],
    ALLOW_DATA_ATTR: false,
    // Block dangerous protocols
    ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
  };

  function renderMarkdown(text: string): string {
    try {
      const html = marked.parse(text) as string;
      return DOMPurify.sanitize(html, PURIFY_CONFIG);
    } catch {
      // Fallback: escape and return as text
      return DOMPurify.sanitize(text);
    }
  }

  function toggle() {
    expanded = !expanded;
  }
</script>

{#if isLarge && !expanded}
  <!-- Large content: plain text preview until expanded -->
  <div class="text-block text-[var(--term-text)]">
    <pre class="whitespace-pre-wrap text-sm">{previewContent()}</pre>
    <button
      type="button"
      onclick={toggle}
      class="mt-1 text-xs text-[var(--term-accent)] hover:underline"
    >
      [{Math.round(content.length / 1024)}KB - click to expand]
    </button>
  </div>
{:else}
  <div class="text-block terminal-prose text-[var(--term-text)]">
    {@html renderMarkdown(content)}
  </div>
{/if}

<style>
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
