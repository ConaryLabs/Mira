<script lang="ts">
  import { onMount } from 'svelte';
  import { marked } from 'marked';
  import ToolCallBlock from './ToolCallBlock.svelte';
  import {
    checkApiStatus,
    getMessages,
    streamChatEvents,
    createMessageBuilder,
    type Message,
    type MessageBlock,
    type StatusResponse,
  } from '$lib/api/client';

  // Configure marked for safe rendering
  marked.setOptions({
    breaks: true,
    gfm: true,
  });

  // Props
  interface Props {
    projectPath?: string;
  }
  let { projectPath = '' }: Props = $props();

  // State
  let messages = $state<Message[]>([]);
  let inputValue = $state('');
  let isLoading = $state(false);
  let apiStatus = $state<StatusResponse | null>(null);
  let messagesContainer: HTMLElement;
  let hasMoreMessages = $state(false);
  let loadingMore = $state(false);
  let currentProjectPath = $state(projectPath || '/home/peter/Mira');

  // Streaming message (shown while response is being received)
  let streamingMessage = $state<{ id: string; blocks: MessageBlock[] } | null>(null);

  onMount(async () => {
    // Get project from URL if available
    const urlParams = new URLSearchParams(window.location.search);
    const urlProject = urlParams.get('project');
    if (urlProject) {
      currentProjectPath = urlProject;
    }

    // Load saved project from localStorage
    const savedProject = localStorage.getItem('mira-project-path');
    if (savedProject && !urlProject) {
      currentProjectPath = savedProject;
    }

    try {
      apiStatus = await checkApiStatus();
      await loadMessages();
    } catch (e) {
      console.error('Failed to initialize:', e);
    }
  });

  async function loadMessages() {
    try {
      const loaded = await getMessages({ limit: 50 });
      // Messages come newest first, reverse for display (oldest at top)
      messages = loaded.reverse();
      hasMoreMessages = loaded.length >= 50;
      setTimeout(scrollToBottom, 0);
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
      // Prepend older messages (they come newest first, so reverse)
      messages = [...older.reverse(), ...messages];
      hasMoreMessages = older.length >= 50;
    } catch (e) {
      console.error('Failed to load more messages:', e);
    } finally {
      loadingMore = false;
    }
  }

  function handleScroll(event: Event) {
    const target = event.target as HTMLElement;
    if (target.scrollTop < 100 && hasMoreMessages && !loadingMore) {
      loadMoreMessages();
    }
  }

  function scrollToBottom() {
    if (messagesContainer) {
      messagesContainer.scrollTop = messagesContainer.scrollHeight;
    }
  }

  async function sendMessage() {
    const content = inputValue.trim();
    if (!content || isLoading) return;

    // Add user message
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      blocks: [{ type: 'text', content }],
      created_at: Date.now() / 1000,
    };
    messages = [...messages, userMessage];
    inputValue = '';
    isLoading = true;

    setTimeout(scrollToBottom, 0);

    // Create streaming message placeholder
    const messageId = crypto.randomUUID();
    streamingMessage = { id: messageId, blocks: [] };

    try {
      const { message: builder, handleEvent } = createMessageBuilder(messageId);

      for await (const event of streamChatEvents({
        message: content,
        project_path: currentProjectPath,
      })) {
        handleEvent(event);
        // Update reactive state
        streamingMessage = { id: messageId, blocks: [...builder.blocks] };
        setTimeout(scrollToBottom, 0);
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
      // Add error message
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
      setTimeout(scrollToBottom, 0);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      sendMessage();
    }
  }

  function renderMarkdown(content: string): string {
    try {
      return marked.parse(content) as string;
    } catch {
      return content;
    }
  }

  function handleProjectChange(event: Event) {
    const target = event.target as HTMLInputElement;
    currentProjectPath = target.value;
    localStorage.setItem('mira-project-path', currentProjectPath);
  }

  // Check if tool call is still loading (no result yet)
  function isToolCallLoading(block: MessageBlock): boolean {
    return block.type === 'tool_call' && !block.result;
  }
</script>

<div class="flex flex-col h-full">
  <!-- Header with project selector -->
  <header class="flex items-center justify-between px-6 py-4 border-b border-gray-200 bg-white">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-full bg-gradient-to-br from-violet-500 to-purple-600 flex items-center justify-center">
        <span class="text-white text-sm font-medium">M</span>
      </div>
      <h1 class="text-lg font-semibold text-gray-900">Mira Studio</h1>
    </div>

    <!-- Project selector -->
    <div class="flex items-center gap-3">
      <label for="project-path" class="text-sm text-gray-500">Project:</label>
      <input
        id="project-path"
        type="text"
        value={currentProjectPath}
        onchange={handleProjectChange}
        class="text-sm px-3 py-1.5 rounded-lg border border-gray-300 focus:border-violet-500 focus:outline-none focus:ring-1 focus:ring-violet-500 w-64 font-mono"
        placeholder="/path/to/project"
      />
    </div>

    <!-- Status -->
    <div class="flex items-center gap-3 text-sm text-gray-500">
      {#if apiStatus?.status === 'ok'}
        <span class="flex items-center gap-1">
          <span class="w-2 h-2 rounded-full bg-green-500"></span>
          connected
        </span>
      {:else}
        <span class="flex items-center gap-1">
          <span class="w-2 h-2 rounded-full bg-yellow-500"></span>
          connecting...
        </span>
      {/if}
      <span>{messages.length} messages</span>
    </div>
  </header>

  <!-- Messages -->
  <div
    bind:this={messagesContainer}
    onscroll={handleScroll}
    class="flex-1 overflow-y-auto px-6 py-4 space-y-4"
  >
    {#if loadingMore}
      <div class="flex justify-center py-2">
        <span class="text-sm text-gray-400">Loading older messages...</span>
      </div>
    {/if}

    {#if hasMoreMessages && !loadingMore}
      <div class="flex justify-center py-2">
        <button
          onclick={loadMoreMessages}
          class="text-sm text-violet-600 hover:text-violet-800"
        >
          Load more
        </button>
      </div>
    {/if}

    {#if messages.length === 0 && !streamingMessage}
      <div class="flex flex-col items-center justify-center h-full text-gray-400">
        <div class="w-16 h-16 rounded-full bg-gray-100 flex items-center justify-center mb-4">
          <span class="text-2xl">💭</span>
        </div>
        <p class="text-lg font-medium">Start chatting</p>
        <p class="text-sm">GPT-5.2 powered coding assistant</p>
        <p class="text-xs mt-2 text-gray-400">Working in: {currentProjectPath}</p>
      </div>
    {:else}
      <!-- Render messages -->
      {#each messages as message (message.id)}
        <div class="flex {message.role === 'user' ? 'justify-end' : 'justify-start'}">
          <div
            class="max-w-[85%] rounded-2xl px-4 py-3 {message.role === 'user'
              ? 'bg-[var(--chat-bubble-user)] text-gray-900'
              : 'bg-[var(--chat-bubble-ai)] border border-gray-200 text-gray-800 shadow-sm'}"
          >
            {#each message.blocks as block}
              {#if block.type === 'text'}
                <div class="prose prose-sm prose-gray max-w-none">
                  {@html renderMarkdown(block.content || '')}
                </div>
              {:else if block.type === 'tool_call'}
                <ToolCallBlock
                  name={block.name || 'unknown'}
                  arguments={block.arguments || {}}
                  result={block.result}
                  isLoading={isToolCallLoading(block)}
                />
              {/if}
            {/each}
          </div>
        </div>
      {/each}

      <!-- Streaming message -->
      {#if streamingMessage}
        <div class="flex justify-start">
          <div class="max-w-[85%] rounded-2xl px-4 py-3 bg-[var(--chat-bubble-ai)] border border-gray-200 text-gray-800 shadow-sm">
            {#if streamingMessage.blocks.length === 0}
              <!-- Loading indicator -->
              <div class="flex gap-1">
                <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 0ms"></span>
                <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 150ms"></span>
                <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 300ms"></span>
              </div>
            {:else}
              {#each streamingMessage.blocks as block}
                {#if block.type === 'text'}
                  <div class="prose prose-sm prose-gray max-w-none">
                    {@html renderMarkdown(block.content || '')}
                  </div>
                {:else if block.type === 'tool_call'}
                  <ToolCallBlock
                    name={block.name || 'unknown'}
                    arguments={block.arguments || {}}
                    result={block.result}
                    isLoading={isToolCallLoading(block)}
                  />
                {/if}
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Input -->
  <div class="px-6 py-4 border-t border-gray-200 bg-white">
    <div class="flex items-end gap-3">
      <textarea
        bind:value={inputValue}
        onkeydown={handleKeydown}
        placeholder="Message Mira..."
        rows="1"
        class="flex-1 resize-none rounded-xl border border-gray-300 px-4 py-3 text-gray-900 placeholder-gray-400 focus:border-violet-500 focus:outline-none focus:ring-1 focus:ring-violet-500 transition-colors"
      ></textarea>
      <button
        onclick={sendMessage}
        disabled={!inputValue.trim() || isLoading}
        class="rounded-xl bg-violet-600 px-4 py-3 text-white font-medium hover:bg-violet-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
      >
        Send
      </button>
    </div>
  </div>
</div>
