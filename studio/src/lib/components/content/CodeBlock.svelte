<script lang="ts">
  import { isExpanded, setExpanded } from '$lib/stores/expansionState';
  import { artifactViewer, shouldShowViewerButton } from '$lib/stores/artifacts.svelte';

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

  // Language display names
  const languageNames: Record<string, string> = {
    ts: 'TypeScript',
    typescript: 'TypeScript',
    js: 'JavaScript',
    javascript: 'JavaScript',
    rs: 'Rust',
    rust: 'Rust',
    py: 'Python',
    python: 'Python',
    sh: 'Shell',
    bash: 'Bash',
    json: 'JSON',
    html: 'HTML',
    css: 'CSS',
    svelte: 'Svelte',
    sql: 'SQL',
    toml: 'TOML',
    yaml: 'YAML',
    yml: 'YAML',
    md: 'Markdown',
    markdown: 'Markdown',
    text: 'Plain Text',
  };

  const displayLang = $derived(languageNames[language.toLowerCase()] || language);
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
        class="text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
    </div>
  </div>

  <!-- Code content -->
  <div class="p-3 overflow-x-auto">
    <pre class="text-sm font-mono text-[var(--term-text)] whitespace-pre">{expanded || !hasMore ? code : previewCode}</pre>
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
  pre {
    margin: 0;
    line-height: 1.5;
  }
</style>
