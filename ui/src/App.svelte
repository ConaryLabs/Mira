<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { marked } from 'marked';
  import hljs from 'highlight.js';
  import 'highlight.js/styles/github-dark.css';
  import TerminalTray from './lib/TerminalTray.svelte';
  import ProjectsView from './lib/ProjectsView.svelte';

  const API_BASE = 'http://localhost:3000';
  const WS_URL = 'ws://localhost:3000/ws';

  // Types
  interface ChatMessage {
    id: number;
    role: 'user' | 'assistant';
    content: string;
    timestamp: string;
    toolCalls?: ToolCall[];
  }

  interface ToolCall {
    name: string;
    success: boolean;
    args?: Record<string, any>;
    result?: string;
  }

  interface Project {
    name: string;
    path: string;
  }

  // State
  let messages = $state<ChatMessage[]>([]);
  let input = $state('');
  let loading = $state(false);
  let isThinking = $state(false);
  let currentTool = $state<string | null>(null);
  let messageIdCounter = $state(0);
  let pendingToolCalls = $state<ToolCall[]>([]);

  // Project state
  let projects = $state<Project[]>([]);
  let currentProject = $state<Project | null>(null);
  let sidebarOpen = $state(false);

  // View state
  let activeView = $state<'chat' | 'projects'>('chat');

  // Server status
  let serverStatus = $state<'checking' | 'connected' | 'disconnected'>('checking');

  // Terminal tray state
  let terminalTrayOpen = $state(false);

  // Tool expansion state (tracks which message's tools are expanded)
  let expandedToolsMessageId = $state<number | null>(null);

  // Claude instances (for indicator)
  let claudeInstances = $state<Map<string, { id: string; active: boolean }>>(new Map());
  let ws: WebSocket | null = null;
  let wsReconnectTimer: number | null = null;

  // Derived: active Claude count
  let activeClaudeCount = $derived(Array.from(claudeInstances.values()).filter(i => i.active).length);

  // Refs
  let messagesContainer: HTMLDivElement;
  let inputEl: HTMLInputElement;

  // Configure marked
  marked.setOptions({
    highlight: (code, lang) => {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(code, { language: lang }).value;
      }
      return hljs.highlightAuto(code).value;
    },
    breaks: true,
    gfm: true,
  });

  // Get current time
  function getTime(): string {
    const now = new Date();
    return `${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}`;
  }

  // Format UTC timestamp from DB to local time
  function formatTimestamp(utcTimestamp: string | undefined): string {
    if (!utcTimestamp) return '';
    try {
      // DB stores as "2026-01-03 04:15:37" (UTC, no timezone indicator)
      // Parse as UTC and convert to local
      const utcDate = new Date(utcTimestamp.replace(' ', 'T') + 'Z');
      return `${utcDate.getHours().toString().padStart(2, '0')}:${utcDate.getMinutes().toString().padStart(2, '0')}`;
    } catch {
      return '';
    }
  }

  // Scroll to bottom
  async function scrollToBottom() {
    await tick();
    if (messagesContainer) {
      messagesContainer.scrollTop = messagesContainer.scrollHeight;
    }
  }

  // Load initial data
  onMount(async () => {
    // Check server health
    try {
      const res = await fetch(`${API_BASE}/api/health`);
      if (res.ok) {
        serverStatus = 'connected';
      } else {
        serverStatus = 'disconnected';
      }
    } catch {
      serverStatus = 'disconnected';
    }

    // Load projects
    try {
      const res = await fetch(`${API_BASE}/api/projects`);
      if (res.ok) {
        const data = await res.json();
        projects = data.data || [];
      }
    } catch {}

    // Load current project
    try {
      const res = await fetch(`${API_BASE}/api/project`);
      if (res.ok) {
        const data = await res.json();
        currentProject = data.data || null;
      }
    } catch {}

    // Load chat history
    try {
      const res = await fetch(`${API_BASE}/api/chat/history`);
      if (res.ok) {
        const data = await res.json();
        if (data.data) {
          messages = data.data.map((msg: any, i: number) => ({
            id: i + 1,
            role: msg.role,
            content: msg.content,
            timestamp: formatTimestamp(msg.timestamp),
            toolCalls: [],
          }));
          messageIdCounter = messages.length;
          scrollToBottom();
        }
      }
    } catch {}

    // Focus input
    inputEl?.focus();

    // Connect WebSocket for Claude events
    connectWs();
  });

  // Connect WebSocket
  function connectWs() {
    if (ws?.readyState === WebSocket.OPEN) return;

    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
      console.log('[WS] Connected');
    };

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        handleWsEvent(data);
      } catch {}
    };

    ws.onclose = () => {
      console.log('[WS] Disconnected');
      scheduleWsReconnect();
    };

    ws.onerror = () => {};
  }

  function scheduleWsReconnect() {
    if (wsReconnectTimer) return;
    wsReconnectTimer = window.setTimeout(() => {
      wsReconnectTimer = null;
      connectWs();
    }, 2000);
  }

  function handleWsEvent(event: any) {
    switch (event.type) {
      case 'claude_spawned':
        claudeInstances.set(event.instance_id, { id: event.instance_id, active: true });
        claudeInstances = new Map(claudeInstances);
        break;
      case 'claude_stopped':
        const inst = claudeInstances.get(event.instance_id);
        if (inst) {
          inst.active = false;
          claudeInstances = new Map(claudeInstances);
          // Auto-cleanup after 30 seconds
          setTimeout(() => {
            const instance = claudeInstances.get(event.instance_id);
            if (instance && !instance.active) {
              claudeInstances.delete(event.instance_id);
              claudeInstances = new Map(claudeInstances);
            }
          }, 30000);
        }
        break;
      case 'tool_start':
        // Track tool usage as an implicit "tools" instance
        if (!claudeInstances.has('tools')) {
          claudeInstances.set('tools', { id: 'tools', active: true });
          claudeInstances = new Map(claudeInstances);
        }
        break;
      case 'tool_result':
        // Mark tools instance as inactive after tool completes
        const toolsInst = claudeInstances.get('tools');
        if (toolsInst) {
          toolsInst.active = false;
          claudeInstances = new Map(claudeInstances);
          // Auto-cleanup after 10 seconds (shorter for tools)
          setTimeout(() => {
            const ti = claudeInstances.get('tools');
            if (ti && !ti.active) {
              claudeInstances.delete('tools');
              claudeInstances = new Map(claudeInstances);
            }
          }, 10000);
        }
        break;
    }
  }

  onDestroy(() => {
    if (wsReconnectTimer) clearTimeout(wsReconnectTimer);
    ws?.close();
  });

  // Send message
  async function sendMessage() {
    if (!input.trim() || loading) return;

    const userMessage = input.trim();
    input = '';
    loading = true;
    isThinking = false;
    currentTool = null;
    pendingToolCalls = [];

    // Add user message
    messageIdCounter++;
    messages = [...messages, {
      id: messageIdCounter,
      role: 'user',
      content: userMessage,
      timestamp: getTime(),
    }];

    // Add assistant placeholder
    messageIdCounter++;
    const assistantId = messageIdCounter;
    messages = [...messages, {
      id: assistantId,
      role: 'assistant',
      content: '',
      timestamp: getTime(),
    }];

    await scrollToBottom();

    // Stream response
    try {
      const res = await fetch(`${API_BASE}/api/chat/stream`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: userMessage }),
      });

      if (!res.ok) {
        updateLastMessage(`Error: HTTP ${res.status}`);
        loading = false;
        return;
      }

      const reader = res.body?.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (reader) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        // Process SSE events
        while (buffer.includes('\n\n')) {
          const pos = buffer.indexOf('\n\n');
          const eventStr = buffer.slice(0, pos);
          buffer = buffer.slice(pos + 2);

          for (const line of eventStr.split('\n')) {
            if (line.startsWith('data: ')) {
              const data = line.slice(6);
              try {
                const event = JSON.parse(data);
                handleSSEEvent(event);
              } catch {}
            }
          }
        }
      }
    } catch (e) {
      updateLastMessage(`Connection error: ${e}`);
    }

    loading = false;
    isThinking = false;
    currentTool = null;
    inputEl?.focus();
  }

  // Handle SSE event
  function handleSSEEvent(event: any) {
    switch (event.type) {
      case 'start':
        isThinking = true;
        break;
      case 'delta':
        isThinking = false;
        appendToLastMessage(event.content);
        scrollToBottom();
        break;
      case 'tool_start':
        currentTool = event.name;
        pendingToolCalls = [...pendingToolCalls, { name: event.name, success: true }];
        break;
      case 'tool_result':
        currentTool = null;
        pendingToolCalls = pendingToolCalls.map(tc =>
          tc.name === event.name ? { ...tc, success: event.success } : tc
        );
        break;
      case 'done':
        updateLastMessage(event.content, pendingToolCalls);
        pendingToolCalls = [];
        break;
      case 'error':
        updateLastMessage(`Error: ${event.message}`);
        break;
    }
  }

  // Update last message
  function updateLastMessage(content: string, toolCalls?: ToolCall[]) {
    messages = messages.map((msg, i) =>
      i === messages.length - 1 ? { ...msg, content, toolCalls } : msg
    );
  }

  // Append to last message
  function appendToLastMessage(content: string) {
    messages = messages.map((msg, i) =>
      i === messages.length - 1 ? { ...msg, content: msg.content + content } : msg
    );
  }

  // Set project
  async function setProject(proj: Project) {
    try {
      const res = await fetch(`${API_BASE}/api/project/set`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: proj.path, name: proj.name }),
      });
      if (res.ok) {
        const data = await res.json();
        currentProject = data.data;
      }
    } catch {}
    sidebarOpen = false;
  }

  // Render markdown
  function renderMarkdown(content: string): string {
    return marked.parse(content) as string;
  }
</script>

<div class="h-screen flex flex-col" style="background: var(--base);">
  <!-- Sidebar backdrop -->
  {#if sidebarOpen}
    <button
      class="fixed inset-0 z-40 cursor-default"
      style="background: rgba(17, 17, 27, 0.7); backdrop-filter: blur(4px);"
      onclick={() => sidebarOpen = false}
      aria-label="Close sidebar"
    ></button>
  {/if}

  <!-- Sidebar -->
  <div
    class="fixed top-0 left-0 h-full w-80 z-50 flex flex-col transition-transform duration-300"
    style="background: var(--glass-heavy); backdrop-filter: blur(24px); border-right: 1px solid var(--glass-border);"
    class:translate-x-0={sidebarOpen}
    class:-translate-x-full={!sidebarOpen}
  >
    <div class="p-4 flex items-center justify-between" style="border-bottom: 1px solid var(--glass-border);">
      <span class="text-xs font-semibold uppercase tracking-wider" style="color: var(--accent);">Projects</span>
      <button
        class="p-1.5 rounded-lg transition-colors"
        style="color: var(--muted);"
        onclick={() => sidebarOpen = false}
        aria-label="Close sidebar"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 4L4 12M4 4l8 8"/>
        </svg>
      </button>
    </div>
    <div class="flex-1 overflow-y-auto py-2">
      {#each projects as proj}
        <button
          class="w-full text-left px-4 py-3 transition-colors"
          style="border-left: 3px solid {currentProject?.path === proj.path ? 'var(--accent)' : 'transparent'}; background: {currentProject?.path === proj.path ? 'var(--accent-faded)' : 'transparent'};"
          onclick={() => setProject(proj)}
        >
          <div class="text-sm font-medium" style="color: var(--foreground);">{proj.name}</div>
          <div class="text-xs truncate" style="color: var(--muted);">{proj.path}</div>
        </button>
      {/each}
      {#if projects.length === 0}
        <div class="px-4 py-8 text-center text-sm" style="color: var(--muted);">
          No projects found
        </div>
      {/if}
    </div>
  </div>

  <!-- Header -->
  <header
    class="flex items-center gap-3 px-5 py-3"
    style="background: var(--glass-heavy); backdrop-filter: blur(16px); border-bottom: 1px solid var(--glass-border);"
  >
    <!-- Project selector button -->
    <button
      class="w-9 h-9 rounded-lg flex items-center justify-center transition-colors"
      style="border: 1px solid var(--border-subtle); color: var(--muted);"
      onclick={() => sidebarOpen = true}
      aria-label="Open sidebar"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M2 4h12M2 8h12M2 12h12"/>
      </svg>
    </button>

    <!-- Current project indicator -->
    <div class="project-indicator">
      <div class="project-dot" class:active={!!currentProject}></div>
      <span class="project-label">
        {#if currentProject}
          {currentProject.name}
        {:else}
          No Project
        {/if}
      </span>
    </div>

    <!-- Navigation tabs -->
    <nav class="nav-tabs">
      <button
        class="nav-tab"
        class:active={activeView === 'chat'}
        onclick={() => activeView = 'chat'}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
          <path d="M2 3a1 1 0 011-1h10a1 1 0 011 1v8a1 1 0 01-1 1H5l-3 3V3z"/>
        </svg>
        <span>Chat</span>
      </button>
      <button
        class="nav-tab"
        class:active={activeView === 'projects'}
        onclick={() => activeView = 'projects'}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
          <path d="M1 4a1 1 0 011-1h3.5l1 1H14a1 1 0 011 1v8a1 1 0 01-1 1H2a1 1 0 01-1-1V4z"/>
        </svg>
        <span>Projects</span>
      </button>
    </nav>

    <div class="ml-auto flex items-center gap-3">
      <!-- Claude Indicator -->
      {#if claudeInstances.size > 0}
        <button
          class="flex items-center gap-2 px-3 py-1.5 rounded-lg transition-colors"
          style="background: {activeClaudeCount > 0 ? 'var(--success-faded)' : 'var(--surface0)'}; border: 1px solid {activeClaudeCount > 0 ? 'var(--green)' : 'var(--border-subtle)'};"
          onclick={() => terminalTrayOpen = true}
        >
          {#if activeClaudeCount > 0}
            <div class="w-2 h-2 rounded-full animate-pulse" style="background: var(--green);"></div>
          {:else}
            <div class="w-2 h-2 rounded-full" style="background: var(--overlay0);"></div>
          {/if}
          <span class="text-xs font-medium" style="color: {activeClaudeCount > 0 ? 'var(--green)' : 'var(--muted)'};">
            {claudeInstances.size} {claudeInstances.size === 1 ? 'instance' : 'instances'}
          </span>
          <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor" style="color: var(--muted);">
            <path d="M6 4l4 4-4 4"/>
          </svg>
        </button>
      {/if}

      <div class="flex items-center gap-2">
        <div
          class="w-2 h-2 rounded-full"
          style="background: {serverStatus === 'connected' ? 'var(--green)' : serverStatus === 'checking' ? 'var(--yellow)' : 'var(--red)'};"
          class:animate-pulse={serverStatus === 'checking'}
        ></div>
        <span class="text-xs" style="color: var(--muted);">
          {serverStatus === 'connected' ? 'Connected' : serverStatus === 'checking' ? 'Connecting...' : 'Disconnected'}
        </span>
      </div>
      <span class="text-xs" style="color: var(--muted);">DeepSeek Reasoner</span>
    </div>
  </header>

  <!-- Main Content Area -->
  {#if activeView === 'chat'}
    <!-- Messages -->
    <div class="flex-1 overflow-y-auto" bind:this={messagesContainer}>
      {#if messages.length === 0 && !loading}
        <div class="h-full flex items-center justify-center">
          <div class="text-center py-12">
            <div class="text-4xl mb-4" style="color: var(--overlay0);">&gt;</div>
            <p class="mb-2" style="color: var(--muted);">Start a conversation</p>
            <p class="text-xs max-w-md" style="color: var(--overlay0);">
              I can search memories, code, manage tasks, and spawn Claude Code for file operations.
            </p>
          </div>
        </div>
      {:else}
        <div class="flex flex-col gap-5 p-6 max-w-4xl mx-auto">
          {#each messages as msg (msg.id)}
            <div class="flex gap-3.5 {msg.role === 'user' ? 'flex-row-reverse ml-auto' : 'mr-auto'}" style="max-width: 85%;">
              <!-- Avatar -->
              <div
                class="w-9 h-9 rounded-lg flex items-center justify-center text-xs font-semibold flex-shrink-0"
                style="background: linear-gradient(135deg, {msg.role === 'user' ? 'var(--blue), var(--sapphire)' : 'var(--mauve), var(--pink)'}); color: var(--crust);"
              >
                {msg.role === 'user' ? 'U' : 'M'}
              </div>

              <!-- Bubble -->
              <div
                class="rounded-xl overflow-hidden shadow-sm"
                style="background: var(--card); border: 1px solid {msg.role === 'user' ? 'rgba(137, 180, 250, 0.3)' : 'rgba(203, 166, 247, 0.2)'};"
              >
                <!-- Header -->
                <div class="px-4 py-2.5 flex items-center gap-2 text-xs" style="border-bottom: 1px solid var(--border-subtle); color: var(--muted);">
                  <span class="font-semibold uppercase tracking-wide" style="color: {msg.role === 'user' ? 'var(--blue)' : 'var(--mauve)'};">
                    {msg.role === 'user' ? 'You' : 'Mira'}
                  </span>
                  {#if msg.timestamp}
                    <span class="ml-auto" style="color: var(--overlay0);">{msg.timestamp}</span>
                  {/if}
                </div>

                <!-- Content -->
                <div class="p-4 leading-relaxed prose prose-invert prose-sm max-w-none">
                  {#if msg.content}
                    {@html renderMarkdown(msg.content)}
                  {:else if loading && msg.role === 'assistant'}
                    <!-- Loading state -->
                    {#if isThinking}
                      <div class="flex items-center gap-2.5 text-xs" style="color: var(--muted);">
                        <div class="w-4 h-4 rounded-full border-2 animate-spin" style="border-color: var(--surface1); border-top-color: var(--mauve);"></div>
                        <span>Reasoning...</span>
                      </div>
                    {:else if currentTool}
                      <div class="flex items-center gap-2.5 text-xs" style="color: var(--muted);">
                        <div class="w-4 h-4 rounded-full border-2 animate-spin" style="border-color: var(--surface1); border-top-color: var(--mauve);"></div>
                        <span>Using {currentTool}...</span>
                      </div>
                    {:else}
                      <div class="flex gap-1 py-2">
                        <div class="w-2 h-2 rounded-full animate-bounce" style="background: var(--overlay1); animation-delay: 0s;"></div>
                        <div class="w-2 h-2 rounded-full animate-bounce" style="background: var(--overlay1); animation-delay: 0.2s;"></div>
                        <div class="w-2 h-2 rounded-full animate-bounce" style="background: var(--overlay1); animation-delay: 0.4s;"></div>
                      </div>
                    {/if}
                  {/if}
                </div>

                <!-- Tool calls -->
                {#if msg.toolCalls && msg.toolCalls.length > 0}
                  <div style="border-top: 1px solid var(--border-subtle);">
                    <button
                      class="w-full px-4 py-2.5 flex items-center gap-2 text-xs cursor-pointer hover:bg-[var(--surface0)] transition-colors"
                      style="color: var(--muted);"
                      onclick={() => expandedToolsMessageId = expandedToolsMessageId === msg.id ? null : msg.id}
                    >
                      <svg
                        width="16" height="16" viewBox="0 0 16 16" fill="currentColor"
                        class="transition-transform"
                        class:rotate-90={expandedToolsMessageId === msg.id}
                      >
                        <path d="M6 4l4 4-4 4"/>
                      </svg>
                      <span class="font-medium uppercase tracking-wide">Tools</span>
                      <span class="px-2 py-0.5 rounded text-xs font-semibold" style="background: var(--accent-faded); color: var(--accent);">
                        {msg.toolCalls.length}
                      </span>
                    </button>
                    {#if expandedToolsMessageId === msg.id}
                      <div class="px-4 pb-3 space-y-2">
                        {#each msg.toolCalls as tool}
                          <div class="flex items-center gap-2 text-xs py-1.5 px-3 rounded-lg" style="background: var(--surface0);">
                            <div
                              class="w-2 h-2 rounded-full flex-shrink-0"
                              style="background: {tool.success ? 'var(--green)' : 'var(--red)'};"
                            ></div>
                            <span class="font-mono" style="color: var(--foreground);">{tool.name}</span>
                            <span style="color: {tool.success ? 'var(--green)' : 'var(--red)'};">
                              {tool.success ? 'success' : 'failed'}
                            </span>
                          </div>
                        {/each}
                      </div>
                    {/if}
                  </div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Input -->
    <div
      class="px-5 py-4"
      style="background: var(--glass-heavy); backdrop-filter: blur(16px); border-top: 1px solid var(--glass-border);"
    >
      <div class="flex gap-3 max-w-4xl mx-auto">
        <input
          type="text"
          placeholder="Ask anything..."
          class="flex-1 px-4 py-3 rounded-xl outline-none transition-all"
          style="background: var(--mantle); border: 1px solid var(--border-subtle); color: var(--foreground);"
          bind:this={inputEl}
          bind:value={input}
          disabled={loading}
          onkeydown={(e) => e.key === 'Enter' && !e.shiftKey && sendMessage()}
          onfocus={(e) => e.currentTarget.style.borderColor = 'var(--accent)'}
          onblur={(e) => e.currentTarget.style.borderColor = 'var(--border-subtle)'}
        />
        <button
          class="px-5 rounded-xl font-semibold transition-all"
          style="background: linear-gradient(135deg, var(--blue), var(--sapphire)); color: var(--crust);"
          disabled={loading || !input.trim()}
          onclick={sendMessage}
          class:opacity-50={loading || !input.trim()}
          class:cursor-not-allowed={loading || !input.trim()}
        >
          Send
        </button>
      </div>
    </div>
  {:else if activeView === 'projects'}
    <!-- Projects View -->
    <ProjectsView {currentProject} onProjectChange={(p) => currentProject = p} />
  {/if}

  <!-- Terminal Tray -->
  <TerminalTray bind:isOpen={terminalTrayOpen} onClose={() => terminalTrayOpen = false} />
</div>

<style>
  /* Tailwind prose overrides for markdown */
  :global(.prose) {
    color: var(--foreground);
  }

  :global(.prose p) {
    margin: 0.625rem 0;
  }

  :global(.prose p:first-child) {
    margin-top: 0;
  }

  :global(.prose p:last-child) {
    margin-bottom: 0;
  }

  :global(.prose code:not(pre code)) {
    background: var(--accent-faded);
    color: var(--accent);
    padding: 0.125rem 0.5rem;
    border-radius: 6px;
    font-family: var(--font-mono);
    font-size: 0.875em;
  }

  :global(.prose pre) {
    background: var(--crust);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 1rem;
    overflow-x: auto;
    margin: 1rem 0;
  }

  :global(.prose pre code) {
    background: transparent;
    padding: 0;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    line-height: 1.6;
  }

  :global(.prose a) {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  :global(.prose ul, .prose ol) {
    margin: 0.625rem 0;
    padding-left: 1.5rem;
  }

  :global(.prose li) {
    margin: 0.375rem 0;
  }

  :global(.prose blockquote) {
    margin: 1rem 0;
    padding: 0.75rem 1rem;
    border-left: 3px solid var(--mauve);
    background: rgba(203, 166, 247, 0.08);
    border-radius: 0 8px 8px 0;
    color: var(--subtext1);
  }

  :global(.prose strong) {
    font-weight: 600;
    color: var(--foreground);
  }

  /* Animation for bouncing dots */
  @keyframes bounce {
    0%, 60%, 100% { transform: translateY(0); }
    30% { transform: translateY(-6px); }
  }

  .animate-bounce {
    animation: bounce 1.4s ease-in-out infinite;
  }

  /* Project indicator in header */
  .project-indicator {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.75rem;
    background: var(--surface0);
    border-radius: 8px;
    border: 1px solid var(--glass-border);
  }

  .project-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--overlay0);
    transition: all 0.2s;
  }

  .project-dot.active {
    background: var(--green);
    box-shadow: 0 0 6px var(--green);
  }

  .project-label {
    font-size: 0.8125rem;
    font-weight: 500;
    color: var(--foreground);
  }

  /* Navigation tabs */
  .nav-tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    margin-left: 1rem;
    padding: 0.25rem;
    background: var(--mantle);
    border-radius: 10px;
    border: 1px solid var(--glass-border);
  }

  .nav-tab {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.5rem 0.875rem;
    font-size: 0.8125rem;
    font-weight: 500;
    color: var(--muted);
    background: transparent;
    border: none;
    border-radius: 7px;
    cursor: pointer;
    transition: all 0.15s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .nav-tab:hover {
    color: var(--foreground);
    background: var(--surface0);
  }

  .nav-tab.active {
    color: var(--foreground);
    background: var(--surface0);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  }

  .nav-tab svg {
    opacity: 0.7;
    transition: opacity 0.15s;
  }

  .nav-tab:hover svg,
  .nav-tab.active svg {
    opacity: 1;
  }

  .nav-tab.active svg {
    color: var(--blue);
  }

  /* Rotation for expand chevrons */
  .rotate-90 {
    transform: rotate(90deg);
  }
</style>
