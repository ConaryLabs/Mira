// API client for Mira Studio

export interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface ChatRequest {
  message: string;
  conversation_id?: string;
  model?: string;
  max_tokens?: number;
}

export interface ConversationInfo {
  id: string;
  title: string | null;
  created_at: number;
  updated_at: number;
  message_count: number;
}

export interface MessageInfo {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  created_at: number;
}

export async function checkApiStatus(): Promise<{ status: string; anthropic_configured: boolean }> {
  const response = await fetch('/api/status');
  return response.json();
}

export async function listConversations(): Promise<ConversationInfo[]> {
  const response = await fetch('/api/conversations');
  if (!response.ok) {
    throw new Error(`Failed to list conversations: ${response.status}`);
  }
  return response.json();
}

export async function createConversation(): Promise<ConversationInfo> {
  const response = await fetch('/api/conversations', { method: 'POST' });
  if (!response.ok) {
    throw new Error(`Failed to create conversation: ${response.status}`);
  }
  return response.json();
}

export async function getConversation(id: string): Promise<ConversationInfo> {
  const response = await fetch(`/api/conversations/${id}`);
  if (!response.ok) {
    throw new Error(`Failed to get conversation: ${response.status}`);
  }
  return response.json();
}

export async function getMessages(conversationId: string, limit = 20, beforeId?: string): Promise<MessageInfo[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  if (beforeId) {
    params.set('before', beforeId);
  }
  const response = await fetch(`/api/conversations/${conversationId}/messages?${params}`);
  if (!response.ok) {
    throw new Error(`Failed to get messages: ${response.status}`);
  }
  return response.json();
}

export interface StreamResult {
  conversationId: string;
  chunks: AsyncGenerator<string, void, unknown>;
}

/**
 * Parse SSE events from a buffer, handling event boundaries correctly.
 * Returns [parsedEvents, remainingBuffer]
 */
function parseSSEEvents(buffer: string): [string[], string] {
  const events: string[] = [];
  let remaining = buffer;

  // SSE events are separated by blank lines (\n\n or \r\n\r\n)
  while (true) {
    // Find event boundary
    let boundaryPos = remaining.indexOf('\r\n\r\n');
    let boundaryLen = 4;
    if (boundaryPos === -1) {
      boundaryPos = remaining.indexOf('\n\n');
      boundaryLen = 2;
    }

    if (boundaryPos === -1) {
      // No complete event yet
      break;
    }

    const eventBlock = remaining.substring(0, boundaryPos);
    remaining = remaining.substring(boundaryPos + boundaryLen);

    // Parse the event block - collect all data: lines
    const dataLines: string[] = [];
    for (const line of eventBlock.split(/\r?\n/)) {
      if (line.startsWith('data:')) {
        // SSE spec: optional single space after colon is stripped
        const value = line.substring(5);
        dataLines.push(value.startsWith(' ') ? value.substring(1) : value);
      }
      // Ignore event:, id:, retry:, and comment lines
    }

    // Per SSE spec, multiple data lines are joined with newlines
    if (dataLines.length > 0) {
      events.push(dataLines.join('\n'));
    }
  }

  return [events, remaining];
}

export async function streamChat(request: ChatRequest): Promise<StreamResult> {
  const response = await fetch('/api/chat/stream', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || `HTTP ${response.status}`);
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error('No response body');
  }

  let conversationId = '';
  const decoder = new TextDecoder();
  let buffer = '';
  let pendingEvents: string[] = []; // Events received after conversation ID

  // Read until we get the conversation ID
  while (!conversationId) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const [events, remaining] = parseSSEEvents(buffer);
    buffer = remaining;

    for (let i = 0; i < events.length; i++) {
      const data = events[i];
      // Check if this looks like a UUID (conversation ID)
      if (data.match(/^[0-9a-f-]{36}$/i)) {
        conversationId = data;
        // Capture any events that came after the ID in this batch
        pendingEvents = events.slice(i + 1);
        break;
      }
    }
  }

  // Create async generator for the rest of the stream
  async function* generateChunks(): AsyncGenerator<string, void, unknown> {
    // First, yield any events that came after the conversation ID
    for (const data of pendingEvents) {
      if (data === '[DONE]') return;
      if (data.startsWith('[ERROR]')) throw new Error(data);
      if (data && !data.match(/^[0-9a-f-]{36}$/i)) {
        yield data;
      }
    }

    // Process any buffered events
    const [initialEvents, remaining] = parseSSEEvents(buffer);
    buffer = remaining;

    for (const data of initialEvents) {
      if (data === '[DONE]') return;
      if (data.startsWith('[ERROR]')) throw new Error(data);
      if (data && !data.match(/^[0-9a-f-]{36}$/i)) {
        yield data;
      }
    }

    // Continue reading stream
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        // Flush decoder
        buffer += decoder.decode();
        break;
      }

      buffer += decoder.decode(value, { stream: true });
      const [events, rem] = parseSSEEvents(buffer);
      buffer = rem;

      for (const data of events) {
        if (data === '[DONE]') return;
        if (data.startsWith('[ERROR]')) throw new Error(data);
        yield data;
      }
    }

    // Process any final buffered events
    if (buffer.trim()) {
      // Add fake boundary to flush remaining
      const [finalEvents] = parseSSEEvents(buffer + '\n\n');
      for (const data of finalEvents) {
        if (data === '[DONE]') return;
        if (data.startsWith('[ERROR]')) throw new Error(data);
        yield data;
      }
    }
  }

  return {
    conversationId,
    chunks: generateChunks(),
  };
}
