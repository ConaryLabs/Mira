// src/stores/__tests__/chatStore.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { useChatStore } from '../useChatStore';

describe('useChatStore - Streaming Logic', () => {
  beforeEach(() => {
    localStorage.clear();
    useChatStore.setState({
      messages: [],
      isWaitingForResponse: false,
      isStreaming: false,
      streamingContent: '',
      streamingMessageId: null,
    });
  });

  it('accumulates stream content', () => {
    const store = useChatStore.getState();
    
    store.startStreaming();
    store.appendStreamContent('Hello');
    store.appendStreamContent(' ');
    store.appendStreamContent('world');
    
    const currentState = useChatStore.getState();
    expect(currentState.streamingContent).toBe('Hello world');
    expect(currentState.isStreaming).toBe(true);
  });

  it('creates message when stream completes', () => {
    const store = useChatStore.getState();
    
    store.startStreaming();
    const streamId = useChatStore.getState().streamingMessageId;
    store.appendStreamContent('Complete message');
    store.endStreaming();
    
    const currentState = useChatStore.getState();
    expect(currentState.messages).toHaveLength(1);
    expect(currentState.messages[0]).toMatchObject({
      id: streamId,
      role: 'assistant',
      content: 'Complete message',
    });
    expect(currentState.isStreaming).toBe(false);
    expect(currentState.streamingContent).toBe('');
  });

  it('handles empty streams gracefully', () => {
    const store = useChatStore.getState();
    
    store.startStreaming();
    store.endStreaming();
    
    const currentState = useChatStore.getState();
    // Empty streams shouldn't create messages
    expect(currentState.messages).toHaveLength(0);
    expect(currentState.isStreaming).toBe(false);
  });

  it('clears streaming state without creating message', () => {
    const store = useChatStore.getState();
    
    store.startStreaming();
    store.appendStreamContent('partial content');
    store.clearStreaming();
    
    const currentState = useChatStore.getState();
    expect(currentState.isStreaming).toBe(false);
    expect(currentState.streamingContent).toBe('');
    expect(currentState.streamingMessageId).toBeNull();
    expect(currentState.messages).toHaveLength(0);
  });

  it('generates unique stream IDs', async () => {
    const store = useChatStore.getState();
    
    store.startStreaming();
    const firstId = useChatStore.getState().streamingMessageId;
    store.endStreaming();
    
    // Wait 1ms to ensure different timestamp
    await new Promise(resolve => setTimeout(resolve, 1));
    
    store.startStreaming();
    const secondId = useChatStore.getState().streamingMessageId;
    
    expect(firstId).not.toBeNull();
    expect(secondId).not.toBeNull();
    expect(firstId).not.toBe(secondId);
  });

  it('updates waiting state when adding messages', () => {
    const store = useChatStore.getState();
    
    // Manually set state
    useChatStore.setState({ isWaitingForResponse: true });
    expect(useChatStore.getState().isWaitingForResponse).toBe(true);
    
    store.addMessage({
      id: 'msg-1',
      role: 'assistant',
      content: 'Response',
      timestamp: Date.now(),
    });
    
    const currentState = useChatStore.getState();
    expect(currentState.isWaitingForResponse).toBe(false);
  });
});

describe('useChatStore - Message Management', () => {
  beforeEach(() => {
    localStorage.clear();
    useChatStore.setState({
      messages: [],
      isWaitingForResponse: false,
      isStreaming: false,
      streamingContent: '',
      streamingMessageId: null,
    });
  });

  it('adds messages to history', () => {
    const store = useChatStore.getState();
    
    store.addMessage({
      id: 'msg-1',
      role: 'user',
      content: 'Hello',
      timestamp: Date.now(),
    });
    
    store.addMessage({
      id: 'msg-2',
      role: 'assistant',
      content: 'Hi there',
      timestamp: Date.now(),
    });
    
    const currentState = useChatStore.getState();
    expect(currentState.messages).toHaveLength(2);
    expect(currentState.messages[0].role).toBe('user');
    expect(currentState.messages[1].role).toBe('assistant');
  });

  it('updates existing messages', () => {
    const store = useChatStore.getState();
    
    store.addMessage({
      id: 'msg-1',
      role: 'assistant',
      content: 'Original',
      timestamp: Date.now(),
    });
    
    store.updateMessage('msg-1', { content: 'Updated' });
    
    const currentState = useChatStore.getState();
    expect(currentState.messages[0].content).toBe('Updated');
  });

  it('preserves artifacts in messages', () => {
    const store = useChatStore.getState();
    
    const artifact = {
      id: 'art-1',
      path: 'test.ts',
      content: 'const x = 1;',
      language: 'typescript',
    };
    
    store.addMessage({
      id: 'msg-1',
      role: 'assistant',
      content: 'Here is your code',
      artifacts: [artifact],
      timestamp: Date.now(),
    });
    
    const currentState = useChatStore.getState();
    expect(currentState.messages[0].artifacts).toHaveLength(1);
    expect(currentState.messages[0].artifacts![0]).toMatchObject(artifact);
  });

  it('clears all messages', () => {
    const store = useChatStore.getState();
    
    store.addMessage({ id: 'msg-1', role: 'user', content: 'Test', timestamp: Date.now() });
    store.addMessage({ id: 'msg-2', role: 'assistant', content: 'Response', timestamp: Date.now() });
    
    store.clearMessages();
    
    const currentState = useChatStore.getState();
    expect(currentState.messages).toHaveLength(0);
  });
});
