<script lang="ts">
  /**
   * TextRenderer - Simple markdown-ish text rendering
   *
   * Handles basic formatting without heavy parsing:
   * - **bold**
   * - *italic*
   * - `inline code`
   * - [links](url)
   *
   * No code block parsing - that's handled by backend typed events.
   */

  interface Props {
    text: string;
  }

  let { text }: Props = $props();

  // Simple regex-based formatting
  // Process in order: code (to protect from other formatting), then bold, italic, links
  function renderText(input: string): string {
    if (!input) return '';

    let result = input;

    // Escape HTML
    result = result
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');

    // Inline code: `code` -> <code>code</code>
    result = result.replace(
      /`([^`\n]+)`/g,
      '<code class="inline-code">$1</code>'
    );

    // Bold: **text** -> <strong>text</strong>
    result = result.replace(
      /\*\*([^*]+)\*\*/g,
      '<strong>$1</strong>'
    );

    // Italic: *text* -> <em>text</em> (but not inside words)
    result = result.replace(
      /(?<!\w)\*([^*]+)\*(?!\w)/g,
      '<em>$1</em>'
    );

    // Links: [text](url) -> <a href="url">text</a>
    result = result.replace(
      /\[([^\]]+)\]\(([^)]+)\)/g,
      '<a href="$2" target="_blank" rel="noopener noreferrer" class="text-link">$1</a>'
    );

    return result;
  }
</script>

<span class="text-content">{@html renderText(text)}</span>

<style>
  .text-content {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .text-content :global(.inline-code) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 0.15em 0.4em;
    background: var(--term-bg-secondary);
    border-radius: 4px;
    color: var(--term-accent);
  }

  .text-content :global(strong) {
    font-weight: 600;
    color: var(--term-text);
  }

  .text-content :global(em) {
    font-style: italic;
  }

  .text-content :global(.text-link) {
    color: var(--term-accent);
    text-decoration: none;
  }

  .text-content :global(.text-link:hover) {
    text-decoration: underline;
  }
</style>
