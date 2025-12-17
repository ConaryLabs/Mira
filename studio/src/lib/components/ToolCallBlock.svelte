<script lang="ts">
  import DiffView from './DiffView.svelte';
  import type { ToolCallResult, DiffInfo } from '$lib/api/client';

  interface Props {
    name: string;
    arguments: Record<string, unknown>;
    result?: ToolCallResult;
    isLoading?: boolean;
  }

  let { name, arguments: args, result, isLoading = false }: Props = $props();
  let isExpanded = $state(false);

  function getToolIcon(toolName: string): string {
    const icons: Record<string, string> = {
      read_file: '📄',
      write_file: '✏️',
      edit_file: '✂️',
      glob: '🔍',
      grep: '🔎',
      bash: '💻',
      web_search: '🌐',
      web_fetch: '📥',
      remember: '💾',
      recall: '💭',
    };
    return icons[toolName] || '🔧';
  }

  function formatArgs(args: Record<string, unknown>): string {
    // Show key info inline based on tool type
    if (args.path) return String(args.path);
    if (args.pattern) return String(args.pattern);
    if (args.command) {
      const cmd = String(args.command);
      return cmd.length > 50 ? cmd.slice(0, 50) + '...' : cmd;
    }
    if (args.query) return String(args.query);
    if (args.url) return String(args.url);
    if (args.content) {
      const content = String(args.content);
      return content.length > 50 ? content.slice(0, 50) + '...' : content;
    }
    return '';
  }

  function truncateOutput(output: string, maxLength = 500): string {
    if (output.length <= maxLength) return output;
    return output.slice(0, maxLength) + '\n... (truncated)';
  }
</script>

<div class="tool-call-block border border-gray-200 rounded-lg my-2 bg-gray-50 overflow-hidden">
  <button
    class="w-full flex items-center justify-between p-3 hover:bg-gray-100 transition-colors text-left"
    onclick={() => isExpanded = !isExpanded}
  >
    <div class="flex items-center gap-2 min-w-0 flex-1">
      <span class="text-lg flex-shrink-0">{getToolIcon(name)}</span>
      <span class="font-mono text-sm font-medium text-gray-700">{name}</span>
      <span class="text-sm text-gray-500 truncate">{formatArgs(args)}</span>
    </div>
    <div class="flex items-center gap-2 flex-shrink-0">
      {#if isLoading}
        <span class="flex gap-1">
          <span class="w-1.5 h-1.5 bg-violet-500 rounded-full animate-bounce" style="animation-delay: 0ms"></span>
          <span class="w-1.5 h-1.5 bg-violet-500 rounded-full animate-bounce" style="animation-delay: 150ms"></span>
          <span class="w-1.5 h-1.5 bg-violet-500 rounded-full animate-bounce" style="animation-delay: 300ms"></span>
        </span>
      {:else if result}
        <span class="text-xs px-2 py-0.5 rounded-full {result.success ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}">
          {result.success ? 'done' : 'error'}
        </span>
      {/if}
      <span class="text-gray-400 text-sm">{isExpanded ? '−' : '+'}</span>
    </div>
  </button>

  {#if isExpanded}
    <div class="p-3 border-t border-gray-200 bg-white space-y-3">
      <!-- Arguments -->
      <div>
        <div class="text-xs font-medium text-gray-500 mb-1">Arguments</div>
        <pre class="text-xs bg-gray-100 p-2 rounded overflow-x-auto font-mono">{JSON.stringify(args, null, 2)}</pre>
      </div>

      <!-- Result -->
      {#if result}
        {#if result.diff}
          <div>
            <div class="text-xs font-medium text-gray-500 mb-1">Changes</div>
            <DiffView diff={result.diff} />
          </div>
        {:else}
          <div>
            <div class="text-xs font-medium text-gray-500 mb-1">Output</div>
            <pre class="text-xs bg-gray-100 p-2 rounded overflow-x-auto max-h-64 font-mono {result.success ? '' : 'text-red-600'}">{truncateOutput(result.output)}</pre>
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>
