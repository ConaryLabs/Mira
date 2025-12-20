/**
 * Stream State Machine
 *
 * Centralizes streaming lifecycle management with explicit states:
 * - idle: No active stream
 * - streaming: Active stream in progress
 * - error: Stream failed with error
 * - cancelled: Stream was cancelled by user
 */

import type { MessageBlock, UsageInfo } from '$lib/api/client';

// State types
export type StreamState =
  | { status: 'idle' }
  | { status: 'streaming'; messageId: string; blocks: MessageBlock[]; usage?: UsageInfo; controller: AbortController }
  | { status: 'error'; error: Error; messageId: string; blocks: MessageBlock[] }
  | { status: 'cancelled'; messageId: string; blocks: MessageBlock[] };

// Initial state
const initialState: StreamState = { status: 'idle' };

// Reactive state
let state = $state<StreamState>({ ...initialState });

/**
 * Stream state store with derived values and actions
 */
export const streamState = {
  // Raw state access
  get current() { return state; },
  get status() { return state.status; },

  // Derived values
  get isLoading() {
    return state.status === 'streaming';
  },

  get canCancel() {
    return state.status === 'streaming';
  },

  get streamingMessage() {
    if (state.status === 'streaming') {
      return {
        id: state.messageId,
        blocks: state.blocks,
        usage: state.usage,
      };
    }
    return null;
  },

  get abortController() {
    if (state.status === 'streaming') {
      return state.controller;
    }
    return null;
  },

  // Actions
  startStream(messageId: string): AbortController {
    const controller = new AbortController();
    state = {
      status: 'streaming',
      messageId,
      blocks: [],
      controller,
    };
    return controller;
  },

  updateStream(blocks: MessageBlock[], usage?: UsageInfo) {
    if (state.status === 'streaming') {
      state = {
        ...state,
        blocks,
        usage,
      };
    }
  },

  completeStream() {
    state = { status: 'idle' };
  },

  cancelStream() {
    if (state.status === 'streaming') {
      state.controller.abort();
      state = {
        status: 'cancelled',
        messageId: state.messageId,
        blocks: state.blocks,
      };
    }
  },

  errorStream(error: Error) {
    if (state.status === 'streaming') {
      state = {
        status: 'error',
        error,
        messageId: state.messageId,
        blocks: state.blocks,
      };
    }
  },

  reset() {
    state = { status: 'idle' };
  },

  // Get final message after stream ends (for cancelled/error states)
  getFinalBlocks(): MessageBlock[] {
    if (state.status === 'cancelled' || state.status === 'error') {
      return state.blocks;
    }
    if (state.status === 'streaming') {
      return state.blocks;
    }
    return [];
  },

  getMessageId(): string | null {
    if (state.status !== 'idle') {
      return state.messageId;
    }
    return null;
  },
};
