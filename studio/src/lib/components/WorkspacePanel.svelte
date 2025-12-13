<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  interface WorkspaceEntry {
    id: string;
    type: 'command' | 'output' | 'file' | 'diff' | 'info' | 'memory' | 'context';
    content: string;
    timestamp: Date;
    status?: 'running' | 'success' | 'error';
    file?: string;
    tool?: string;
  }

  let entries = $state<WorkspaceEntry[]>([]);
  let connected = $state(false);
  let eventSource: EventSource | null = null;

  onMount(() => {
    connectToEvents();
  });

  onDestroy(() => {
    if (eventSource) {
      eventSource.close();
    }
  });

  function connectToEvents() {
    eventSource = new EventSource('/api/workspace/events');

    eventSource.onopen = () => {
      connected = true;
    };

    eventSource.onerror = () => {
      connected = false;
      // Try to reconnect after 2 seconds
      setTimeout(() => {
        if (eventSource) eventSource.close();
        connectToEvents();
      }, 2000);
    };

    eventSource.onmessage = (event) => {
      if (event.data === 'ping') return;

      try {
        const data = JSON.parse(event.data);
        handleEvent(data);
      } catch (e) {
        console.error('Failed to parse workspace event:', e);
      }
    };
  }

  function handleEvent(event: any) {
    const id = crypto.randomUUID();
    const timestamp = new Date();

    switch (event.type) {
      case 'info':
        addEntry({ id, type: 'info', content: event.message, timestamp });
        break;

      case 'tool_start':
        addEntry({
          id,
          type: 'command',
          content: event.args ? `${event.tool}(${event.args})` : event.tool,
          timestamp,
          status: 'running',
          tool: event.tool
        });
        break;

      case 'tool_end':
        // Update the running entry or add new one
        const result = event.result || (event.success ? 'done' : 'failed');
        addEntry({
          id,
          type: 'output',
          content: `└─ ${result}`,
          timestamp,
          status: event.success ? 'success' : 'error',
          tool: event.tool
        });
        break;

      case 'memory':
        addEntry({
          id,
          type: 'memory',
          content: `${event.action}: ${event.content}`,
          timestamp
        });
        break;

      case 'context':
        addEntry({
          id,
          type: 'context',
          content: `loaded ${event.count} ${event.kind}`,
          timestamp
        });
        break;

      case 'file':
        addEntry({
          id,
          type: 'file',
          content: event.action,
          timestamp,
          file: event.path
        });
        break;
    }
  }

  function addEntry(entry: WorkspaceEntry) {
    entries = [...entries.slice(-50), entry]; // Keep last 50 entries
  }

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
      case 'memory': return '🧠';
      case 'context': return '📚';
      default: return '•';
    }
  }

  function formatTime(date: Date): string {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
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
    {#if entries.length === 0}
      <div class="text-gray-500 italic">Waiting for activity...</div>
    {/if}

    {#each entries as entry (entry.id)}
      <div class="mb-1.5 {getStatusColor(entry.status)} group">
        <div class="flex items-start gap-2">
          <span class="text-gray-600 select-none w-4 flex-shrink-0">{getTypeIcon(entry.type)}</span>
          <div class="flex-1 min-w-0">
            {#if entry.file}
              <span class="text-[var(--terminal-accent)]">{entry.file}</span>
              <br />
            {/if}
            <span class="whitespace-pre-wrap break-all">{entry.content}</span>
          </div>
          <span class="text-gray-600 text-xs opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
            {formatTime(entry.timestamp)}
          </span>
        </div>
      </div>
    {/each}

    <!-- Blinking cursor -->
    <div class="flex items-center gap-2 mt-4">
      <span class="text-gray-600 select-none w-4">›</span>
      <span class="w-2 h-4 bg-[var(--terminal-accent)] animate-pulse"></span>
    </div>
  </div>

  <!-- Footer status bar -->
  <footer class="flex items-center justify-between px-4 py-2 border-t border-[var(--terminal-border)] text-xs text-gray-500 font-mono">
    <div class="flex items-center gap-4">
      <span class="flex items-center gap-1">
        {#if connected}
          <span class="w-2 h-2 rounded-full bg-[var(--terminal-success)]"></span>
          connected
        {:else}
          <span class="w-2 h-2 rounded-full bg-[var(--terminal-warning)] animate-pulse"></span>
          reconnecting...
        {/if}
      </span>
      <span>{entries.length} events</span>
    </div>
    <div>
      mira://studio
    </div>
  </footer>
</div>
