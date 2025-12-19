<script lang="ts">
  interface Props {
    path: string;
    oldContent?: string;
    newContent: string;
    isNewFile?: boolean;
    previewLines?: number;
    defaultExpanded?: boolean;
  }

  let {
    path,
    oldContent,
    newContent,
    isNewFile = false,
    previewLines = 5,
    defaultExpanded = false,
  }: Props = $props();

  let expanded = $state(defaultExpanded);
  let copied = $state(false);

  interface DiffLine {
    type: 'add' | 'remove' | 'context' | 'header';
    content: string;
  }

  function computeDiffLines(): DiffLine[] {
    const lines: DiffLine[] = [];

    if (isNewFile) {
      // New file - all lines are additions
      lines.push({ type: 'header', content: `+++ ${path} (new file)` });
      for (const line of newContent.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    } else if (oldContent) {
      // Edit - show old/new
      lines.push({ type: 'header', content: `--- ${path}` });
      lines.push({ type: 'header', content: `+++ ${path}` });

      // Simple diff: show removed then added
      for (const line of oldContent.split('\n')) {
        lines.push({ type: 'remove', content: `- ${line}` });
      }
      for (const line of newContent.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    } else {
      // Just new content
      lines.push({ type: 'header', content: `+++ ${path}` });
      for (const line of newContent.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    }

    return lines;
  }

  const diffLines = $derived(computeDiffLines());
  const changedLineCount = $derived(diffLines.filter(l => l.type === 'add' || l.type === 'remove').length);
  const previewDiffLines = $derived(diffLines.slice(0, previewLines + 1)); // +1 for header
  const hasMore = $derived(diffLines.length > previewLines + 1);

  function getLineClass(type: DiffLine['type']): string {
    switch (type) {
      case 'add':
        return 'diff-add';
      case 'remove':
        return 'diff-remove';
      case 'header':
        return 'diff-header';
      default:
        return 'diff-context';
    }
  }

  function toggle() {
    expanded = !expanded;
  }

  async function copyDiff() {
    try {
      const text = diffLines.map(l => l.content).join('\n');
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }
</script>

<div class="diff-view my-2 border border-[var(--term-border)] rounded overflow-hidden bg-[var(--term-bg-secondary)]">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-2 bg-[var(--term-bg)]">
    <button
      type="button"
      onclick={toggle}
      class="flex items-center gap-2 text-sm hover:text-[var(--term-accent)] transition-colors"
    >
      <span class="text-[var(--term-text-dim)] font-mono select-none">
        [{expanded ? '-' : '+'}]
      </span>
      <span class="text-[var(--term-accent)]">{path}</span>
      {#if !expanded && hasMore}
        <span class="text-[var(--term-text-dim)] text-xs">({changedLineCount} lines)</span>
      {/if}
    </button>
    <div class="flex items-center gap-2">
      <span class="text-xs px-1.5 py-0.5 rounded {isNewFile ? 'bg-[var(--term-success)] text-[var(--term-bg)]' : 'bg-[var(--term-warning)] text-[var(--term-bg)]'}">
        {isNewFile ? 'new' : 'modified'}
      </span>
      <button
        type="button"
        onclick={copyDiff}
        class="text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
    </div>
  </div>

  <!-- Diff content -->
  <div class="p-2 overflow-x-auto font-mono text-xs">
    {#each (expanded ? diffLines : previewDiffLines) as line}
      <div class="px-2 py-0.5 whitespace-pre {getLineClass(line.type)}">{line.content}</div>
    {/each}
    {#if !expanded && hasMore}
      <button
        type="button"
        onclick={toggle}
        class="mt-1 text-xs text-[var(--term-text-dim)] hover:text-[var(--term-accent)]"
      >
        ... {diffLines.length - previewDiffLines.length} more lines
      </button>
    {/if}
  </div>
</div>


<style>
  .diff-add {
    background-color: rgba(var(--term-success-rgb, 34, 197, 94), 0.15);
    color: var(--term-success);
  }

  .diff-remove {
    background-color: rgba(var(--term-error-rgb, 239, 68, 68), 0.15);
    color: var(--term-error);
  }

  .diff-header {
    color: var(--term-accent);
    font-weight: 600;
  }

  .diff-context {
    color: var(--term-text-dim);
  }
</style>
