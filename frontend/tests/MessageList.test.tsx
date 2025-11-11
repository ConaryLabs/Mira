// tests/MessageList.test.tsx
// Component tests for MessageList - FIXED with Virtuoso mock

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MessageList } from '../src/components/MessageList';
import { useChatStore } from '../src/stores/useChatStore';
import type { ChatMessage } from '../src/stores/useChatStore';

vi.mock('../src/stores/useChatStore');

// CRITICAL FIX: Mock ChatMessage component
vi.mock('../src/components/ChatMessage', () => ({
  ChatMessage: ({ message }: { message: ChatMessage & { isStreaming?: boolean } }) => (
    <div data-testid={`message-${message.id}`}>
      {message.role}: {message.content}
      {message.isStreaming && <span data-testid="streaming">streaming</span>}
    </div>
  ),
}));

// CRITICAL FIX: Mock Virtuoso to render all items directly (no virtualization in tests)
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

beforeEach(() => {
  vi.clearAllMocks();
});

describe('MessageList Component', () => {
  // ===== Basic Rendering =====
  
  describe('Rendering', () => {
    it('renders empty state when no messages', () => {
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: [],
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      const messages = screen.queryAllByTestId(/message-/);
      expect(messages).toHaveLength(0);
    });
    
    it('renders single message', () => {
      const messages: ChatMessage[] = [
        {
          id: '1',
          role: 'user',
          content: 'Hello!',
          timestamp: Date.now(),
        },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
      expect(screen.getByText(/Hello!/)).toBeInTheDocument();
    });
    
    it('renders multiple messages in order', () => {
      const messages: ChatMessage[] = [
        { id: '1', role: 'user', content: 'First', timestamp: 1000 },
        { id: '2', role: 'assistant', content: 'Second', timestamp: 2000 },
        { id: '3', role: 'user', content: 'Third', timestamp: 3000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
      expect(screen.getByTestId('message-2')).toBeInTheDocument();
      expect(screen.getByTestId('message-3')).toBeInTheDocument();
    });
    
    it('renders messages in chronological order', () => {
      const messages: ChatMessage[] = [
        { id: '1', role: 'user', content: 'First', timestamp: 1000 },
        { id: '2', role: 'assistant', content: 'Second', timestamp: 2000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      const { container } = render(<MessageList />);
      const messageElements = container.querySelectorAll('[data-testid^="message-"]');
      
      expect(messageElements[0]).toHaveAttribute('data-testid', 'message-1');
      expect(messageElements[1]).toHaveAttribute('data-testid', 'message-2');
    });
  });
  
  // ===== Streaming Messages =====
  
  describe('Streaming', () => {
    it('shows streaming message when isStreaming is true', () => {
      const messages: ChatMessage[] = [
        { id: '1', role: 'user', content: 'Hello', timestamp: 1000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: true,
          streamingContent: 'Thinking...',
          streamingMessageId: 'stream-1',
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Should show streaming content
      expect(screen.getByText(/Thinking.../)).toBeInTheDocument();
    });
    
    it('marks streaming message with isStreaming flag', () => {
      const messages: ChatMessage[] = [];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: true,
          streamingContent: 'Streaming...',
          streamingMessageId: 'stream-1',
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Streaming message should be marked
      expect(screen.getByTestId('streaming')).toBeInTheDocument();
    });
    
    it('hides streaming message when isStreaming becomes false', () => {
      const messages: ChatMessage[] = [];
      
      // First render: streaming
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: true,
          streamingContent: 'Streaming...',
          streamingMessageId: 'stream-1',
          isWaitingForResponse: false,
        })
      );
      
      const { rerender } = render(<MessageList />);
      expect(screen.getByText(/Streaming.../)).toBeInTheDocument();
      
      // Second render: not streaming
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      rerender(<MessageList />);
      expect(screen.queryByText(/Streaming.../)).not.toBeInTheDocument();
    });
  });
  
  // ===== Message Updates =====
  
  describe('Message Updates', () => {
    it('updates when new message is added', () => {
      const messages1: ChatMessage[] = [
        { id: '1', role: 'user', content: 'Hello', timestamp: 1000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: messages1,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      const { rerender } = render(<MessageList />);
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
      
      // Add new message
      const messages2: ChatMessage[] = [
        ...messages1,
        { id: '2', role: 'assistant', content: 'Hi there!', timestamp: 2000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: messages2,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      rerender(<MessageList />);
      expect(screen.getByTestId('message-2')).toBeInTheDocument();
    });
    
    it('updates when message content changes', () => {
      const messages1: ChatMessage[] = [
        { id: '1', role: 'user', content: 'Original', timestamp: 1000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: messages1,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      const { rerender } = render(<MessageList />);
      expect(screen.getByText(/Original/)).toBeInTheDocument();
      
      // Update content
      const messages2: ChatMessage[] = [
        { id: '1', role: 'user', content: 'Updated', timestamp: 1000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: messages2,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      rerender(<MessageList />);
      expect(screen.getByText(/Updated/)).toBeInTheDocument();
    });
  });
  
  // ===== Edge Cases =====
  
  describe('Edge Cases', () => {
    it('handles empty streaming content', () => {
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages: [],
          isStreaming: true,
          streamingContent: '',
          streamingMessageId: 'stream-1',
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Should not crash with empty streaming content
      const messages = screen.queryAllByTestId(/message-/);
      expect(messages.length).toBeGreaterThanOrEqual(0);
    });
    
    it('handles very long message list', () => {
      const messages: ChatMessage[] = Array.from({ length: 100 }, (_, i) => ({
        id: `${i}`,
        role: i % 2 === 0 ? 'user' as const : 'assistant' as const,
        content: `Message ${i}`,
        timestamp: i * 1000,
      }));
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Should render all messages (mock Virtuoso renders everything)
      expect(screen.getAllByTestId(/message-/)).toHaveLength(100);
    });
    
    it('handles messages with artifacts', () => {
      const messages: ChatMessage[] = [
        {
          id: '1',
          role: 'assistant',
          content: 'Here are your files',
          timestamp: 1000,
          artifacts: [
            {
              id: 'art1',
              path: 'test.ts',
              content: 'code',
              language: 'typescript',
            },
          ],
        },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Should pass artifacts to ChatMessage component
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
    });
    
    it('handles messages with metadata', () => {
      const messages: ChatMessage[] = [
        {
          id: '1',
          role: 'user',
          content: 'Test',
          timestamp: 1000,
          metadata: {
            session_id: 'test-session',
            project_id: 'test-project',
          },
        },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
    });
  });
  
  // ===== Auto-scroll behavior =====
  
  describe('Auto-scroll', () => {
    it('should handle scroll behavior', () => {
      // Note: Actual scroll testing would require more complex setup
      // This test just verifies the component renders without scroll errors
      const scrollIntoViewMock = vi.fn();
      Element.prototype.scrollIntoView = scrollIntoViewMock;
      
      const messages: ChatMessage[] = [
        { id: '1', role: 'user', content: 'First', timestamp: 1000 },
      ];
      
      vi.mocked(useChatStore).mockImplementation((selector: any) => 
        selector({
          messages,
          isStreaming: false,
          streamingContent: '',
          streamingMessageId: null,
          isWaitingForResponse: false,
        })
      );
      
      render(<MessageList />);
      
      // Component should render without errors
      expect(screen.getByTestId('message-1')).toBeInTheDocument();
    });
  });
});
