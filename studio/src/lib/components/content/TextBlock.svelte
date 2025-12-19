<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  interface Props {
    content: string;
  }

  let { content }: Props = $props();

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
</script>

<div class="text-block terminal-prose text-[var(--term-text)]">
  {@html renderMarkdown(content)}
</div>

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
