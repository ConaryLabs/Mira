// frontend/src/hooks/__tests__/useMessageHandler.test.ts
// Message Handler Hook Tests

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, cleanup } from '@testing-library/react';
import { useMessageHandler } from '../useMessageHandler';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { useChatStore } from '../../stores/useChatStore';
import { useAppState } from '../../stores/useAppState';

// Mock the stores
vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

vi.mock('../../stores/useChatStore', () => ({
  useChatStore: vi.fn(),
}));

vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
}));

describe('useMessageHandler', () => {
  let mockSubscribe: ReturnType<typeof vi.fn>;
  let mockUnsubscribe: ReturnType<typeof vi.fn>;
  let mockAddMessage: ReturnType<typeof vi.fn>;
  let mockStartStreaming: ReturnType<typeof vi.fn>;
  let mockAppendStreamContent: ReturnType<typeof vi.fn>;
  let mockEndStreaming: ReturnType<typeof vi.fn>;
  let mockAddToolExecution: ReturnType<typeof vi.fn>;
  let mockAddToast: ReturnType<typeof vi.fn>;
  let mockAddArtifact: ReturnType<typeof vi.fn>;
  let messageHandler: ((message: any) => void) | null = null;

  beforeEach(() => {
    // Reset all mocks
    vi.clearAllMocks();
    messageHandler = null;

    // Setup WebSocketStore mock
    mockUnsubscribe = vi.fn();
    mockSubscribe = vi.fn((id, handler, types) => {
      messageHandler = handler;
      return mockUnsubscribe;
    });

    (useWebSocketStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ subscribe: mockSubscribe });
      }
      return { subscribe: mockSubscribe };
    });

    // Setup ChatStore mock
    mockAddMessage = vi.fn();
    mockStartStreaming = vi.fn();
    mockAppendStreamContent = vi.fn();
    mockEndStreaming = vi.fn();
    mockAddToolExecution = vi.fn();

    (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      const state = {
        addMessage: mockAddMessage,
        startStreaming: mockStartStreaming,
        appendStreamContent: mockAppendStreamContent,
        endStreaming: mockEndStreaming,
        addToolExecution: mockAddToolExecution,
        messages: [],
        streamingMessageId: null,
      };
      if (typeof selector === 'function') {
        return selector(state);
      }
      return state;
    });

    // Setup useChatStore.getState mock
    (useChatStore as any).getState = vi.fn(() => ({
      messages: [],
      streamingMessageId: null,
    }));

    // Setup AppState mock
    mockAddToast = vi.fn();
    mockAddArtifact = vi.fn();

    (useAppState as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      const state = {
        addToast: mockAddToast,
        addArtifact: mockAddArtifact,
      };
      if (typeof selector === 'function') {
        return selector(state);
      }
      return state;
    });

    // Setup useAppState.getState mock
    (useAppState as any).getState = vi.fn(() => ({
      addArtifact: mockAddArtifact,
    }));
  });

  afterEach(() => {
    cleanup();
  });

  it('should subscribe to websocket messages on mount', () => {
    renderHook(() => useMessageHandler());

    expect(mockSubscribe).toHaveBeenCalledWith(
      'chat-handler',
      expect.any(Function),
      ['response', 'stream', 'status', 'chat_complete', 'operation.tool_executed']
    );
  });

  it('should unsubscribe on unmount', () => {
    const { unmount } = renderHook(() => useMessageHandler());

    unmount();

    expect(mockUnsubscribe).toHaveBeenCalled();
  });

  it('should handle status message with thinking status', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    act(() => {
      messageHandler!({ type: 'status', status: 'thinking' });
    });

    expect(mockStartStreaming).toHaveBeenCalled();
  });

  it('should handle stream message with delta', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    act(() => {
      messageHandler!({ type: 'stream', delta: 'Hello, world!' });
    });

    expect(mockAppendStreamContent).toHaveBeenCalledWith('Hello, world!');
  });

  it('should handle chat_complete message', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    const message = {
      type: 'chat_complete',
      content: 'Test response content',
      thinking: 'some thinking',
      artifacts: [],
    };

    act(() => {
      messageHandler!(message);
    });

    expect(mockEndStreaming).toHaveBeenCalled();
    expect(mockAddMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        role: 'assistant',
        content: 'Test response content',
        thinking: 'some thinking',
      })
    );
  });

  it('should handle legacy response message', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    const message = {
      type: 'response',
      content: 'Legacy response',
      artifacts: [],
    };

    act(() => {
      messageHandler!(message);
    });

    expect(mockAddMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        role: 'assistant',
        content: 'Legacy response',
      })
    );
  });

  it('should handle streaming response message', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    act(() => {
      messageHandler!({ type: 'response', streaming: true, content: 'chunk1' });
    });

    expect(mockAppendStreamContent).toHaveBeenCalledWith('chunk1');
    expect(mockAddMessage).not.toHaveBeenCalled();
  });

  it('should end streaming when complete flag received', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    act(() => {
      messageHandler!({ type: 'response', complete: true });
    });

    expect(mockEndStreaming).toHaveBeenCalled();
  });

  it('should handle tool_executed message with file operations', () => {
    // Setup a message to target
    (useChatStore as any).getState = vi.fn(() => ({
      messages: [{ id: 'msg-1', role: 'assistant' }],
      streamingMessageId: null,
    }));

    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    const toolMessage = {
      type: 'operation.tool_executed',
      tool_type: 'file_write',
      tool_name: 'write_file',
      summary: 'Created src/test.ts',
      success: true,
      details: { path: 'src/test.ts' },
    };

    act(() => {
      messageHandler!(toolMessage);
    });

    // Should show toast for file operations
    expect(mockAddToast).toHaveBeenCalledWith({
      type: 'success',
      message: 'Created src/test.ts',
      duration: 4000,
    });

    // Should add tool execution to message
    expect(mockAddToolExecution).toHaveBeenCalledWith(
      'msg-1',
      expect.objectContaining({
        toolName: 'write_file',
        toolType: 'file_write',
        summary: 'Created src/test.ts',
        success: true,
      })
    );
  });

  it('should show error toast for failed file operations', () => {
    (useChatStore as any).getState = vi.fn(() => ({
      messages: [{ id: 'msg-1', role: 'assistant' }],
      streamingMessageId: null,
    }));

    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    const toolMessage = {
      type: 'operation.tool_executed',
      tool_type: 'file_write',
      tool_name: 'write_file',
      summary: 'Failed to write file',
      success: false,
    };

    act(() => {
      messageHandler!(toolMessage);
    });

    expect(mockAddToast).toHaveBeenCalledWith({
      type: 'error',
      message: 'Failed to write file',
      duration: 4000,
    });
  });

  it('should handle dataType field for message type detection', () => {
    renderHook(() => useMessageHandler());

    expect(messageHandler).not.toBeNull();

    // Message with dataType field (alternative to type)
    const message = {
      dataType: 'stream',
      delta: 'Hello via dataType',
    };

    act(() => {
      messageHandler!(message);
    });

    expect(mockAppendStreamContent).toHaveBeenCalledWith('Hello via dataType');
  });
});
