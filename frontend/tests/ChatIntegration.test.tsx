// tests/ChatIntegration.test.tsx
// Integration tests - component + store interactions
// FIXED: Added useArtifactState mock, fixed session ID expectations, proper store setup

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatArea } from '../src/components/ChatArea';
import { useChatStore } from '../src/stores/useChatStore';
import { useWebSocketStore } from '../src/stores/useWebSocketStore';
import { useAppState } from '../src/stores/useAppState';

// CRITICAL FIX: Preserve real store, only mock useArtifactState
vi.mock('../src/stores/useAppState', async () => {
  const actual = await vi.importActual('../src/stores/useAppState');
  return {
    ...actual,
    useArtifactState: vi.fn(() => ({
      activeArtifact: null,
      artifacts: [],
      showArtifacts: false,
      appliedFiles: new Set(),
      addArtifact: vi.fn(),
      setActiveArtifact: vi.fn(),
      updateArtifact: vi.fn(),
      removeArtifact: vi.fn(),
      markArtifactApplied: vi.fn(),
      markArtifactUnapplied: vi.fn(),
      isArtifactApplied: vi.fn(),
    })),
  };
});

// CRITICAL FIX: Mock Virtuoso to render all items
vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, components }: any) => {
    return (
      <div data-testid="virtuoso-list">
        {data?.map((item: any, index: number) => (
          <div key={item.id || index}>
            {itemContent(index, item)}
          </div>
        ))}
        {components?.Footer && <components.Footer />}
      </div>
    );
  },
}));

const mockSend = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  
  // Reset stores to initial state
  useChatStore.getState().clearMessages();
  useChatStore.getState().setSessionId('peter-eternal'); // FIXED: Use actual session ID
  useChatStore.getState().setWaitingForResponse(false);
  
  // Mock WebSocket send
  useWebSocketStore.setState({ 
    send: mockSend, 
    connectionState: 'connected' 
  });
  
  // Mock app state
  useAppState.setState({ 
    currentProject: { 
      id: 'test-project',
      name: 'Test Project',
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    },
    modifiedFiles: [],
    currentBranch: 'main',
  });
});

describe('Chat Integration Tests', () => {
  // ===== Message Flow =====
  
  describe('Message Flow: Input → Store → Display', () => {
    const user = userEvent.setup();
    
    it('sends message and displays it in chat', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      await user.type(input, 'Hello, Mira!');
      
      const sendButton = screen.getByRole('button');
      await user.click(sendButton);
      
      // Message should appear in chat
      await waitFor(() => {
        expect(screen.getByText(/Hello, Mira!/)).toBeInTheDocument();
      });
      
      // Store should have the message
      const messages = useChatStore.getState().messages;
      expect(messages).toHaveLength(1);
      expect(messages[0].content).toBe('Hello, Mira!');
      expect(messages[0].role).toBe('user');
    });
    
    it('sends message via WebSocket', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      await user.type(input, 'Test message');
      
      const sendButton = screen.getByRole('button');
      await user.click(sendButton);
      
      // FIXED: Wait for async send to complete
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalled();
      }, { timeout: 1000 });
      
      const call = mockSend.mock.calls[0][0];
      expect(call.type).toBe('chat');
      expect(call.content).toBe('Test message'); // FIXED: content not message
      expect(call.project_id).toBe('test-project');
    });
    
    it('clears input after sending', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      await user.type(input, 'Test message');
      await user.click(screen.getByRole('button'));
      
      await waitFor(() => {
        expect(input.value).toBe('');
      });
    });
    
    it('disables input while waiting for response', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      await user.type(input, 'Test message');
      await user.click(screen.getByRole('button'));
      
      // Input should be disabled
      await waitFor(() => {
        expect(input).toBeDisabled();
      });
    });
  });
  
  // ===== Multiple Messages =====
  
  describe('Multiple Messages', () => {
    const user = userEvent.setup();
    
    it('displays multiple messages in order', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      const sendButton = screen.getByRole('button');
      
      // Send first message
      await user.type(input, 'First message');
      await user.click(sendButton);
      
      // Wait for user message to appear
      await waitFor(() => {
        expect(screen.getByText(/First message/)).toBeInTheDocument();
      });
      
      // Manually add assistant response (simulating backend)
      useChatStore.getState().addMessage({
        id: 'assistant-1',
        role: 'assistant',
        content: 'First response',
        timestamp: Date.now(),
      });
      useChatStore.getState().setWaitingForResponse(false);
      
      // Wait for assistant message to render
      await waitFor(() => {
        expect(screen.getByText(/First response/)).toBeInTheDocument();
      });
      
      // FIXED: Now check both messages are visible
      expect(screen.getByText(/First message/)).toBeInTheDocument();
      expect(screen.getByText(/First response/)).toBeInTheDocument();
      
      // Verify order in DOM
      const messages = useChatStore.getState().messages;
      expect(messages).toHaveLength(2);
      expect(messages[0].role).toBe('user');
      expect(messages[1].role).toBe('assistant');
    });
  });
  
  // ===== Streaming =====
  
  describe('Streaming', () => {
    it('shows streaming indicator during response', async () => {
      const store = useChatStore.getState();
      store.startStreaming();
      store.appendStreamContent('Thinking...');
      
      render(<ChatArea />);
      
      // FIXED: Component shows the streaming content, not "thinking..." label
      await waitFor(() => {
        expect(screen.getByText(/Thinking.../)).toBeInTheDocument();
      });
      
      // Should show animated pulse cursor
      const cursor = document.querySelector('.animate-pulse');
      expect(cursor).toBeInTheDocument();
    });
    
    it('finalizes message when streaming ends', async () => {
      const store = useChatStore.getState();
      
      store.startStreaming();
      store.appendStreamContent('Hello, ');
      store.appendStreamContent('world!');
      
      render(<ChatArea />);
      
      // FIXED: Shows the actual streaming content
      await waitFor(() => {
        expect(screen.getByText(/Hello, world!/)).toBeInTheDocument();
      });
      
      // Should show streaming cursor
      let cursor = document.querySelector('.animate-pulse');
      expect(cursor).toBeInTheDocument();
      
      // End streaming
      store.endStreaming();
      
      await waitFor(() => {
        // Streaming cursor should be gone
        cursor = document.querySelector('.animate-pulse');
        expect(cursor).not.toBeInTheDocument();
      });
    });
  });
  
  // ===== Artifacts =====
  
  describe('Artifacts', () => {
    const user = userEvent.setup();
    
    it('displays artifacts from assistant message', async () => {
      render(<ChatArea />);
      
      // Add message with artifact
      useChatStore.getState().addMessage({
        id: 'assistant-1',
        role: 'assistant',
        content: 'Here is your code',
        timestamp: Date.now(),
        artifacts: [
          {
            id: 'art1',
            path: 'src/test.ts',
            content: 'const x = 42;',
            language: 'typescript',
          },
        ],
      });
      
      await waitFor(() => {
        expect(screen.getByText('src/test.ts')).toBeInTheDocument();
        expect(screen.getByText('Apply')).toBeInTheDocument();
      });
    });
    
    it('applies artifact and updates UI', async () => {
      render(<ChatArea />);
      
      // Add message with artifact
      useChatStore.getState().addMessage({
        id: 'assistant-1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts: [
          {
            id: 'art1',
            path: 'src/test.ts',
            content: 'const x = 42;',
            language: 'typescript',
          },
        ],
      });
      
      await waitFor(() => {
        expect(screen.getByText('Apply')).toBeInTheDocument();
      });
      
      const applyButton = screen.getByText('Apply');
      await user.click(applyButton);
      
      // Should send file write command
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalledWith({
          type: 'file_system_command',
          method: 'files.write',
          params: {
            project_id: 'test-project',
            path: 'src/test.ts',
            content: 'const x = 42;',
          },
        });
      });
      
      // Button should change to "Applied"
      await waitFor(() => {
        expect(screen.getByText('Applied')).toBeInTheDocument();
      }, { timeout: 500 });
    });
  });
  
  // ===== Connection Status =====
  
  describe('Connection Status', () => {
    it('shows connection banner when disconnected', async () => {
      useWebSocketStore.setState({ connectionState: 'disconnected' });
      
      render(<ChatArea />);
      
      expect(screen.getByText(/disconnected/i)).toBeInTheDocument();
    });
    
    it('hides connection banner when connected', async () => {
      useWebSocketStore.setState({ connectionState: 'connected' });
      
      render(<ChatArea />);
      
      expect(screen.queryByText(/disconnected/i)).not.toBeInTheDocument();
    });
    
    it('shows reconnecting banner', async () => {
      useWebSocketStore.setState({ connectionState: 'reconnecting' });
      
      render(<ChatArea />);
      
      expect(screen.getByText(/reconnecting/i)).toBeInTheDocument();
    });
  });
  
  // ===== Session Management =====
  
  describe('Session Management', () => {
    const user = userEvent.setup();
    
    it('includes session ID in messages', async () => {
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      await user.type(input, 'Test');
      await user.click(screen.getByRole('button'));
      
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalled();
      }, { timeout: 1000 });
      
      const call = mockSend.mock.calls[0][0];
      // FIXED: Uses peter-eternal from config, not custom-session
      expect(call.metadata?.session_id).toBe('peter-eternal');
    });
  });
  
  // ===== Error Handling =====
  
  describe('Error Handling', () => {
    const user = userEvent.setup();
    
    it('handles missing project gracefully', async () => {
      useAppState.setState({ currentProject: null });
      
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      await user.type(input, 'Test');
      await user.click(screen.getByRole('button'));
      
      // Should still add message to store and attempt send with null project_id
      await waitFor(() => {
        const messages = useChatStore.getState().messages;
        expect(messages.length).toBeGreaterThan(0);
      });
      
      // Check that send was called with null project_id
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalled();
      }, { timeout: 1000 });
      
      const call = mockSend.mock.calls[0][0];
      expect(call.project_id).toBeNull();
    });
    
    it('handles WebSocket errors gracefully', async () => {
      mockSend.mockRejectedValue(new Error('Connection failed'));
      
      render(<ChatArea />);
      
      const input = screen.getByRole('textbox');
      await user.type(input, 'Test');
      await user.click(screen.getByRole('button'));
      
      // Should not crash - message still added to store
      await waitFor(() => {
        const messages = useChatStore.getState().messages;
        expect(messages.length).toBeGreaterThan(0);
        expect(messages[0].content).toBe('Test');
      });
      
      // Waiting state should be cleared on error
      await waitFor(() => {
        expect(useChatStore.getState().isWaitingForResponse).toBe(false);
      }, { timeout: 1000 });
    });
  });
  
  // ===== Store Persistence =====
  
  describe('Store State', () => {
    it('persists messages in store', async () => {
      render(<ChatArea />);
      
      useChatStore.getState().addMessage({
        id: '1',
        role: 'user',
        content: 'Persisted message',
        timestamp: Date.now(),
      });
      
      await waitFor(() => {
        expect(screen.getByText(/Persisted message/)).toBeInTheDocument();
      });
      
      // Message should be in store
      const messages = useChatStore.getState().messages;
      expect(messages).toHaveLength(1);
      expect(messages[0].content).toBe('Persisted message');
    });
    
    it('updates message in store', async () => {
      render(<ChatArea />);
      
      useChatStore.getState().addMessage({
        id: '1',
        role: 'assistant',
        content: 'Original',
        timestamp: Date.now(),
      });
      
      await waitFor(() => {
        expect(screen.getByText(/Original/)).toBeInTheDocument();
      });
      
      useChatStore.getState().updateMessage('1', {
        content: 'Updated',
      });
      
      await waitFor(() => {
        expect(screen.getByText(/Updated/)).toBeInTheDocument();
      });
    });
  });
});
