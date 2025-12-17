<script lang="ts">
  interface Props {
    path: string;
    content: string;
    language?: string;
  }

  let { path, content, language }: Props = $props();
  let isExpanded = $state(false);

  function getLanguageFromPath(path: string): string {
    const ext = path.split('.').pop()?.toLowerCase() || '';
    const langMap: Record<string, string> = {
      rs: 'rust',
      ts: 'typescript',
      js: 'javascript',
      py: 'python',
      go: 'go',
      rb: 'ruby',
      java: 'java',
      c: 'c',
      cpp: 'cpp',
      h: 'c',
      hpp: 'cpp',
      md: 'markdown',
      json: 'json',
      yaml: 'yaml',
      yml: 'yaml',
      toml: 'toml',
      sql: 'sql',
      sh: 'bash',
      bash: 'bash',
      zsh: 'bash',
      svelte: 'svelte',
      html: 'html',
      css: 'css',
      scss: 'scss',
    };
    return langMap[ext] || 'text';
  }

  function truncateContent(content: string, maxLines = 50): { text: string; truncated: boolean } {
    const lines = content.split('\n');
    if (lines.length <= maxLines) {
      return { text: content, truncated: false };
    }
    return {
      text: lines.slice(0, maxLines).join('\n') + '\n...',
      truncated: true,
    };
  }

  let displayLang = $derived(language || getLanguageFromPath(path));
  let truncateResult = $derived(truncateContent(content));
  let displayContent = $derived(truncateResult.text);
  let truncated = $derived(truncateResult.truncated);
</script>

<div class="file-content-block border border-gray-200 rounded-lg my-2 overflow-hidden">
  <button
    class="w-full flex items-center justify-between p-2 bg-gray-100 hover:bg-gray-200 transition-colors text-left"
    onclick={() => isExpanded = !isExpanded}
  >
    <div class="flex items-center gap-2">
      <span class="text-gray-500">📄</span>
      <span class="font-mono text-sm">{path}</span>
      <span class="text-xs text-gray-400">{displayLang}</span>
    </div>
    <span class="text-gray-400 text-sm">{isExpanded ? '−' : '+'}</span>
  </button>

  {#if isExpanded}
    <div class="p-2 overflow-x-auto max-h-96 bg-gray-50">
      <pre class="text-xs font-mono whitespace-pre">{displayContent}</pre>
      {#if truncated}
        <div class="text-xs text-gray-400 mt-2">Content truncated. Full file has more lines.</div>
      {/if}
    </div>
  {/if}
</div>
