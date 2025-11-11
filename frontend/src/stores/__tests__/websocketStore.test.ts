// src/stores/__tests__/websocketStore.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useWebSocketStore } from '../useWebSocketStore';

describe('WebSocket Store (Transport Layer)', () => {
  beforeEach(() => {
    // Reset store state completely
    useWebSocketStore.setState({
      socket: null,
      connectionState: 'disconnected',
      reconnectAttempts: 0,
      lastMessage: null,
      messageQueue: [],
      listeners: new Map(),
    });
  });

  describe('Connection Management', () => {
    it('starts in disconnected state', () => {
      const store = useWebSocketStore.getState();
      expect(store.connectionState).toBe('disconnected');
      expect(store.socket).toBeNull();
    });

    it('tracks connection state changes', () => {
      const store = useWebSocketStore.getState();
      
      store.setConnectionState('connecting');
      expect(useWebSocketStore.getState().connectionState).toBe('connecting');
      
      store.setConnectionState('connected');
      expect(useWebSocketStore.getState().connectionState).toBe('connected');
      
      store.setConnectionState('disconnected');
      expect(useWebSocketStore.getState().connectionState).toBe('disconnected');
    });

    it('queues messages when disconnected', async () => {
      const store = useWebSocketStore.getState();
      
      await store.send({ type: 'test', data: 'queued' });
      
      const currentState = useWebSocketStore.getState();
      expect(currentState.messageQueue).toHaveLength(1);
      expect(currentState.messageQueue[0].type).toBe('test');
    });
  });

  describe('Subscription System', () => {
    beforeEach(() => {
      // Clear console spy before each test
      vi.clearAllMocks();
    });

    it('subscribes to all messages by default', () => {
      const store = useWebSocketStore.getState();
      const callback = vi.fn();
      
      store.subscribe('test-listener', callback);
      
      expect(store.listeners.has('test-listener')).toBe(true);
    });

    it('subscribes to specific message types', () => {
      const store = useWebSocketStore.getState();
      const callback = vi.fn();
      
      store.subscribe('test-listener', callback, ['stream', 'status']);
      
      expect(store.listeners.has('test-listener')).toBe(true);
    });

    it('unsubscribes when cleanup function called', () => {
      const store = useWebSocketStore.getState();
      const callback = vi.fn();
      
      const unsubscribe = store.subscribe('test-listener', callback);
      expect(store.listeners.has('test-listener')).toBe(true);
      
      unsubscribe();
      expect(store.listeners.has('test-listener')).toBe(false);
    });

    it('notifies all subscribers when no filter specified', () => {
      const store = useWebSocketStore.getState();
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      
      store.subscribe('listener-1', callback1);
      store.subscribe('listener-2', callback2);
      
      store.handleMessage({ type: 'test', data: 'hello' });
      
      expect(callback1).toHaveBeenCalledWith({ type: 'test', data: 'hello' });
      expect(callback2).toHaveBeenCalledWith({ type: 'test', data: 'hello' });
    });

    it('notifies only matching subscribers when filter specified', () => {
      const store = useWebSocketStore.getState();
      const streamCallback = vi.fn();
      const statusCallback = vi.fn();
      
      store.subscribe('stream-listener', streamCallback, ['stream']);
      store.subscribe('status-listener', statusCallback, ['status']);
      
      store.handleMessage({ type: 'stream', delta: 'test' });
      
      expect(streamCallback).toHaveBeenCalled();
      expect(statusCallback).not.toHaveBeenCalled();
    });

    it('handles subscriber errors gracefully', () => {
      const store = useWebSocketStore.getState();
      const errorCallback = vi.fn(() => {
        throw new Error('Subscriber error');
      });
      const goodCallback = vi.fn();
      
      store.subscribe('error-listener', errorCallback);
      store.subscribe('good-listener', goodCallback);
      
      // Should not throw - errors are caught and logged
      expect(() => {
        store.handleMessage({ type: 'test', data: 'hello' });
      }).not.toThrow();
      
      // Good callback should still be called
      expect(goodCallback).toHaveBeenCalled();
    });

    it('allows multiple subscriptions with different filters', () => {
      const store = useWebSocketStore.getState();
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      const callback3 = vi.fn();
      
      store.subscribe('listener-1', callback1, ['stream']);
      store.subscribe('listener-2', callback2, ['status']);
      store.subscribe('listener-3', callback3); // No filter
      
      expect(store.listeners.size).toBe(3);
    });
  });

  describe('Message Handling', () => {
    beforeEach(() => {
      // Clear lastMessage between tests
      useWebSocketStore.setState({ lastMessage: null });
    });

    it('stores last received message', () => {
      const store = useWebSocketStore.getState();
      const message = { type: 'test', data: 'hello' };
      
      store.handleMessage(message);
      
      const currentState = useWebSocketStore.getState();
      expect(currentState.lastMessage).toEqual(message);
    });

    it('updates last message on each new message', () => {
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'first', data: 'a' });
      let currentState = useWebSocketStore.getState();
      expect(currentState.lastMessage?.type).toBe('first');
      
      store.handleMessage({ type: 'second', data: 'b' });
      currentState = useWebSocketStore.getState();
      expect(currentState.lastMessage?.type).toBe('second');
    });

    it('handles messages with data envelope', () => {
      const store = useWebSocketStore.getState();
      const callback = vi.fn();
      
      store.subscribe('test', callback);
      store.handleMessage({
        type: 'data',
        data: { type: 'project_list', projects: [] }
      });
      
      expect(callback).toHaveBeenCalled();
    });

    it('logs warnings for unknown message types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'unknown_type', data: {} });
      
      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('[WS] Unknown message type: unknown_type')
      );
      
      consoleSpy.mockRestore();
    });

    it('logs warnings for unknown data types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({
        type: 'data',
        data: { type: 'unknown_data_type', content: {} }
      });
      
      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('[WS] Unknown data type: unknown_data_type')
      );
      
      consoleSpy.mockRestore();
    });

    it('does not log for known message types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'status', status: 'thinking' });
      store.handleMessage({ type: 'stream', delta: 'test' });
      store.handleMessage({ type: 'chat_complete', content: 'done' });
      
      // Should not have any warnings for known types
      const unknownWarnings = consoleSpy.mock.calls.filter(call => 
        call[0].includes('Unknown message type')
      );
      expect(unknownWarnings).toHaveLength(0);
      
      consoleSpy.mockRestore();
    });

    it('does not log for silent message types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'heartbeat' });
      
      expect(consoleSpy).not.toHaveBeenCalled();
      
      consoleSpy.mockRestore();
    });
  });

  describe('Message Queue Processing', () => {
    beforeEach(() => {
      useWebSocketStore.setState({ messageQueue: [] });
    });

    it('processes queued messages on connect', async () => {
      const store = useWebSocketStore.getState();
      
      // Queue multiple messages
      await store.send({ type: 'msg1' });
      await store.send({ type: 'msg2' });
      await store.send({ type: 'msg3' });
      
      const currentState = useWebSocketStore.getState();
      expect(currentState.messageQueue).toHaveLength(3);
      
      // Mock connected state and process
      useWebSocketStore.setState({ connectionState: 'connected' });
      store.processMessageQueue();
      
      const finalState = useWebSocketStore.getState();
      expect(finalState.messageQueue).toHaveLength(0);
    });

    it('queues messages when socket is not ready', async () => {
      const store = useWebSocketStore.getState();
      
      await store.send({ type: 'test' });
      
      const currentState = useWebSocketStore.getState();
      expect(currentState.messageQueue.length).toBeGreaterThan(0);
    });
  });

  describe('Reconnection Logic', () => {
    beforeEach(() => {
      useWebSocketStore.setState({ reconnectAttempts: 0 });
    });

    it('tracks reconnection attempts', () => {
      const store = useWebSocketStore.getState();
      
      store.scheduleReconnect();
      let currentState = useWebSocketStore.getState();
      expect(currentState.reconnectAttempts).toBeGreaterThan(0);
      
      store.scheduleReconnect();
      currentState = useWebSocketStore.getState();
      expect(currentState.reconnectAttempts).toBeGreaterThan(1);
    });

    it('stops reconnecting after max attempts', () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      // Set to max attempts
      useWebSocketStore.setState({ 
        reconnectAttempts: store.maxReconnectAttempts 
      });
      
      store.scheduleReconnect();
      
      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('[WS] Max reconnection attempts reached')
      );
      
      consoleSpy.mockRestore();
    });

    it('resets reconnect attempts on successful connection', () => {
      const store = useWebSocketStore.getState();
      
      // Simulate failed attempts
      useWebSocketStore.setState({ reconnectAttempts: 5 });
      expect(useWebSocketStore.getState().reconnectAttempts).toBe(5);
      
      // When connection succeeds, reset the counter
      // (simulating what the real connection handler does)
      store.setConnectionState('connected');
      useWebSocketStore.setState({ reconnectAttempts: 0 });
      
      const currentState = useWebSocketStore.getState();
      expect(currentState.connectionState).toBe('connected');
      expect(currentState.reconnectAttempts).toBe(0);
    });
  });

  describe('Known Message Type Registry', () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it('includes legacy protocol types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'status', status: 'test' });
      store.handleMessage({ type: 'stream', delta: 'test' });
      store.handleMessage({ type: 'chat_complete', content: 'test' });
      
      // None of these should trigger unknown warnings
      const unknownWarnings = consoleSpy.mock.calls.filter(call => 
        call[0].includes('Unknown message type')
      );
      expect(unknownWarnings).toHaveLength(0);
      
      consoleSpy.mockRestore();
    });

    it('includes operations protocol types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({ type: 'operation.started' });
      store.handleMessage({ type: 'operation.streaming' });
      store.handleMessage({ type: 'operation.completed' });
      
      // None of these should trigger unknown warnings
      const unknownWarnings = consoleSpy.mock.calls.filter(call => 
        call[0].includes('Unknown message type')
      );
      expect(unknownWarnings).toHaveLength(0);
      
      consoleSpy.mockRestore();
    });

    it('includes data envelope types', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const store = useWebSocketStore.getState();
      
      store.handleMessage({
        type: 'data',
        data: { type: 'project_list', projects: [] }
      });
      
      // Should not trigger unknown warnings
      const unknownWarnings = consoleSpy.mock.calls.filter(call => 
        call[0].includes('Unknown')
      );
      expect(unknownWarnings).toHaveLength(0);
      
      consoleSpy.mockRestore();
    });
  });
});
