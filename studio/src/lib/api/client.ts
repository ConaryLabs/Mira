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

  // Read until we get the conversation ID
  while (!conversationId) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (line.startsWith('event: conversation')) {
        continue;
      }
      if (line.startsWith('data: ') && !conversationId) {
        const data = line.slice(6);
        // Check if this looks like a UUID (conversation ID)
        if (data.match(/^[0-9a-f-]{36}$/i)) {
          conversationId = data;
          // Save remaining lines back to buffer for the generator
          const remaining = lines.slice(i + 1);
          if (remaining.length > 0) {
            buffer = remaining.join('\n') + '\n' + buffer;
          }
          break;
        }
      }
    }
  }

  // Create async generator for the rest of the stream
  async function* generateChunks(): AsyncGenerator<string, void, unknown> {
    // Process any remaining buffer from conversation ID parsing
    // Pop last line in case it's incomplete
    const initialLines = buffer.split('\n');
    buffer = initialLines.pop() || '';

    for (const line of initialLines) {
      // Handle SSE data lines - preserve all whitespace, handle \r
      const cleanLine = line.replace(/\r$/, '');
      if (cleanLine.startsWith('data: ')) {
        const data = cleanLine.substring(6);
        if (data === '[DONE]') return;
        if (data.startsWith('[ERROR]')) throw new Error(data);
        // Skip empty strings and conversation IDs
        if (data && !data.match(/^[0-9a-f-]{36}$/i)) {
          yield data;
        }
      }
    }

    // Continue reading stream
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      for (const line of lines) {
        // Handle SSE data lines - preserve all whitespace, handle \r
        const cleanLine = line.replace(/\r$/, '');
        if (cleanLine.startsWith('data: ')) {
          const data = cleanLine.substring(6);
          if (data === '[DONE]') return;
          if (data.startsWith('[ERROR]')) throw new Error(data);
          yield data;
        } else if (line.includes('data')) {
          // Debug: log lines that contain 'data' but don't match our pattern
          console.warn('Unexpected SSE line:', JSON.stringify(line));
        }
      }
    }
  }

  return {
    conversationId,
    chunks: generateChunks(),
  };
}
