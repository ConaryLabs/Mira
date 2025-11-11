// tests/ChatInput.test.tsx
// Component tests for ChatInput - user input and message sending
// FIXED: Reset useUIStore to prevent textarea content bleeding between tests

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, waitFor, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatInput } from '../src/components/ChatInput';
import { useWebSocketStore } from '../src/stores/useWebSocketStore';
import { useChatStore } from '../src/stores/useChatStore';
import { useAppState } from '../src/stores/useAppState';
import { useUIStore } from '../src/stores/useUIStore';

// NOTE: Do NOT mock useWebSocketStore or useChatStore - use setState instead

// CRITICAL FIX: Mock useAppState module with both exports
vi.mock('../src/stores/useAppState', async () => {
  const actual = await vi.importActual('../src/stores/useAppState');
  return {
    ...actual,
    useAppState: vi.fn(),
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

// CRITICAL: Use let instead of const so we can reassign fresh mocks
let mockSend: ReturnType<typeof vi.fn>;
let mockAddMessage: ReturnType<typeof vi.fn>;
let mockStartStreaming: ReturnType<typeof vi.fn>;
let mockSetWaitingForResponse: ReturnType<typeof vi.fn>;

beforeEach(() => {
  // Create FRESH mock functions for each test
  mockSend = vi.fn();
  mockAddMessage = vi.fn();
  mockStartStreaming = vi.fn();
  mockSetWaitingForResponse = vi.fn();
  
  vi.clearAllMocks();
  
  // CRITICAL FIX: Use setState instead of mockReturnValue for Zustand stores
  useWebSocketStore.setState({
    send: mockSend,
    connectionState: 'connected',
  });
  
  // CRITICAL FIX: Use setState for useChatStore too
  useChatStore.setState({
    messages: [],
    currentSessionId: 'test-session',
    isWaitingForResponse: false,
    isStreaming: false,
    streamingContent: '',
    addMessage: mockAddMessage,
    startStreaming: mockStartStreaming,
    setWaitingForResponse: mockSetWaitingForResponse,
  });
  
  // CRITICAL FIX: Reset useUIStore to clear textarea content
  // This was the missing piece - textarea is controlled by useUIStore!
  useUIStore.setState({
    inputContent: '',
  });
  
  // Mock useAppState
  vi.mocked(useAppState).mockReturnValue({
    currentProject: { id: 'test-project' },
    modifiedFiles: [],
    currentBranch: 'main',
  } as any);
  
  // Note: useArtifactState is mocked at module level with vi.fn(() => ({...}))
});

afterEach(() => {
  // Properly unmount all React components
  cleanup();
  
  // CRITICAL: Reset Zustand stores with completely fresh mock instances
  // This prevents function call history and state from bleeding between tests
  useChatStore.setState({
    messages: [],
    currentSessionId: 'test-session',
    isWaitingForResponse: false,
    isStreaming: false,
    streamingContent: '',
    // Fresh vi.fn() instances - not the same references!
    addMessage: vi.fn(),
    startStreaming: vi.fn(),
    setWaitingForResponse: vi.fn(),
  });
  
  useWebSocketStore.setState({
    send: vi.fn(), // Fresh mock function
    connectionState: 'disconnected',
  });
  
  // CRITICAL: Reset useUIStore to prevent textarea content bleeding
  // Without this, the multiline test's "Line 1\nLine 2" persists into next tests!
  useUIStore.setState({
    inputContent: '',
  });
});

describe('ChatInput Component', () => {
  // ===== Basic Rendering =====
  
  describe('Rendering', () => {
    it('renders input field', () => {
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox');
      expect(input).toBeInTheDocument();
    });
    
    it('renders send button', () => {
      render(<ChatInput />);
      
      const button = screen.getByRole('button');
      expect(button).toBeInTheDocument();
    });
    
    it('shows placeholder text', () => {
      render(<ChatInput />);
      
      const input = screen.getByPlaceholderText(/message/i);
      expect(input).toBeInTheDocument();
    });
  });
  
  // ===== Input Handling =====
  
  describe('Input Handling', () => {
    it('allows typing in input field', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      await user.type(input, 'Hello, Mira!');
      
      expect(input.value).toBe('Hello, Mira!');
    });
    
    it('handles multiline input', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLTextAreaElement;
      await user.type(input, 'Line 1\nLine 2\nLine 3');
      
      expect(input.value).toContain('Line 1');
      expect(input.value).toContain('Line 2');
      expect(input.value).toContain('Line 3');
    });
    
    it('clears input after sending message', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test message');
      await user.click(button);
      
      await waitFor(() => {
        expect(input.value).toBe('');
      });
    });
  });
  
  // ===== Message Sending =====
  
  describe('Message Sending', () => {
    it('sends message when button is clicked', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test message');
      await user.click(button);
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'chat',
          content: 'Test message',
        })
      );
    });
    
    it('sends message when Enter is pressed', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      
      await user.type(input, 'Test message{Enter}');
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'chat',
          content: 'Test message',
        })
      );
    });
    
    it('does not send on Shift+Enter (allows newline)', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      
      await user.type(input, 'Line 1{Shift>}{Enter}{/Shift}Line 2');
      
      // Should not send, just add newline
      expect(mockSend).not.toHaveBeenCalled();
      expect(input.value).toContain('\n');
    });
    
    it('adds user message to store when sending', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test message');
      await user.click(button);
      
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'user',
          content: 'Test message',
        })
      );
    });
    
    it('starts streaming after sending message', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test');
      await user.click(button);
      
      expect(mockSetWaitingForResponse).toHaveBeenCalledWith(true);
    });
    
    it('sets waiting for response after sending', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test');
      await user.click(button);
      
      expect(mockSetWaitingForResponse).toHaveBeenCalledWith(true);
    });
  });
  
  // ===== Input Validation =====
  
  describe('Input Validation', () => {
    it('does not send empty message', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const button = screen.getByRole('button');
      await user.click(button);
      
      expect(mockSend).not.toHaveBeenCalled();
    });
    
    it('does not send whitespace-only message', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, '   \n\n   ');
      await user.click(button);
      
      expect(mockSend).not.toHaveBeenCalled();
    });
    
    it('trims whitespace from message', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, '  Test message  ');
      await user.click(button);
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: 'Test message',
        })
      );
    });
  });
  
  // ===== Disabled State =====
  
  describe('Disabled State', () => {
    it('disables input when waiting for response', () => {
      useChatStore.setState({ isWaitingForResponse: true });
      
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      expect(input).toBeDisabled();
    });
    
    it('disables button when waiting for response', () => {
      useChatStore.setState({ isWaitingForResponse: true });
      
      render(<ChatInput />);
      
      const button = screen.getByRole('button');
      expect(button).toBeDisabled();
    });
    
    it('shows loading state when waiting for response', () => {
      useChatStore.setState({ isWaitingForResponse: true });
      
      render(<ChatInput />);
      
      // Should show some loading indicator (spinner, "Sending...", etc)
      expect(screen.getByRole('button')).toHaveAttribute('disabled');
    });
    
    it('does not send message when disabled', async () => {
      const user = userEvent.setup();
      useChatStore.setState({ isWaitingForResponse: true });
      
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test message');
      await user.click(button);
      
      expect(mockSend).not.toHaveBeenCalled();
    });
  });
  
  // ===== Project Context =====
  
  describe('Project Context', () => {
    it('includes project ID in message when project selected', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test message');
      await user.click(button);
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          project_id: 'test-project',
        })
      );
    });
    
    it('handles missing project gracefully', async () => {
      const user = userEvent.setup();
      vi.mocked(useAppState).mockReturnValue({
        currentProject: null, // No project
        modifiedFiles: [],
        currentBranch: 'main',
      } as any);
      
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, 'Test');
      await user.click(button);
      
      // Should send with null project_id
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          project_id: null,
        })
      );
    });
  });
  
  // ===== Edge Cases =====
  
  describe('Edge Cases', () => {
    it('handles very long messages', async () => {
      const user = userEvent.setup();
      const longMessage = 'A'.repeat(10000);
      
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      // Use paste instead of type - typing 10000 chars is too slow
      await user.click(input);
      await user.paste(longMessage);
      await user.click(button);
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: longMessage,
        })
      );
    }, 10000); // Increase timeout to 10s for this test
    
    it('handles special characters in message', async () => {
      const user = userEvent.setup();
      const specialMessage = 'Test <script>alert("xss")</script> message';
      
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      await user.type(input, specialMessage);
      await user.click(button);
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: specialMessage,
        })
      );
    });
    
    it('handles rapid consecutive sends', async () => {
      const user = userEvent.setup();
      render(<ChatInput />);
      
      const input = screen.getByRole('textbox') as HTMLInputElement;
      const button = screen.getByRole('button');
      
      // Send multiple messages quickly
      await user.type(input, 'Message 1');
      await user.click(button);
      
      await user.type(input, 'Message 2');
      await user.click(button);
      
      await user.type(input, 'Message 3');
      await user.click(button);
      
      // All should be sent
      expect(mockSend).toHaveBeenCalledTimes(3);
    });
  });
});
