// API client for Mira Studio (GPT-5.2 backend)

// ============================================================================
// Types
// ============================================================================

export interface DiffInfo {
  path: string;
  old_content?: string;
  new_content: string;
  is_new_file: boolean;
}

export interface ToolCallResult {
  success: boolean;
  output: string;
  diff?: DiffInfo;
}

export interface MessageBlock {
  type: 'text' | 'tool_call';
  content?: string;
  call_id?: string;
  name?: string;
  arguments?: Record<string, unknown>;
  result?: ToolCallResult;
}

export interface UsageInfo {
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cached_tokens: number;
}

export interface Message {
  id: string;
  role: 'user' | 'assistant';
  blocks: MessageBlock[];
  created_at: number;
  usage?: UsageInfo;
}

export interface ChatRequest {
  message: string;
  project_path: string;
  reasoning_effort?: string;
  provider?: 'gpt' | 'deepseek';
}

export interface StatusResponse {
  status: string;
  semantic_search: boolean;
  database: boolean;
  model?: string;
  default_reasoning_effort?: string;
}

// ============================================================================
// SSE Events (from backend)
// ============================================================================

export type ChatEvent =
  | { type: 'text_delta'; delta: string }
  | { type: 'tool_call_start'; call_id: string; name: string; arguments: Record<string, unknown> }
  | { type: 'tool_call_result'; call_id: string; name: string; success: boolean; output: string; diff?: DiffInfo }
  | { type: 'reasoning'; effort: string; summary?: string }
  | { type: 'usage'; input_tokens: number; output_tokens: number; reasoning_tokens: number; cached_tokens: number }
  | { type: 'done' }
  | { type: 'error'; message: string };

// ============================================================================
// API Functions
// ============================================================================

export async function checkApiStatus(): Promise<StatusResponse> {
  const response = await fetch('/api/status');
  if (!response.ok) {
    throw new Error(`Failed to check status: ${response.status}`);
  }
  return response.json();
}

export interface MessagesQuery {
  limit?: number;
  before?: number; // created_at timestamp for cursor pagination
}

export async function getMessages(query: MessagesQuery = {}): Promise<Message[]> {
  const params = new URLSearchParams();
  if (query.limit) params.set('limit', String(query.limit));
  if (query.before) params.set('before', String(query.before));

  const url = params.toString() ? `/api/messages?${params}` : '/api/messages';
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to get messages: ${response.status}`);
  }
  return response.json();
}

// ============================================================================
// SSE Streaming
// ============================================================================

/**
 * Parse SSE events from a buffer.
 * Returns [parsedEventData, remainingBuffer]
 */
function parseSSEEvents(buffer: string): [string[], string] {
  const events: string[] = [];
  let remaining = buffer;

  while (true) {
    // Find event boundary (\n\n or \r\n\r\n)
    let boundaryPos = remaining.indexOf('\r\n\r\n');
    let boundaryLen = 4;
    if (boundaryPos === -1) {
      boundaryPos = remaining.indexOf('\n\n');
      boundaryLen = 2;
    }

    if (boundaryPos === -1) break;

    const eventBlock = remaining.substring(0, boundaryPos);
    remaining = remaining.substring(boundaryPos + boundaryLen);

    // Parse data: lines
    const dataLines: string[] = [];
    for (const line of eventBlock.split(/\r?\n/)) {
      if (line.startsWith('data:')) {
        const value = line.substring(5);
        dataLines.push(value.startsWith(' ') ? value.substring(1) : value);
      }
    }

    if (dataLines.length > 0) {
      events.push(dataLines.join('\n'));
    }
  }

  return [events, remaining];
}

/**
 * Stream chat events from the backend.
 * Yields typed ChatEvent objects.
 */
export async function* streamChatEvents(request: ChatRequest): AsyncGenerator<ChatEvent, void, unknown> {
  const response = await fetch('/api/chat/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
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

  const decoder = new TextDecoder();
  let buffer = '';

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        // Flush decoder
        buffer += decoder.decode();
        break;
      }

      buffer += decoder.decode(value, { stream: true });
      const [events, remaining] = parseSSEEvents(buffer);
      buffer = remaining;

      for (const data of events) {
        try {
          const event = JSON.parse(data) as ChatEvent;
          yield event;
        } catch {
          console.warn('Failed to parse SSE event:', data);
        }
      }
    }

    // Process any final buffered events
    if (buffer.trim()) {
      const [finalEvents] = parseSSEEvents(buffer + '\n\n');
      for (const data of finalEvents) {
        try {
          const event = JSON.parse(data) as ChatEvent;
          yield event;
        } catch {
          console.warn('Failed to parse final SSE event:', data);
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

// ============================================================================
// Helper for building message blocks from events
// ============================================================================

export interface StreamingMessage {
  id: string;
  role: 'assistant';
  blocks: MessageBlock[];
  isComplete: boolean;
  usage?: UsageInfo;
}

/**
 * Create a message builder that accumulates events into a message.
 */
export function createMessageBuilder(id: string): {
  message: StreamingMessage;
  handleEvent: (event: ChatEvent) => void;
} {
  const message: StreamingMessage = {
    id,
    role: 'assistant',
    blocks: [],
    isComplete: false,
  };

  // Track current text block index (if any)
  let currentTextBlockIndex = -1;
  // Track tool calls by call_id
  const toolCallIndices = new Map<string, number>();

  function handleEvent(event: ChatEvent) {
    switch (event.type) {
      case 'text_delta': {
        if (currentTextBlockIndex === -1) {
          // Create new text block
          currentTextBlockIndex = message.blocks.length;
          message.blocks.push({ type: 'text', content: '' });
        }
        const block = message.blocks[currentTextBlockIndex];
        if (block.type === 'text') {
          block.content = (block.content || '') + event.delta;
        }
        break;
      }

      case 'tool_call_start': {
        // End current text block
        currentTextBlockIndex = -1;
        // Create new tool call block
        const index = message.blocks.length;
        toolCallIndices.set(event.call_id, index);
        message.blocks.push({
          type: 'tool_call',
          call_id: event.call_id,
          name: event.name,
          arguments: event.arguments,
        });
        break;
      }

      case 'tool_call_result': {
        const index = toolCallIndices.get(event.call_id);
        if (index !== undefined) {
          const block = message.blocks[index];
          if (block.type === 'tool_call') {
            block.result = {
              success: event.success,
              output: event.output,
              diff: event.diff,
            };
          }
        }
        break;
      }

      case 'done': {
        message.isComplete = true;
        break;
      }

      case 'error': {
        // Add error as text block
        message.blocks.push({
          type: 'text',
          content: `Error: ${event.message}`,
        });
        message.isComplete = true;
        break;
      }

      case 'usage': {
        // Accumulate usage (multiple events possible in agentic loop)
        if (!message.usage) {
          message.usage = {
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 0,
          };
        }
        message.usage.input_tokens += event.input_tokens;
        message.usage.output_tokens += event.output_tokens;
        message.usage.reasoning_tokens += event.reasoning_tokens;
        message.usage.cached_tokens += event.cached_tokens;
        break;
      }

      // Ignore reasoning events for now
      case 'reasoning':
        break;
    }
  }

  return { message, handleEvent };
}
