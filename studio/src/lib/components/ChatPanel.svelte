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
  import { toolActivityStore } from '$lib/stores/toolActivity.svelte';
  import AppShell from './layout/AppShell.svelte';
  import TerminalView from './terminal/TerminalView.svelte';
  import TerminalPrompt from './terminal/TerminalPrompt.svelte';
  import ArtifactViewer from './ArtifactViewer.svelte';
  import { artifactViewer, artifactStore } from '$lib/stores/artifacts.svelte';
  import { layoutStore } from '$lib/stores/layout.svelte';

  // State
  let messages = $state<Message[]>([]);
  let apiStatus = $state<StatusResponse | null>(null);
  let hasMoreMessages = $state(false);
  let loadingMore = $state(false);
  let initialLoadComplete = $state(false);
  let messageQueue = $state<string[]>([]);
  let isProcessing = $state(false);

  // Reference to terminal view for scrolling
  let terminalView: { scrollToBottom: () => void; forceScrollToBottom: () => void };

  // Reference to terminal prompt for keyboard shortcuts
  let terminalPrompt: { focus: () => void };

  // Global keyboard shortcuts
  function handleGlobalKeydown(event: KeyboardEvent) {
    // Cmd/Ctrl + / - Focus input
    if ((event.metaKey || event.ctrlKey) && event.key === '/') {
      event.preventDefault();
      terminalPrompt?.focus();
      return;
    }

    // Cmd/Ctrl + \ - Toggle drawer
    if ((event.metaKey || event.ctrlKey) && event.key === '\\') {
      event.preventDefault();
      layoutStore.toggleDrawer();
      return;
    }

    // Cmd/Ctrl + 1 - Timeline tab
    if ((event.metaKey || event.ctrlKey) && event.key === '1') {
      event.preventDefault();
      layoutStore.setDrawerTab('timeline');
      return;
    }

    // Cmd/Ctrl + 2 - Workspace tab
    if ((event.metaKey || event.ctrlKey) && event.key === '2') {
      event.preventDefault();
      layoutStore.setDrawerTab('workspace');
      return;
    }

    // Escape - Cancel streaming or close drawer
    if (event.key === 'Escape') {
      if (streamState.canCancel) {
        event.preventDefault();
        streamState.cancelStream();
        return;
      }
      if (layoutStore.contextDrawer.open) {
        event.preventDefault();
        layoutStore.closeDrawer();
        return;
      }
    }
  }

  onMount(async () => {
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

  function sendMessage(content: string) {
    if (!content) return;

    // Add user message to UI immediately
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      blocks: [{ type: 'text', content }],
      created_at: Date.now() / 1000,
    };
    messages = [...messages, userMessage];
    setTimeout(() => terminalView?.scrollToBottom(), 0);

    // Queue the message for processing
    messageQueue = [...messageQueue, content];
    processQueue();
  }

  async function processQueue() {
    // Don't start if already processing or queue is empty
    if (isProcessing || messageQueue.length === 0) return;

    isProcessing = true;
    const content = messageQueue[0];
    messageQueue = messageQueue.slice(1);

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

        // Forward tool events to stores
        if (event.type === 'tool_call_start') {
          toolActivityStore.toolStarted(event);
        } else if (event.type === 'tool_call_result') {
          toolActivityStore.toolCompleted(event);
          artifactStore.processToolResult(event);
        }

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
      isProcessing = false;
      setTimeout(() => terminalView?.scrollToBottom(), 0);
      // Process next message in queue if any
      processQueue();
    }
  }
</script>

<AppShell {apiStatus}>
  <div class="chat-area">
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
      disabled={false}
      isStreaming={streamState.isLoading}
      placeholder={messageQueue.length > 0 ? `${messageQueue.length} queued...` : undefined}
    />
  </div>
</AppShell>

<!-- Artifact viewer modal -->
<ArtifactViewer
  isOpen={artifactViewer.isOpen}
  filename={artifactViewer.filename}
  language={artifactViewer.language}
  code={artifactViewer.code}
  onClose={artifactViewer.close}
/>

<style>
  .chat-area {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    overflow: hidden;
  }
</style>
