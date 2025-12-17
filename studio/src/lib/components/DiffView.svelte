<script lang="ts">
  import type { DiffInfo } from '$lib/api/client';

  interface Props {
    diff: DiffInfo;
  }

  let { diff }: Props = $props();
  let isExpanded = $state(true);

  interface DiffLine {
    type: 'add' | 'remove' | 'context' | 'header';
    content: string;
  }

  function computeDiffLines(diff: DiffInfo): DiffLine[] {
    const lines: DiffLine[] = [];

    if (diff.is_new_file) {
      // New file - all lines are additions
      lines.push({ type: 'header', content: `+++ ${diff.path} (new file)` });
      for (const line of diff.new_content.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    } else if (diff.old_content) {
      // Edit - show old/new
      lines.push({ type: 'header', content: `--- ${diff.path}` });
      lines.push({ type: 'header', content: `+++ ${diff.path}` });

      // Simple diff: show removed then added
      for (const line of diff.old_content.split('\n')) {
        lines.push({ type: 'remove', content: `- ${line}` });
      }
      for (const line of diff.new_content.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    } else {
      // Just new content
      lines.push({ type: 'header', content: `+++ ${diff.path}` });
      for (const line of diff.new_content.split('\n')) {
        lines.push({ type: 'add', content: `+ ${line}` });
      }
    }

    return lines;
  }

  function getLineClass(type: DiffLine['type']): string {
    switch (type) {
      case 'add':
        return 'bg-green-50 text-green-800';
      case 'remove':
        return 'bg-red-50 text-red-800';
      case 'header':
        return 'text-gray-500 font-semibold';
      default:
        return 'text-gray-600';
    }
  }

  let diffLines = $derived(computeDiffLines(diff));
</script>

<div class="diff-view border border-gray-300 rounded overflow-hidden bg-gray-900 text-sm font-mono">
  <button
    class="w-full flex items-center justify-between p-2 bg-gray-800 text-gray-300 hover:bg-gray-700 transition-colors text-left"
    onclick={() => isExpanded = !isExpanded}
  >
    <div class="flex items-center gap-2">
      <span class="text-gray-400">#</span>
      <span>{diff.path}</span>
    </div>
    <div class="flex items-center gap-2">
      <span class="text-xs px-1.5 py-0.5 rounded {diff.is_new_file ? 'bg-green-600 text-white' : 'bg-yellow-600 text-white'}">
        {diff.is_new_file ? 'new' : 'modified'}
      </span>
      <span class="text-gray-400">{isExpanded ? '−' : '+'}</span>
    </div>
  </button>

  {#if isExpanded}
    <div class="p-2 overflow-x-auto max-h-96 bg-white">
      {#each diffLines as line}
        <div class="px-2 py-0.5 whitespace-pre {getLineClass(line.type)}">{line.content}</div>
      {/each}
    </div>
  {/if}
</div>
