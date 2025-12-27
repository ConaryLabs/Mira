/**
 * Orchestration Store
 *
 * Manages real-time orchestration state via SSE:
 * - Instructions queue (pending/in_progress/completed)
 * - MCP tool call activity
 *
 * Uses EventSource for live updates instead of polling.
 */

export interface InstructionEntry {
  id: string;
  instruction: string;
  context: string | null;
  priority: string;
  status: string;
  created_at: string;
  completed_at: string | null;
  result: string | null;
  error: string | null;
}

export interface McpHistoryEntry {
  id: number;
  tool_name: string;
  args_preview: string;
  result_summary: string | null;
  success: boolean;
  duration_ms: number | null;
  created_at: string;
}

export type OrchestrationEvent =
  | { type: 'instruction_update'; instruction: InstructionEntry }
  | { type: 'mcp_activity'; entry: McpHistoryEntry }
  | { type: 'heartbeat'; ts: number };

interface OrchestrationState {
  instructions: Map<string, InstructionEntry>;
  mcpHistory: McpHistoryEntry[];
  connected: boolean;
  lastHeartbeat: number | null;
}

// Reactive state
let state = $state<OrchestrationState>({
  instructions: new Map(),
  mcpHistory: [],
  connected: false,
  lastHeartbeat: null,
});

// SSE connection
let eventSource: EventSource | null = null;

/**
 * Orchestration Store
 */
export const orchestrationStore = {
  // State access
  get instructions() { return state.instructions; },
  get mcpHistory() { return state.mcpHistory; },
  get connected() { return state.connected; },
  get lastHeartbeat() { return state.lastHeartbeat; },

  // Derived: instructions as sorted array
  get instructionsList(): InstructionEntry[] {
    return Array.from(state.instructions.values())
      .sort((a, b) => {
        // Sort by status priority, then by created_at
        const statusOrder: Record<string, number> = {
          'in_progress': 0,
          'pending': 1,
          'delivered': 2,
          'completed': 3,
          'failed': 4,
          'cancelled': 5,
        };
        const aOrder = statusOrder[a.status] ?? 6;
        const bOrder = statusOrder[b.status] ?? 6;
        if (aOrder !== bOrder) return aOrder - bOrder;
        return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
      });
  },

  // Derived: active instruction count
  get activeCount(): number {
    return Array.from(state.instructions.values())
      .filter(i => i.status === 'pending' || i.status === 'in_progress' || i.status === 'delivered')
      .length;
  },

  // Actions

  /**
   * Connect to the SSE stream
   */
  connect() {
    if (eventSource) {
      this.disconnect();
    }

    eventSource = new EventSource('/api/orchestration/stream');

    eventSource.onopen = () => {
      state.connected = true;
    };

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as OrchestrationEvent;
        this.handleEvent(data);
      } catch (e) {
        console.warn('Failed to parse orchestration event:', e);
      }
    };

    eventSource.onerror = () => {
      state.connected = false;
      // Reconnect after delay
      setTimeout(() => {
        if (eventSource?.readyState === EventSource.CLOSED) {
          this.connect();
        }
      }, 3000);
    };
  },

  /**
   * Disconnect from the SSE stream
   */
  disconnect() {
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
    state.connected = false;
  },

  /**
   * Handle incoming SSE event
   */
  handleEvent(event: OrchestrationEvent) {
    switch (event.type) {
      case 'instruction_update': {
        const instr = event.instruction;
        state.instructions.set(instr.id, instr);
        // Force reactivity by creating new Map
        state.instructions = new Map(state.instructions);
        break;
      }

      case 'mcp_activity': {
        const entry = event.entry;
        // Add to front if new, or update existing
        const existingIndex = state.mcpHistory.findIndex(e => e.id === entry.id);
        if (existingIndex >= 0) {
          state.mcpHistory[existingIndex] = entry;
          state.mcpHistory = [...state.mcpHistory];
        } else {
          // Add to front, keep max 100 entries
          state.mcpHistory = [entry, ...state.mcpHistory].slice(0, 100);
        }
        break;
      }

      case 'heartbeat': {
        state.lastHeartbeat = event.ts;
        break;
      }
    }
  },

  /**
   * Create a new instruction
   */
  async createInstruction(instruction: string, priority: string = 'normal'): Promise<{ id: string } | null> {
    try {
      const res = await fetch('/api/instructions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ instruction, priority }),
      });
      if (res.ok) {
        return await res.json();
      }
    } catch (e) {
      console.error('Failed to create instruction:', e);
    }
    return null;
  },

  /**
   * Clear all data
   */
  clear() {
    state.instructions = new Map();
    state.mcpHistory = [];
  },

  /**
   * Get instruction by ID
   */
  getInstruction(id: string): InstructionEntry | undefined {
    return state.instructions.get(id);
  },
};
