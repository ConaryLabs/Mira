/**
 * Tool Activity Store
 *
 * Tracks all tool calls during chat streaming for the Timeline panel.
 * Receives events from the SSE stream and maintains a live feed.
 */

export type ToolStatus = 'running' | 'done' | 'error';

export type ToolCategory = 'file' | 'shell' | 'memory' | 'web' | 'git' | 'mira' | 'other';

export interface ToolCall {
  callId: string;
  name: string;
  arguments: Record<string, unknown>;
  messageId: string;
  seq: number;
  startedAt: number; // timestamp ms
  summary: string;
  category: ToolCategory;
  status: ToolStatus;
  // Result fields (populated when done)
  success?: boolean;
  output?: string;
  durationMs?: number;
  truncated?: boolean;
  totalBytes?: number;
  exitCode?: number;
  stderr?: string;
}

export interface ToolActivityFilter {
  category?: ToolCategory;
  status?: ToolStatus;
}

interface ToolActivityState {
  calls: Map<string, ToolCall>;
  order: string[];  // call IDs ordered by seq
  filter: ToolActivityFilter;
  expandedIds: Set<string>;
}

// Reactive state
let state = $state<ToolActivityState>({
  calls: new Map(),
  order: [],
  filter: {},
  expandedIds: new Set(),
});

/**
 * Tool Activity Store
 */
export const toolActivityStore = {
  // State access
  get calls() { return state.calls; },
  get order() { return state.order; },
  get filter() { return state.filter; },
  get expandedIds() { return state.expandedIds; },

  // Derived: filtered and ordered tool calls
  get filteredCalls(): ToolCall[] {
    const { category, status } = state.filter;
    return state.order
      .map(id => state.calls.get(id)!)
      .filter(call => {
        if (category && call.category !== category) return false;
        if (status && call.status !== status) return false;
        return true;
      });
  },

  // Derived: active (running) tool count
  get activeCount(): number {
    return Array.from(state.calls.values())
      .filter(c => c.status === 'running')
      .length;
  },

  // Actions

  /**
   * Handle tool call start event from SSE
   */
  toolStarted(event: {
    call_id: string;
    name: string;
    arguments: Record<string, unknown>;
    message_id: string;
    seq: number;
    ts_ms: number;
    summary: string;
    category: string;
  }) {
    const call: ToolCall = {
      callId: event.call_id,
      name: event.name,
      arguments: event.arguments,
      messageId: event.message_id,
      seq: event.seq,
      startedAt: event.ts_ms,
      summary: event.summary,
      category: event.category as ToolCategory,
      status: 'running',
    };

    state.calls.set(event.call_id, call);

    // Insert in order by seq
    const insertIndex = state.order.findIndex(id => {
      const existing = state.calls.get(id);
      return existing && existing.seq > event.seq;
    });

    if (insertIndex === -1) {
      state.order = [...state.order, event.call_id];
    } else {
      state.order = [
        ...state.order.slice(0, insertIndex),
        event.call_id,
        ...state.order.slice(insertIndex),
      ];
    }
  },

  /**
   * Handle tool call result event from SSE
   */
  toolCompleted(event: {
    call_id: string;
    success: boolean;
    output: string;
    duration_ms: number;
    truncated: boolean;
    total_bytes: number;
    exit_code?: number;
    stderr?: string;
  }) {
    const existing = state.calls.get(event.call_id);
    if (!existing) return;

    const updated: ToolCall = {
      ...existing,
      status: event.success ? 'done' : 'error',
      success: event.success,
      output: event.output,
      durationMs: event.duration_ms,
      truncated: event.truncated,
      totalBytes: event.total_bytes,
      exitCode: event.exit_code,
      stderr: event.stderr,
    };

    state.calls.set(event.call_id, updated);
  },

  /**
   * Toggle expansion state of a tool call card
   */
  toggleExpanded(callId: string) {
    const newSet = new Set(state.expandedIds);
    if (newSet.has(callId)) {
      newSet.delete(callId);
    } else {
      newSet.add(callId);
    }
    state.expandedIds = newSet;
  },

  /**
   * Set filter
   */
  setFilter(filter: ToolActivityFilter) {
    state.filter = filter;
  },

  /**
   * Clear filter
   */
  clearFilter() {
    state.filter = {};
  },

  /**
   * Clear all tool activity (e.g., on new conversation)
   */
  clear() {
    state.calls = new Map();
    state.order = [];
    state.expandedIds = new Set();
  },

  /**
   * Get a specific tool call by ID
   */
  getCall(callId: string): ToolCall | undefined {
    return state.calls.get(callId);
  },

  /**
   * Get tool calls for a specific message
   */
  getCallsForMessage(messageId: string): ToolCall[] {
    return state.order
      .map(id => state.calls.get(id)!)
      .filter(call => call.messageId === messageId);
  },

  /**
   * Scroll to a tool call in the chat view
   */
  scrollToChat(callId: string) {
    const element = document.getElementById(`tool-call-${callId}`);
    if (element) {
      element.scrollIntoView({ behavior: 'smooth', block: 'center' });
      // Flash highlight
      element.classList.add('highlight-flash');
      setTimeout(() => element.classList.remove('highlight-flash'), 1500);
    }
  },
};
