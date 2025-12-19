<script lang="ts">
  import { marked } from 'marked';

  interface Props {
    content: string;
  }

  let { content }: Props = $props();

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });

  function renderMarkdown(text: string): string {
    try {
      return marked.parse(text) as string;
    } catch {
      return text;
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
