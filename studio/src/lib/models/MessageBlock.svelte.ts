/**
 * MessageBlockModel - Svelte 5 class with $derived parsing
 *
 * Encapsulates message block data with reactive parsing.
 * Parsing only runs when content actually changes (via $derived),
 * not on every render cycle.
 */

import { parseTextContent, type ParseResult } from '$lib/parser/contentParser';

export class MessageBlockModel {
  id: string;
  type: 'text' | 'tool_call';
  name?: string;
  arguments?: Record<string, unknown>;
  result?: string;

  // Content is reactive state
  content = $state('');

  // Track if this block is still streaming
  isStreaming = $state(true);

  constructor(data: {
    id: string;
    type: 'text' | 'tool_call';
    content?: string;
    name?: string;
    arguments?: Record<string, unknown>;
    result?: string;
  }) {
    this.id = data.id;
    this.type = data.type;
    this.content = data.content || '';
    this.name = data.name;
    this.arguments = data.arguments;
    this.result = data.result;
  }

  // $derived only recalculates when this.content or this.isStreaming changes
  // This keeps expensive parsing out of the render loop
  parsed = $derived.by(() => {
    return parseTextContent(this.content, this.id, this.isStreaming);
  });

  // Append streaming content
  append(chunk: string) {
    this.content += chunk;
  }

  // Mark block as finalized (triggers full parsing)
  finalize() {
    this.isStreaming = false;
  }

  // Update from raw block data
  update(data: { content?: string; result?: string }) {
    if (data.content !== undefined) {
      this.content = data.content;
    }
    if (data.result !== undefined) {
      this.result = data.result;
    }
  }
}

/**
 * MessageModel - Wraps a full message with reactive blocks
 */
export class MessageModel {
  id: string;
  role: 'user' | 'assistant';
  blocks: MessageBlockModel[] = $state([]);
  usage = $state<{
    input_tokens: number;
    output_tokens: number;
    cached_tokens: number;
    reasoning_tokens: number;
  } | null>(null);

  constructor(data: {
    id: string;
    role: 'user' | 'assistant';
    blocks?: Array<{
      id?: string;
      type: 'text' | 'tool_call';
      content?: string;
      name?: string;
      arguments?: Record<string, unknown>;
      result?: string;
    }>;
    usage?: {
      input_tokens: number;
      output_tokens: number;
      cached_tokens: number;
      reasoning_tokens: number;
    };
  }) {
    this.id = data.id;
    this.role = data.role;
    this.usage = data.usage || null;

    if (data.blocks) {
      this.blocks = data.blocks.map((b, i) => new MessageBlockModel({
        id: b.id || `${data.id}-${i}`,
        type: b.type,
        content: b.content,
        name: b.name,
        arguments: b.arguments,
        result: b.result,
      }));
      // Mark all blocks as finalized for completed messages
      this.blocks.forEach(b => b.finalize());
    }
  }

  // Add a new block (during streaming)
  addBlock(data: {
    type: 'text' | 'tool_call';
    content?: string;
    name?: string;
    arguments?: Record<string, unknown>;
  }): MessageBlockModel {
    const block = new MessageBlockModel({
      id: `${this.id}-${this.blocks.length}`,
      type: data.type,
      content: data.content,
      name: data.name,
      arguments: data.arguments,
    });
    this.blocks.push(block);
    return block;
  }

  // Finalize all blocks
  finalize() {
    this.blocks.forEach(b => b.finalize());
  }
}
