<!-- ui/src/lib/TerminalTray.svelte -->
<!-- Terminal tray showing project-scoped Claude Code instances -->
<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { Terminal } from 'xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import 'xterm/css/xterm.css';

  const API_BASE = 'http://localhost:3000';

  // Props
  let {
    isOpen = $bindable(false),
    onClose = () => {},
  } = $props<{
    isOpen?: boolean;
    onClose?: () => void;
  }>();

  // Types
  interface ClaudeInstance {
    id: string;
    projectPath: string;
    projectName: string;
    lines: { content: string; isStderr: boolean }[];
    isActive: boolean;
    terminal?: Terminal;
    fitAddon?: FitAddon;
  }

  interface WsEvent {
    type: string;
    [key: string]: any;
  }

  // State - keyed by project path
  let instances = $state<Map<string, ClaudeInstance>>(new Map());
  let expandedPath = $state<string | null>(null);
  let ws: WebSocket | null = null;
  let reconnectTimer: number | null = null;
  let terminalContainers: Map<string, HTMLDivElement> = new Map();
  let cleanupTimers: Map<string, number> = new Map();

  const WS_URL = 'ws://localhost:3000/ws';

  // Catppuccin Mocha theme for xterm
  const terminalTheme = {
    background: '#11111b',
    foreground: '#cdd6f4',
    cursor: '#f5e0dc',
    cursorAccent: '#11111b',
    selectionBackground: '#585b70',
    black: '#45475a',
    red: '#f38ba8',
    green: '#a6e3a1',
    yellow: '#f9e2af',
    blue: '#89b4fa',
    magenta: '#cba6f7',
    cyan: '#94e2d5',
    white: '#bac2de',
    brightBlack: '#585b70',
    brightRed: '#f38ba8',
    brightGreen: '#a6e3a1',
    brightYellow: '#f9e2af',
    brightBlue: '#89b4fa',
    brightMagenta: '#cba6f7',
    brightCyan: '#94e2d5',
    brightWhite: '#a6adc8',
  };

  // Derived state
  let instanceList = $derived(Array.from(instances.values()).sort((a, b) => a.projectName.localeCompare(b.projectName)));
  let activeCount = $derived(instanceList.filter(i => i.isActive).length);

  // Get project name from path
  function getProjectName(path: string): string {
    if (!path || path === 'tools') return 'Tool Calls';
    const parts = path.split('/');
    return parts[parts.length - 1] || path;
  }

  // Load existing instances from API
  async function loadInstances() {
    try {
      const res = await fetch(`${API_BASE}/api/claude/instances`);
      if (res.ok) {
        const data = await res.json();
        if (data.data) {
          for (const inst of data.data) {
            if (!instances.has(inst.project_path)) {
              instances.set(inst.project_path, {
                id: inst.id,
                projectPath: inst.project_path,
                projectName: getProjectName(inst.project_path),
                lines: [],
                isActive: inst.is_running,
              });
            }
          }
          instances = new Map(instances);
        }
      }
    } catch (e) {
      console.error('[Terminal] Failed to load instances:', e);
    }
  }

  // Connect to WebSocket
  function connect() {
    if (ws?.readyState === WebSocket.OPEN) return;

    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
      console.log('[Terminal] WebSocket connected');
      loadInstances();
    };

    ws.onmessage = (event) => {
      try {
        const data: WsEvent = JSON.parse(event.data);
        handleEvent(data);
      } catch (e) {
        console.error('[Terminal] Parse error:', e);
      }
    };

    ws.onclose = () => {
      console.log('[Terminal] WebSocket closed, reconnecting...');
      scheduleReconnect();
    };

    ws.onerror = (e) => {
      console.error('[Terminal] WebSocket error:', e);
    };
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, 2000);
  }

  // Clear all instances (used on reconnect)
  function clearAllInstances() {
    for (const instance of instances.values()) {
      instance.terminal?.dispose();
    }
    instances = new Map();
    expandedPath = null;
    for (const timer of cleanupTimers.values()) {
      clearTimeout(timer);
    }
    cleanupTimers.clear();
  }

  // Handle WebSocket events
  function handleEvent(event: WsEvent) {
    switch (event.type) {
      case 'claude_spawned': {
        const id = event.instance_id;
        const projectPath = event.working_dir || id;
        instances.set(projectPath, {
          id,
          projectPath,
          projectName: getProjectName(projectPath),
          lines: [],
          isActive: true,
        });
        instances = new Map(instances);
        expandedPath = projectPath;
        tick().then(() => initTerminal(projectPath));
        break;
      }

      case 'claude_stopped': {
        const id = event.instance_id;
        // Find instance by ID
        for (const [path, instance] of instances.entries()) {
          if (instance.id === id) {
            instance.isActive = false;
            instances = new Map(instances);
            break;
          }
        }
        break;
      }

      case 'terminal_output': {
        const id = event.instance_id;
        const content = event.content || '';
        const isStderr = event.is_stderr || false;

        // Find instance by ID
        for (const instance of instances.values()) {
          if (instance.id === id) {
            instance.lines.push({ content, isStderr });
            if (instance.lines.length > 1000) {
              instance.lines = instance.lines.slice(-1000);
            }
            instance.isActive = true;
            instances = new Map(instances);
            if (instance.terminal) {
              const text = isStderr ? `\x1b[31m${content}\x1b[0m` : content;
              instance.terminal.write(text);
            }
            break;
          }
        }
        break;
      }

      case 'tool_start': {
        // Show tool calls in a special "tools" instance
        if (!instances.has('tools')) {
          instances.set('tools', {
            id: 'tools',
            projectPath: 'tools',
            projectName: 'Tool Calls',
            lines: [],
            isActive: true,
          });
          instances = new Map(instances);
          expandedPath = 'tools';
          tick().then(() => initTerminal('tools'));
        }
        const toolsInstance = instances.get('tools');
        if (toolsInstance?.terminal) {
          const name = event.tool_name || event.name || 'unknown';
          toolsInstance.terminal.writeln(`\x1b[36m> ${name}\x1b[0m`);
        }
        break;
      }

      case 'tool_result': {
        const toolsInstance = instances.get('tools');
        if (toolsInstance?.terminal) {
          const name = event.tool_name || event.name || 'unknown';
          const success = event.success !== false;
          const color = success ? '32' : '31';
          const symbol = success ? 'ok' : 'err';
          toolsInstance.terminal.writeln(`\x1b[${color}m  ${symbol}\x1b[0m`);
        }
        break;
      }
    }
  }

  // Initialize xterm for an instance
  async function initTerminal(projectPath: string) {
    await tick();
    const container = terminalContainers.get(projectPath);
    const instance = instances.get(projectPath);
    if (!container || !instance || instance.terminal) return;

    const terminal = new Terminal({
      theme: terminalTheme,
      fontFamily: 'JetBrains Mono, Fira Code, monospace',
      fontSize: 12,
      lineHeight: 1.4,
      cursorBlink: false,
      disableStdin: true,
      convertEol: true,
      scrollback: 1000,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    terminal.loadAddon(fitAddon);
    terminal.loadAddon(webLinksAddon);
    terminal.open(container);
    fitAddon.fit();

    instance.terminal = terminal;
    instance.fitAddon = fitAddon;

    // Write existing lines
    for (const line of instance.lines) {
      const text = line.isStderr ? `\x1b[31m${line.content}\x1b[0m` : line.content;
      terminal.write(text);
    }
  }

  // Handle resize
  function handleResize() {
    for (const instance of instances.values()) {
      instance.fitAddon?.fit();
    }
  }

  // Toggle expanded
  function toggleExpanded(projectPath: string) {
    expandedPath = expandedPath === projectPath ? null : projectPath;
    tick().then(handleResize);
  }

  // Close instance via API
  async function closeInstance(projectPath: string) {
    // Cancel any pending cleanup timer
    const timer = cleanupTimers.get(projectPath);
    if (timer) {
      clearTimeout(timer);
      cleanupTimers.delete(projectPath);
    }

    // Dispose terminal
    const instance = instances.get(projectPath);
    if (instance?.terminal) {
      instance.terminal.dispose();
    }

    // Remove from local state
    instances.delete(projectPath);
    instances = new Map(instances);
    if (expandedPath === projectPath) {
      expandedPath = instances.keys().next().value || null;
    }

    // Call API to close (if it's a real project, not tools)
    if (projectPath !== 'tools') {
      try {
        await fetch(`${API_BASE}/api/claude/close`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ project_path: projectPath }),
        });
      } catch (e) {
        console.error('[Terminal] Failed to close instance:', e);
      }
    }
  }

  // Svelte action to bind terminal container refs
  function bindContainer(node: HTMLDivElement, projectPath: string) {
    terminalContainers.set(projectPath, node);
    initTerminal(projectPath);

    return {
      destroy() {
        terminalContainers.delete(projectPath);
      }
    };
  }

  onMount(() => {
    connect();
    window.addEventListener('resize', handleResize);
  });

  onDestroy(() => {
    if (reconnectTimer) clearTimeout(reconnectTimer);
    for (const timer of cleanupTimers.values()) {
      clearTimeout(timer);
    }
    cleanupTimers.clear();
    ws?.close();
    window.removeEventListener('resize', handleResize);
    for (const instance of instances.values()) {
      instance.terminal?.dispose();
    }
  });
</script>

<!-- Backdrop -->
{#if isOpen}
  <button
    class="fixed inset-0 z-40 cursor-default"
    style="background: rgba(17, 17, 27, 0.5);"
    onclick={() => onClose()}
    aria-label="Close terminal tray"
  ></button>
{/if}

<!-- Tray -->
<div
  class="fixed top-0 right-0 h-full w-[28rem] z-50 flex flex-col transition-transform duration-300"
  style="background: var(--glass-heavy); backdrop-filter: blur(24px); border-left: 1px solid var(--glass-border);"
  class:translate-x-0={isOpen}
  class:translate-x-full={!isOpen}
>
  <!-- Header -->
  <div class="px-4 py-3 flex items-center justify-between" style="border-bottom: 1px solid var(--glass-border);">
    <div class="flex items-center gap-2">
      <span class="text-xs font-semibold uppercase tracking-wider" style="color: var(--mauve);">Claude Instances</span>
      {#if instanceList.length > 0}
        <span class="px-2 py-0.5 rounded text-xs font-semibold" style="background: var(--accent-faded); color: var(--accent);">
          {instanceList.length}
        </span>
      {/if}
    </div>
    <button
      class="p-1.5 rounded-lg transition-colors hover:bg-[var(--surface0)]"
      style="color: var(--muted);"
      onclick={() => onClose()}
      aria-label="Close terminal tray"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M12 4L4 12M4 4l8 8"/>
      </svg>
    </button>
  </div>

  <!-- Instance list -->
  <div class="flex-1 overflow-y-auto">
    {#if instanceList.length === 0}
      <div class="p-6 text-center">
        <div class="text-2xl mb-2" style="color: var(--overlay0);">C</div>
        <p class="text-sm" style="color: var(--muted);">No Claude instances running</p>
        <p class="text-xs mt-1" style="color: var(--overlay0);">Instances will appear when Mira sends tasks</p>
      </div>
    {:else}
      {#each instanceList as instance (instance.projectPath)}
        <div style="border-bottom: 1px solid var(--glass-border);">
          <!-- Instance header row -->
          <div class="flex items-center px-4 py-3 transition-colors hover:bg-[var(--surface0)]">
            <!-- Clickable expand area -->
            <button
              class="flex-1 flex items-center gap-3 text-left"
              onclick={() => toggleExpanded(instance.projectPath)}
            >
              <!-- Status dot -->
              <div
                class="w-2 h-2 rounded-full flex-shrink-0"
                style="background: {instance.isActive ? 'var(--green)' : 'var(--overlay0)'};"
                class:animate-pulse={instance.isActive}
              ></div>

              <!-- Info -->
              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium truncate" style="color: var(--foreground);">
                  {instance.projectName}
                </div>
                {#if instance.projectPath !== 'tools'}
                  <div class="text-xs truncate" style="color: var(--muted);">
                    {instance.projectPath}
                  </div>
                {/if}
              </div>

              <!-- Chevron -->
              <svg
                class="w-4 h-4 transition-transform flex-shrink-0"
                style="color: var(--muted);"
                class:rotate-90={expandedPath === instance.projectPath}
                viewBox="0 0 16 16"
                fill="currentColor"
              >
                <path d="M6 4l4 4-4 4"/>
              </svg>
            </button>

            <!-- Close button -->
            <button
              class="p-1 ml-2 rounded hover:bg-[var(--surface1)] flex-shrink-0"
              style="color: var(--muted);"
              onclick={() => closeInstance(instance.projectPath)}
              aria-label="Close instance"
              title="Close Claude for this project"
            >
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 4L4 12M4 4l8 8"/>
              </svg>
            </button>
          </div>

          <!-- Terminal content -->
          {#if expandedPath === instance.projectPath}
            <div
              class="h-64"
              style="background: var(--crust);"
              use:bindContainer={instance.projectPath}
            ></div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Footer status -->
  {#if activeCount > 0}
    <div class="px-4 py-3 flex items-center gap-2 text-xs" style="border-top: 1px solid var(--glass-border); color: var(--success);">
      <div class="w-2 h-2 rounded-full animate-pulse" style="background: var(--green);"></div>
      <span>{activeCount} active</span>
    </div>
  {/if}
</div>

<style>
  /* xterm container styling */
  :global(.xterm) {
    padding: 8px;
  }

  :global(.xterm-viewport) {
    overflow-y: auto !important;
  }

  :global(.xterm-viewport::-webkit-scrollbar) {
    width: 6px;
  }

  :global(.xterm-viewport::-webkit-scrollbar-thumb) {
    background: var(--surface1);
    border-radius: 3px;
  }

  .rotate-90 {
    transform: rotate(90deg);
  }
</style>
