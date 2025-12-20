<script lang="ts">
  import { onMount } from 'svelte';
  import {
    checkApiStatus,
    getMessages,
    streamChatEvents,
    createMessageBuilder,
    type Message,
    type StatusResponse,
  } from '$lib/api/client';
  import { settings } from '$lib/stores/settings';
  import { streamState } from '$lib/stores/streamState.svelte';
  import SettingsSidebar from './sidebar/SettingsSidebar.svelte';
  import TerminalView from './terminal/TerminalView.svelte';
  import TerminalPrompt from './terminal/TerminalPrompt.svelte';
  import ArtifactViewer from './ArtifactViewer.svelte';
  import { artifactViewer } from '$lib/stores/artifacts.svelte';

  // State
  let messages = $state<Message[]>([]);
  let apiStatus = $state<StatusResponse | null>(null);
  let hasMoreMessages = $state(false);
  let loadingMore = $state(false);
  let initialLoadComplete = $state(false);

  // Mobile sidebar state (transient, not persisted)
  let mobileSidebarOpen = $state(false);
  let isMobile = $state(false);

  // Reference to terminal view for scrolling
  let terminalView: { scrollToBottom: () => void; forceScrollToBottom: () => void };

  // Reference to terminal prompt for keyboard shortcuts
  let terminalPrompt: { focus: () => void };

  function checkMobile() {
    isMobile = window.innerWidth < 768;
    if (!isMobile) {
      mobileSidebarOpen = false;
    }
  }

  // Global keyboard shortcuts
  function handleGlobalKeydown(event: KeyboardEvent) {
    // Cmd/Ctrl + / - Focus input
    if ((event.metaKey || event.ctrlKey) && event.key === '/') {
      event.preventDefault();
      terminalPrompt?.focus();
      return;
    }

    // Escape - Cancel streaming (when not focused on input)
    if (event.key === 'Escape' && streamState.canCancel) {
      event.preventDefault();
      streamState.cancelStream();
      return;
    }
  }

  onMount(async () => {
    // Check if mobile on mount and resize
    checkMobile();
    window.addEventListener('resize', checkMobile);
    window.addEventListener('keydown', handleGlobalKeydown);

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
      window.removeEventListener('keydown', handleGlobalKeydown);
    };
  });

  async function loadMessages() {
    try {
      const loaded = await getMessages({ limit: 50 });
      messages = loaded.reverse();
      hasMoreMessages = loaded.length >= 50;
      // Force scroll to bottom on initial load, then mark complete
      setTimeout(() => {
        terminalView?.forceScrollToBottom();
        // Delay marking complete to prevent scroll handler from immediately loading more
        setTimeout(() => { initialLoadComplete = true; }, 100);
      }, 0);
    } catch (e) {
      console.error('Failed to load messages:', e);
    }
  }

  async function loadMoreMessages() {
    // Don't auto-load more until initial load + scroll is complete
    if (!initialLoadComplete || loadingMore || !hasMoreMessages || messages.length === 0) return;

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
    if (!content || streamState.isLoading) return;

    // Add user message
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      blocks: [{ type: 'text', content }],
      created_at: Date.now() / 1000,
    };
    messages = [...messages, userMessage];

    setTimeout(() => terminalView?.scrollToBottom(), 0);

    // Start streaming via state machine
    const messageId = crypto.randomUUID();
    const controller = streamState.startStream(messageId);

    try {
      const { message: builder, handleEvent } = createMessageBuilder(messageId);

      // Build request with settings
      const currentSettings = $settings;
      const request = {
        message: content,
        project_path: currentSettings.projectPath,
        reasoning_effort: currentSettings.reasoningEffort === 'auto' ? undefined : currentSettings.reasoningEffort,
        provider: currentSettings.modelProvider,
        signal: controller.signal,
      };

      // Batch UI updates to ~60fps using requestAnimationFrame
      let rafPending = false;
      const scheduleUpdate = () => {
        if (!rafPending) {
          rafPending = true;
          requestAnimationFrame(() => {
            rafPending = false;
            streamState.updateStream([...builder.blocks], builder.usage);
            terminalView?.scrollToBottom();
          });
        }
      };

      for await (const event of streamChatEvents(request)) {
        handleEvent(event);
        scheduleUpdate();
      }

      // Ensure final state is rendered
      streamState.updateStream([...builder.blocks], builder.usage);

      // Move streaming message to permanent messages (including usage)
      const finalMessage: Message = {
        id: messageId,
        role: 'assistant',
        blocks: builder.blocks,
        created_at: Date.now() / 1000,
        usage: builder.usage,
      };
      messages = [...messages, finalMessage];
      streamState.completeStream();
    } catch (error) {
      // Handle cancellation gracefully
      if (error instanceof Error && error.name === 'AbortError') {
        // Append cancelled marker to partial message
        const cancelledMessage: Message = {
          id: messageId,
          role: 'assistant',
          blocks: [
            ...streamState.getFinalBlocks(),
            { type: 'text', content: '\n\n*[Cancelled]*' }
          ],
          created_at: Date.now() / 1000,
        };
        messages = [...messages, cancelledMessage];
      } else {
        console.error('Failed to send message:', error);
        streamState.errorStream(error instanceof Error ? error : new Error('Failed to get response'));
        const errorMessage: Message = {
          id: messageId,
          role: 'assistant',
          blocks: [{ type: 'text', content: `Error: ${error instanceof Error ? error.message : 'Failed to get response'}` }],
          created_at: Date.now() / 1000,
        };
        messages = [...messages, errorMessage];
      }
    } finally {
      streamState.reset();
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
      streamingMessage={streamState.streamingMessage}
      onLoadMore={loadMoreMessages}
      hasMore={hasMoreMessages}
      {loadingMore}
    />
    <TerminalPrompt
      bind:this={terminalPrompt}
      onSend={sendMessage}
      onCancel={() => streamState.cancelStream()}
      disabled={streamState.isLoading}
      isStreaming={streamState.isLoading}
      placeholder={streamState.isLoading ? 'Processing...' : 'Enter command...'}
    />
  </main>
</div>

<!-- Artifact viewer modal -->
<ArtifactViewer
  isOpen={artifactViewer.isOpen}
  filename={artifactViewer.filename}
  language={artifactViewer.language}
  code={artifactViewer.code}
  onClose={artifactViewer.close}
/>
