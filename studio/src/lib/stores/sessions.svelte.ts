/**
 * Claude Code Sessions Store
 *
 * Manages spawned Claude Code sessions via SSE:
 * - Active sessions list
 * - Pending questions
 * - Real-time session events
 */

export interface SessionInfo {
  session_id: string;
  status: string;
  project_path?: string;
  initial_prompt?: string;
  spawned_at?: number;
}

export interface PendingQuestion {
  question_id: string;
  session_id: string;
  question: string;
  options?: QuestionOption[];
}

export interface QuestionOption {
  label: string;
  description?: string;
}

export type SessionEvent =
  | { type: 'started'; session_id: string; project_path: string; initial_prompt: string }
  | { type: 'status_changed'; session_id: string; status: string; phase?: string }
  | { type: 'output'; session_id: string; chunk_type: string; content: string }
  | { type: 'tool_call'; session_id: string; tool_name: string; tool_id: string; input_preview: string }
  | { type: 'question_pending'; question_id: string; session_id: string; question: string; options?: QuestionOption[] }
  | { type: 'ended'; session_id: string; status: string; exit_code?: number; summary?: string }
  | { type: 'heartbeat'; ts: number };

interface SessionsState {
  sessions: Map<string, SessionInfo>;
  pendingQuestions: Map<string, PendingQuestion>;
  recentOutput: { session_id: string; content: string; timestamp: number }[];
  connected: boolean;
  lastHeartbeat: number | null;
}

// Reactive state
let state = $state<SessionsState>({
  sessions: new Map(),
  pendingQuestions: new Map(),
  recentOutput: [],
  connected: false,
  lastHeartbeat: null,
});

// SSE connection
let eventSource: EventSource | null = null;

/**
 * Sessions Store
 */
export const sessionsStore = {
  // State access
  get sessions() { return state.sessions; },
  get pendingQuestions() { return state.pendingQuestions; },
  get recentOutput() { return state.recentOutput; },
  get connected() { return state.connected; },
  get lastHeartbeat() { return state.lastHeartbeat; },

  // Derived: sessions as sorted array
  get sessionsList(): SessionInfo[] {
    return Array.from(state.sessions.values())
      .sort((a, b) => {
        // Sort by status (active first), then by spawn time
        const statusOrder: Record<string, number> = {
          'running': 0,
          'starting': 1,
          'paused': 2,
          'pending': 3,
          'completed': 4,
          'failed': 5,
        };
        const aOrder = statusOrder[a.status] ?? 6;
        const bOrder = statusOrder[b.status] ?? 6;
        if (aOrder !== bOrder) return aOrder - bOrder;
        return (b.spawned_at ?? 0) - (a.spawned_at ?? 0);
      });
  },

  // Derived: active session count
  get activeCount(): number {
    return Array.from(state.sessions.values())
      .filter(s => s.status === 'running' || s.status === 'starting' || s.status === 'paused')
      .length;
  },

  // Derived: questions list
  get questionsList(): PendingQuestion[] {
    return Array.from(state.pendingQuestions.values());
  },

  // Actions

  /**
   * Connect to the SSE stream
   */
  connect() {
    if (eventSource) {
      this.disconnect();
    }

    eventSource = new EventSource('/api/sessions/events');

    eventSource.onopen = () => {
      state.connected = true;
      // Fetch initial session list
      this.fetchSessions();
    };

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as SessionEvent;
        this.handleEvent(data);
      } catch (e) {
        console.warn('Failed to parse session event:', e);
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
  handleEvent(event: SessionEvent) {
    switch (event.type) {
      case 'started': {
        const session: SessionInfo = {
          session_id: event.session_id,
          status: 'starting',
          project_path: event.project_path,
          initial_prompt: event.initial_prompt,
          spawned_at: Date.now(),
        };
        state.sessions.set(event.session_id, session);
        state.sessions = new Map(state.sessions);
        break;
      }

      case 'status_changed': {
        const existing = state.sessions.get(event.session_id);
        if (existing) {
          existing.status = event.status;
          state.sessions = new Map(state.sessions);
        }
        break;
      }

      case 'output': {
        // Keep last 50 output chunks
        state.recentOutput = [
          { session_id: event.session_id, content: event.content, timestamp: Date.now() },
          ...state.recentOutput,
        ].slice(0, 50);
        break;
      }

      case 'question_pending': {
        const question: PendingQuestion = {
          question_id: event.question_id,
          session_id: event.session_id,
          question: event.question,
          options: event.options,
        };
        state.pendingQuestions.set(event.question_id, question);
        state.pendingQuestions = new Map(state.pendingQuestions);
        break;
      }

      case 'ended': {
        const existing = state.sessions.get(event.session_id);
        if (existing) {
          existing.status = event.status;
          state.sessions = new Map(state.sessions);
        }
        // Remove any pending questions for this session
        for (const [qid, q] of state.pendingQuestions) {
          if (q.session_id === event.session_id) {
            state.pendingQuestions.delete(qid);
          }
        }
        state.pendingQuestions = new Map(state.pendingQuestions);
        break;
      }

      case 'heartbeat': {
        state.lastHeartbeat = event.ts;
        break;
      }
    }
  },

  /**
   * Fetch current sessions list
   */
  async fetchSessions(): Promise<void> {
    try {
      const res = await fetch('/api/sessions');
      if (res.ok) {
        const sessions: SessionInfo[] = await res.json();
        state.sessions = new Map(sessions.map(s => [s.session_id, s]));
      }
    } catch (e) {
      console.error('Failed to fetch sessions:', e);
    }
  },

  /**
   * Spawn a new session
   */
  async spawnSession(
    projectPath: string,
    prompt: string,
    options?: { budgetUsd?: number; systemPrompt?: string }
  ): Promise<{ session_id: string } | null> {
    try {
      const res = await fetch('/api/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          project_path: projectPath,
          prompt,
          budget_usd: options?.budgetUsd,
          system_prompt: options?.systemPrompt,
        }),
      });
      if (res.ok) {
        return await res.json();
      }
    } catch (e) {
      console.error('Failed to spawn session:', e);
    }
    return null;
  },

  /**
   * Answer a pending question
   */
  async answerQuestion(questionId: string, answer: string): Promise<boolean> {
    try {
      const res = await fetch('/api/sessions/answer', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ question_id: questionId, answer }),
      });
      if (res.ok) {
        // Remove from pending
        state.pendingQuestions.delete(questionId);
        state.pendingQuestions = new Map(state.pendingQuestions);
        return true;
      }
    } catch (e) {
      console.error('Failed to answer question:', e);
    }
    return false;
  },

  /**
   * Terminate a session
   */
  async terminateSession(sessionId: string): Promise<boolean> {
    try {
      const res = await fetch('/api/sessions/terminate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ session_id: sessionId }),
      });
      if (res.ok) {
        return true;
      }
    } catch (e) {
      console.error('Failed to terminate session:', e);
    }
    return false;
  },

  /**
   * Clear all data
   */
  clear() {
    state.sessions = new Map();
    state.pendingQuestions = new Map();
    state.recentOutput = [];
  },

  /**
   * Get session by ID
   */
  getSession(id: string): SessionInfo | undefined {
    return state.sessions.get(id);
  },
};
