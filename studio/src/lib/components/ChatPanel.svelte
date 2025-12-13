<script lang="ts">
  import { onMount } from 'svelte';
  import { marked } from 'marked';
  import {
    streamChat,
    checkApiStatus,
    listConversations,
    getMessages,
    type MessageInfo,
    type ConversationInfo
  } from '$lib/api/client';

  // Configure marked for safe rendering
  marked.setOptions({
    breaks: true,  // Convert \n to <br>
    gfm: true,     // GitHub Flavored Markdown
  });

  interface Message {
    id: string;
    role: 'user' | 'assistant';
    content: string;
    timestamp: Date;
  }

  let messages = $state<Message[]>([]);
  let inputValue = $state('');
  let isLoading = $state(false);
  let apiConfigured = $state(false);
  let messagesContainer: HTMLElement;

  // Conversation state
  let conversationId = $state<string | null>(null);
  let conversations = $state<ConversationInfo[]>([]);
  let hasMoreMessages = $state(false);
  let loadingMore = $state(false);

  onMount(async () => {
    try {
      const status = await checkApiStatus();
      apiConfigured = status.anthropic_configured;

      // Load conversations and resume the most recent one
      await loadConversations();
    } catch (e) {
      console.error('Failed to initialize:', e);
    }
  });

  async function loadConversations() {
    try {
      conversations = await listConversations();
      if (conversations.length > 0) {
        // Resume most recent conversation
        await loadConversation(conversations[0].id);
      }
    } catch (e) {
      console.error('Failed to load conversations:', e);
    }
  }

  async function loadConversation(id: string) {
    try {
      conversationId = id;
      const loaded = await getMessages(id, 20);
      messages = loaded.map(m => ({
        id: m.id,
        role: m.role,
        content: m.content,
        timestamp: new Date(m.created_at * 1000)
      }));
      hasMoreMessages = loaded.length >= 20;
      setTimeout(scrollToBottom, 0);
    } catch (e) {
      console.error('Failed to load conversation:', e);
    }
  }

  async function loadMoreMessages() {
    if (!conversationId || loadingMore || !hasMoreMessages) return;

    const firstMessageId = messages[0]?.id;
    if (!firstMessageId) return;

    loadingMore = true;
    try {
      const older = await getMessages(conversationId, 20, firstMessageId);
      const olderMessages = older.map(m => ({
        id: m.id,
        role: m.role as 'user' | 'assistant',
        content: m.content,
        timestamp: new Date(m.created_at * 1000)
      }));
      messages = [...olderMessages, ...messages];
      hasMoreMessages = older.length >= 20;
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

  async function startNewConversation() {
    conversationId = null;
    messages = [];
    hasMoreMessages = false;
  }

  async function sendMessage() {
    const content = inputValue.trim();
    if (!content || isLoading) return;

    // Add user message (temporary ID until we get server response)
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content,
      timestamp: new Date()
    };
    messages = [...messages, userMessage];
    inputValue = '';
    isLoading = true;

    setTimeout(scrollToBottom, 0);

    // Create assistant message placeholder
    const assistantMessage: Message = {
      id: crypto.randomUUID(),
      role: 'assistant',
      content: '',
      timestamp: new Date()
    };
    messages = [...messages, assistantMessage];

    try {
      const result = await streamChat({
        message: content,
        conversation_id: conversationId ?? undefined
      });

      // Update conversation ID if new
      if (!conversationId) {
        conversationId = result.conversationId;
        // Refresh conversation list
        loadConversations();
      }

      // Stream the response
      for await (const chunk of result.chunks) {
        // Debug: log chunks that might be problematic
        if (chunk === '' || chunk.startsWith(' ') || chunk.endsWith(' ')) {
          console.log('SSE chunk:', JSON.stringify(chunk));
        }
        const lastIndex = messages.length - 1;
        messages[lastIndex] = {
          ...messages[lastIndex],
          content: messages[lastIndex].content + chunk
        };
        setTimeout(scrollToBottom, 0);
      }
    } catch (error) {
      console.error('Failed to send message:', error);
      const lastIndex = messages.length - 1;
      messages[lastIndex] = {
        ...messages[lastIndex],
        content: `Error: ${error instanceof Error ? error.message : 'Failed to get response'}`
      };
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

  function formatTime(date: Date): string {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function renderMarkdown(content: string): string {
    try {
      return marked.parse(content) as string;
    } catch {
      return content;
    }
  }
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <header class="flex items-center justify-between px-6 py-4 border-b border-gray-200">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-full bg-gradient-to-br from-violet-500 to-purple-600 flex items-center justify-center">
        <span class="text-white text-sm font-medium">M</span>
      </div>
      <h1 class="text-lg font-semibold text-gray-900">Mira Studio</h1>
    </div>
    <div class="flex items-center gap-3">
      <button
        onclick={startNewConversation}
        class="text-sm px-3 py-1 rounded-lg border border-gray-300 hover:bg-gray-50 transition-colors"
      >
        New Chat
      </button>
      <div class="flex items-center gap-3 text-sm text-gray-500">
        {#if apiConfigured}
          <span class="flex items-center gap-1">
            <span class="w-2 h-2 rounded-full bg-green-500"></span>
            connected
          </span>
        {:else}
          <span class="flex items-center gap-1">
            <span class="w-2 h-2 rounded-full bg-yellow-500"></span>
            API not configured
          </span>
        {/if}
        <span>{messages.length} messages</span>
      </div>
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

    {#if messages.length === 0}
      <div class="flex flex-col items-center justify-center h-full text-gray-400">
        <div class="w-16 h-16 rounded-full bg-gray-100 flex items-center justify-center mb-4">
          <span class="text-2xl">💭</span>
        </div>
        <p class="text-lg font-medium">Start a conversation</p>
        <p class="text-sm">Say hello, ask a question, or let's get to work.</p>
        {#if !apiConfigured}
          <p class="text-sm text-yellow-600 mt-2">Add ANTHROPIC_API_KEY to .env to enable chat</p>
        {/if}
      </div>
    {:else}
      {#each messages as message (message.id)}
        <div class="flex {message.role === 'user' ? 'justify-end' : 'justify-start'}">
          <div
            class="max-w-[80%] rounded-2xl px-4 py-3 {message.role === 'user'
              ? 'bg-[var(--chat-bubble-user)] text-gray-900'
              : 'bg-[var(--chat-bubble-ai)] border border-gray-200 text-gray-800 shadow-sm'}"
          >
            {#if message.role === 'assistant'}
              <div class="prose prose-sm prose-gray max-w-none">
                {@html renderMarkdown(message.content)}
              </div>
            {:else}
              <p class="whitespace-pre-wrap">{message.content}</p>
            {/if}
          </div>
        </div>
      {/each}

      {#if isLoading && messages[messages.length - 1]?.content === ''}
        <div class="flex justify-start">
          <div class="bg-[var(--chat-bubble-ai)] border border-gray-200 rounded-2xl px-4 py-3 shadow-sm">
            <div class="flex gap-1">
              <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 0ms"></span>
              <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 150ms"></span>
              <span class="w-2 h-2 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 300ms"></span>
            </div>
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
        placeholder={apiConfigured ? "Message Mira..." : "Configure API key to chat..."}
        disabled={!apiConfigured}
        rows="1"
        class="flex-1 resize-none rounded-xl border border-gray-300 px-4 py-3 text-gray-900 placeholder-gray-400 focus:border-violet-500 focus:outline-none focus:ring-1 focus:ring-violet-500 transition-colors disabled:bg-gray-100 disabled:cursor-not-allowed"
      ></textarea>
      <button
        onclick={sendMessage}
        disabled={!inputValue.trim() || isLoading || !apiConfigured}
        class="rounded-xl bg-violet-600 px-4 py-3 text-white font-medium hover:bg-violet-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
      >
        Send
      </button>
    </div>
  </div>
</div>
