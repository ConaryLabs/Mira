<script lang="ts">
  import type { ToolCallResult } from '$lib/api/client';
  import DiffView from '../DiffView.svelte';

  interface Props {
    name: string;
    arguments: Record<string, unknown>;
    result?: ToolCallResult;
    isLoading?: boolean;
  }

  let { name, arguments: args, result, isLoading = false }: Props = $props();

  let expanded = $state(false);

  // Tool icons (terminal style)
  const toolIcons: Record<string, string> = {
    read_file: 'cat',
    write_file: 'tee',
    edit_file: 'sed',
    glob: 'find',
    grep: 'grep',
    bash: '$',
    list_files: 'ls',
    search: 'rg',
    default: '>>',
  };

  function getIcon(toolName: string): string {
    return toolIcons[toolName] || toolIcons.default;
  }

  function getArgsSummary(): string {
    if (args.path) return String(args.path);
    if (args.pattern) return String(args.pattern);
    if (args.command) {
      const cmd = String(args.command);
      return cmd.length > 50 ? cmd.slice(0, 50) + '...' : cmd;
    }
    if (args.file_path) return String(args.file_path);
    return '';
  }

  function getStatus(): { text: string; class: string } {
    if (isLoading) return { text: 'running...', class: 'text-[var(--term-warning)]' };
    if (!result) return { text: 'pending', class: 'text-[var(--term-text-dim)]' };
    if (result.success) return { text: 'done', class: 'text-[var(--term-success)]' };
    return { text: 'error', class: 'text-[var(--term-error)]' };
  }

  function formatOutput(output: string): string {
    if (!output) return '';
    const lines = output.split('\n');
    if (lines.length > 20) {
      return lines.slice(0, 20).join('\n') + `\n... (${lines.length - 20} more lines)`;
    }
    return output;
  }

  const status = $derived(getStatus());
</script>

<div class="my-2 font-mono text-sm">
  <!-- Header row -->
  <button
    onclick={() => expanded = !expanded}
    class="flex items-center gap-2 w-full text-left hover:bg-[var(--term-bg-secondary)] px-2 py-1 rounded transition-colors"
  >
    <span class="text-[var(--term-text-dim)]">[{expanded ? '-' : '+'}]</span>
    <span class="text-[var(--term-accent)]">{getIcon(name)}</span>
    <span class="text-[var(--term-text)]">{name}</span>
    <span class="text-[var(--term-text-dim)] truncate flex-1">{getArgsSummary()}</span>
    <span class={status.class}>[{status.text}]</span>
  </button>

  <!-- Expanded content -->
  {#if expanded}
    <div class="ml-6 mt-1 pl-2 border-l border-[var(--term-border)]">
      <!-- Arguments -->
      <div class="text-[var(--term-text-dim)] text-xs mb-2">
        {#each Object.entries(args) as [key, value]}
          <div class="truncate">
            <span class="text-[var(--term-accent)]">{key}:</span>
            <span>{typeof value === 'string' ? value : JSON.stringify(value)}</span>
          </div>
        {/each}
      </div>

      <!-- Result -->
      {#if result}
        {#if result.diff}
          <DiffView
            path={result.diff.path}
            oldContent={result.diff.old_content}
            newContent={result.diff.new_content}
            isNewFile={result.diff.is_new_file}
          />
        {:else if result.output}
          <div class="bg-[var(--term-bg)] p-2 rounded text-xs overflow-x-auto">
            <pre class="text-[var(--term-text)] whitespace-pre-wrap">{formatOutput(result.output)}</pre>
          </div>
        {/if}
      {:else if isLoading}
        <div class="flex items-center gap-2 text-[var(--term-warning)]">
          <span class="animate-pulse">...</span>
        </div>
      {/if}
    </div>
  {/if}
</div>
