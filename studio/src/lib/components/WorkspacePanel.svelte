<script lang="ts">
  interface WorkspaceEntry {
    id: string;
    type: 'command' | 'output' | 'file' | 'diff' | 'info';
    content: string;
    timestamp: Date;
    status?: 'running' | 'success' | 'error';
    file?: string;
  }

  // Demo entries to show the aesthetic
  let entries = $state<WorkspaceEntry[]>([
    {
      id: '1',
      type: 'info',
      content: 'Workspace ready. Waiting for activity...',
      timestamp: new Date()
    }
  ]);

  function getStatusColor(status?: string) {
    switch (status) {
      case 'running': return 'text-[var(--terminal-accent)]';
      case 'success': return 'text-[var(--terminal-success)]';
      case 'error': return 'text-[var(--terminal-error)]';
      default: return 'text-[var(--terminal-text)]';
    }
  }

  function getTypeIcon(type: string) {
    switch (type) {
      case 'command': return '›';
      case 'output': return ' ';
      case 'file': return '📄';
      case 'diff': return '±';
      case 'info': return 'ℹ';
      default: return '•';
    }
  }
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <header class="flex items-center justify-between px-4 py-3 border-b border-[var(--terminal-border)]">
    <div class="flex items-center gap-2">
      <div class="flex gap-1.5">
        <div class="w-3 h-3 rounded-full bg-[#ff5f56]"></div>
        <div class="w-3 h-3 rounded-full bg-[#ffbd2e]"></div>
        <div class="w-3 h-3 rounded-full bg-[#27c93f]"></div>
      </div>
      <span class="ml-3 text-sm font-medium text-[var(--terminal-text)] font-mono">workspace</span>
    </div>
    <div class="text-xs text-gray-500 font-mono">
      mira://studio
    </div>
  </header>

  <!-- Terminal output -->
  <div class="flex-1 overflow-y-auto terminal-scroll p-4 font-mono text-sm">
    {#each entries as entry (entry.id)}
      <div class="mb-2 {getStatusColor(entry.status)}">
        <div class="flex items-start gap-2">
          <span class="text-gray-500 select-none w-4">{getTypeIcon(entry.type)}</span>
          <div class="flex-1">
            {#if entry.file}
              <span class="text-[var(--terminal-accent)]">{entry.file}</span>
              <br />
            {/if}
            <span class="whitespace-pre-wrap">{entry.content}</span>
          </div>
        </div>
      </div>
    {/each}

    <!-- Blinking cursor -->
    <div class="flex items-center gap-2 mt-4">
      <span class="text-gray-500 select-none w-4">›</span>
      <span class="w-2 h-4 bg-[var(--terminal-accent)] animate-pulse"></span>
    </div>
  </div>

  <!-- Footer status bar -->
  <footer class="flex items-center justify-between px-4 py-2 border-t border-[var(--terminal-border)] text-xs text-gray-500 font-mono">
    <div class="flex items-center gap-4">
      <span class="flex items-center gap-1">
        <span class="w-2 h-2 rounded-full bg-[var(--terminal-success)]"></span>
        connected
      </span>
      <span>claude code: ready</span>
    </div>
    <div>
      {new Date().toLocaleTimeString()}
    </div>
  </footer>
</div>
