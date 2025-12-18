<script lang="ts">
  import { onMount } from 'svelte';
  import {
    checkApiStatus,
    getMessages,
    streamChatEvents,
    createMessageBuilder,
    type Message,
    type MessageBlock,
    type StatusResponse,
  } from '$lib/api/client';
  import { settings } from '$lib/stores/settings';
  import SettingsSidebar from './sidebar/SettingsSidebar.svelte';
  import TerminalView from './terminal/TerminalView.svelte';
  import TerminalPrompt from './terminal/TerminalPrompt.svelte';

  // State
  let messages = $state<Message[]>([]);
  let isLoading = $state(false);
  let apiStatus = $state<StatusResponse | null>(null);
  let hasMoreMessages = $state(false);
  let loadingMore = $state(false);

  // Mobile sidebar state (transient, not persisted)
  let mobileSidebarOpen = $state(false);
  let isMobile = $state(false);

  // Streaming message
  let streamingMessage = $state<{ id: string; blocks: MessageBlock[] } | null>(null);

  // Reference to terminal view for scrolling
  let terminalView: { scrollToBottom: () => void };

  function checkMobile() {
    isMobile = window.innerWidth < 768;
    if (!isMobile) {
      mobileSidebarOpen = false;
    }
  }

  onMount(async () => {
    // Check if mobile on mount and resize
    checkMobile();
    window.addEventListener('resize', checkMobile);

    // Get project from URL if available
    const urlParams = new URLSearchParams(window.location.search);
    const urlProject = urlParams.get('project');
    if (urlProject) {
      settings.setProjectPath(urlProject);
    }

    try {
      apiStatus = await checkApiStatus();
      await loadMessages();
    } catch (e) {
      console.error('Failed to initialize:', e);
    }

    return () => {
      window.removeEventListener('resize', checkMobile);
    };
  });

  async function loadMessages() {
    try {
      const loaded = await getMessages({ limit: 50 });
      messages = loaded.reverse();
      hasMoreMessages = loaded.length >= 50;
      setTimeout(() => terminalView?.scrollToBottom(), 0);
    } catch (e) {
      console.error('Failed to load messages:', e);
    }
  }

  async function loadMoreMessages() {
    if (loadingMore || !hasMoreMessages || messages.length === 0) return;

    const oldestMessage = messages[0];
    if (!oldestMessage) return;

    loadingMore = true;
    try {
      const older = await getMessages({
        limit: 50,
        before: oldestMessage.created_at,
      });
      messages = [...older.reverse(), ...messages];
      hasMoreMessages = older.length >= 50;
    } catch (e) {
      console.error('Failed to load more messages:', e);
    } finally {
      loadingMore = false;
    }
  }

  async function sendMessage(content: string) {
    if (!content || isLoading) return;

    // Add user message
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      blocks: [{ type: 'text', content }],
      created_at: Date.now() / 1000,
    };
    messages = [...messages, userMessage];
    isLoading = true;

    setTimeout(() => terminalView?.scrollToBottom(), 0);

    // Create streaming message placeholder
    const messageId = crypto.randomUUID();
    streamingMessage = { id: messageId, blocks: [] };

    try {
      const { message: builder, handleEvent } = createMessageBuilder(messageId);

      // Build request with reasoning effort from settings
      const currentSettings = $settings;
      const request = {
        message: content,
        project_path: currentSettings.projectPath,
        reasoning_effort: currentSettings.reasoningEffort === 'auto' ? undefined : currentSettings.reasoningEffort,
      };

      for await (const event of streamChatEvents(request)) {
        handleEvent(event);
        streamingMessage = { id: messageId, blocks: [...builder.blocks] };
        setTimeout(() => terminalView?.scrollToBottom(), 0);
      }

      // Move streaming message to permanent messages
      const finalMessage: Message = {
        id: messageId,
        role: 'assistant',
        blocks: builder.blocks,
        created_at: Date.now() / 1000,
      };
      messages = [...messages, finalMessage];
      streamingMessage = null;
    } catch (error) {
      console.error('Failed to send message:', error);
      const errorMessage: Message = {
        id: messageId,
        role: 'assistant',
        blocks: [{ type: 'text', content: `Error: ${error instanceof Error ? error.message : 'Failed to get response'}` }],
        created_at: Date.now() / 1000,
      };
      messages = [...messages, errorMessage];
      streamingMessage = null;
    } finally {
      isLoading = false;
      setTimeout(() => terminalView?.scrollToBottom(), 0);
    }
  }
</script>

<div class="flex h-full w-full bg-[var(--term-bg)]">
  <!-- Mobile backdrop -->
  {#if isMobile && mobileSidebarOpen}
    <button
      class="mobile-backdrop"
      onclick={() => mobileSidebarOpen = false}
      aria-label="Close sidebar"
    ></button>
  {/if}

  <!-- Settings Sidebar -->
  <div class="{isMobile ? (mobileSidebarOpen ? 'mobile-overlay' : 'hidden') : ''}">
    <SettingsSidebar status={apiStatus} onClose={() => mobileSidebarOpen = false} {isMobile} />
  </div>

  <!-- Main Terminal Area -->
  <main class="flex-1 flex flex-col min-w-0">
    <!-- Mobile header with menu button -->
    {#if isMobile}
      <div class="flex items-center gap-2 px-3 py-2 bg-[var(--term-bg-secondary)] border-b border-[var(--term-border)]">
        <button
          onclick={() => mobileSidebarOpen = true}
          class="p-1.5 rounded hover:bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-text)] transition-colors"
          aria-label="Open menu"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 12h16M4 18h16" />
          </svg>
        </button>
        <span class="text-[var(--term-accent)] font-mono font-bold">M</span>
        <span class="text-[var(--term-text)] font-mono text-sm">Mira</span>
        <!-- Status dot -->
        <span
          class="ml-auto w-2 h-2 rounded-full {apiStatus?.status === 'ok' ? 'bg-[var(--term-success)]' : 'bg-[var(--term-error)]'}"
          title={apiStatus?.status === 'ok' ? 'Connected' : 'Disconnected'}
        ></span>
      </div>
    {/if}

    <TerminalView
      bind:this={terminalView}
      {messages}
      {streamingMessage}
      onLoadMore={loadMoreMessages}
      hasMore={hasMoreMessages}
      {loadingMore}
    />
    <TerminalPrompt
      onSend={sendMessage}
      disabled={isLoading}
      placeholder={isLoading ? 'Processing...' : 'Enter command...'}
    />
  </main>
</div>
