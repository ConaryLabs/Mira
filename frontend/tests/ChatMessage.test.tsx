// tests/ChatMessage.test.tsx
// Component tests for ChatMessage - rendering, streaming, artifacts

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatMessage } from '../src/components/ChatMessage';
import { useWebSocketStore } from '../src/stores/useWebSocketStore';
import { useAppState } from '../src/stores/useAppState'; // FIXED: Correct path
import type { ChatMessage as ChatMessageType, Artifact } from '../src/stores/useChatStore';

// Mock stores
vi.mock('../src/stores/useWebSocketStore');
vi.mock('../src/stores/useAppState');

const mockSend = vi.fn();
const mockSetShowArtifacts = vi.fn();
const mockAddArtifact = vi.fn();
const mockSetActiveArtifact = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  
  vi.mocked(useWebSocketStore).mockReturnValue({
    send: mockSend,
  } as any);
  
  vi.mocked(useAppState).mockReturnValue({
    currentProject: { id: 'test-project' },
    setShowArtifacts: mockSetShowArtifacts,
    addArtifact: mockAddArtifact,
    setActiveArtifact: mockSetActiveArtifact,
  } as any);
});

describe('ChatMessage Component', () => {
  // ===== Basic Rendering =====
  
  describe('Message Rendering', () => {
    it('renders user message with correct styling', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'user',
        content: 'Hello, Mira!',
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.getByText('Hello, Mira!')).toBeInTheDocument();
      // User messages should have blue background
      const messageDiv = screen.getByText('Hello, Mira!').closest('.bg-blue-600');
      expect(messageDiv).toBeInTheDocument();
    });
    
    it('renders assistant message with correct styling', () => {
      const message: ChatMessageType = {
        id: '2',
        role: 'assistant',
        content: "What's up?",
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.getByText("What's up?")).toBeInTheDocument();
      // Assistant messages should have gray background
      const messageDiv = screen.getByText("What's up?").closest('.bg-gray-800');
      expect(messageDiv).toBeInTheDocument();
    });
    
    it('shows avatar for assistant messages', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Response',
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      // Look for bot icon (SVG element)
      const botIcon = document.querySelector('.lucide-bot');
      expect(botIcon).toBeInTheDocument();
    });
    
    it('displays timestamp', () => {
      const timestamp = Date.now();
      const message: ChatMessageType = {
        id: '1',
        role: 'user',
        content: 'Test',
        timestamp,
      };
      
      render(<ChatMessage message={message} />);
      
      // FIXED: Component shows seconds, so use regex to match
      const timeString = new Date(timestamp).toLocaleTimeString('en-US', {
        hour: 'numeric',
        minute: '2-digit',
        second: '2-digit', // ADDED: Seconds are shown
        hour12: true,
      });
      
      expect(screen.getByText(timeString)).toBeInTheDocument();
    });
  });
  
  // ===== Streaming Indicator =====
  
  describe('Streaming State', () => {
    it('shows streaming indicator when message is streaming', () => {
      const message: ChatMessageType & { isStreaming: boolean } = {
        id: '1',
        role: 'assistant',
        content: 'Partial response...',
        timestamp: Date.now(),
        isStreaming: true,
      };
      
      render(<ChatMessage message={message} />);
      
      // FIXED: Component shows animated pulse cursor, not "thinking..." text
      const cursor = document.querySelector('.animate-pulse');
      expect(cursor).toBeInTheDocument();
    });
    
    it('hides streaming indicator when not streaming', () => {
      const message: ChatMessageType & { isStreaming: boolean } = {
        id: '1',
        role: 'assistant',
        content: 'Complete response',
        timestamp: Date.now(),
        isStreaming: false,
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.queryByText('thinking...')).not.toBeInTheDocument();
    });
  });
  
  // ===== Markdown Rendering =====
  
  describe('Content Formatting', () => {
    it('renders markdown content', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: '# Header\n\nThis is **bold** text.',
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      // Markdown should be rendered
      expect(screen.getByText('Header')).toBeInTheDocument();
      expect(screen.getByText('bold')).toBeInTheDocument();
    });
    
    it('renders code blocks with syntax highlighting', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: '```typescript\nconst x = 42;\n```',
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      // Check for syntax highlighted code keywords
      expect(screen.getByText(/const/)).toBeInTheDocument();
      
      // Verify code block with language class exists (syntax highlighting is active)
      const codeBlock = document.querySelector('code.language-typescript');
      expect(codeBlock).toBeInTheDocument();
      expect(codeBlock?.textContent).toContain('42');
    });
  });
  
  // ===== Artifacts Display =====
  
  describe('Artifacts', () => {
    const createArtifact = (id: string, path: string): Artifact => ({
      id,
      path,
      content: 'test content',
      language: 'typescript',
    });
    
    it('displays artifacts when present', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed your code',
        timestamp: Date.now(),
        artifacts: [
          createArtifact('art1', 'src/test.ts'),
        ],
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.getByText('src/test.ts')).toBeInTheDocument();
      expect(screen.getByText('Apply')).toBeInTheDocument();
    });
    
    it('shows "Apply All" button for multiple artifacts', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed your code',
        timestamp: Date.now(),
        artifacts: [
          createArtifact('art1', 'src/test1.ts'),
          createArtifact('art2', 'src/test2.ts'),
        ],
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.getByText(/Apply All/)).toBeInTheDocument();
    });
    
    it('does not show "Apply All" button for single artifact', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed your code',
        timestamp: Date.now(),
        artifacts: [createArtifact('art1', 'src/test.ts')],
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.queryByText(/Apply All/)).not.toBeInTheDocument();
    });
  });
  
  // ===== Artifact Interactions =====
  
  describe('Artifact Actions', () => {
    const user = userEvent.setup();
    
    it('sends file_system_command when Apply is clicked', async () => {
      const artifact = {
        id: 'art1',
        path: 'src/test.ts',
        content: 'const x = 42;',
        language: 'typescript',
      };
      
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts: [artifact],
      };
      
      render(<ChatMessage message={message} />);
      
      const applyButton = screen.getByText('Apply');
      await user.click(applyButton);
      
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
    });
    
    it('changes button to "Applied" after successful apply', async () => {
      const artifact = {
        id: 'art1',
        path: 'src/test.ts',
        content: 'const x = 42;',
        language: 'typescript',
      };
      
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts: [artifact],
      };
      
      render(<ChatMessage message={message} />);
      
      const applyButton = screen.getByText('Apply');
      await user.click(applyButton);
      
      await waitFor(() => {
        expect(screen.getByText('Applied')).toBeInTheDocument();
      }, { timeout: 500 });
    });
    
    it('opens artifact panel when View is clicked', async () => {
      const artifact = {
        id: 'art1',
        path: 'src/test.ts',
        content: 'const x = 42;',
        language: 'typescript',
      };
      
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts: [artifact],
      };
      
      render(<ChatMessage message={message} />);
      
      const viewButton = screen.getByText('View');
      await user.click(viewButton);
      
      expect(mockAddArtifact).toHaveBeenCalledWith(artifact);
      expect(mockSetActiveArtifact).toHaveBeenCalledWith('art1');
      expect(mockSetShowArtifacts).toHaveBeenCalledWith(true);
    });
    
    it('applies all artifacts when Apply All is clicked', async () => {
      const artifacts = [
        { id: 'art1', path: 'src/test.ts', content: 'test1', language: 'typescript', changeType: 'primary' as const },
        { id: 'art2', path: 'src/utils.ts', content: 'test2', language: 'typescript', changeType: 'import' as const },
      ];
      
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts,
      };
      
      render(<ChatMessage message={message} />);
      
      const applyAllButton = screen.getByText(/Apply All/);
      await user.click(applyAllButton);
      
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalledWith({
          type: 'file_system_command',
          method: 'write_files',
          params: {
            project_id: 'test-project',
            files: [
              { path: 'src/test.ts', content: 'test1' },
              { path: 'src/utils.ts', content: 'test2' },
            ],
          },
        });
      });
    });
    
    it('does not send command when no project is selected', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: null, // No project
        setShowArtifacts: mockSetShowArtifacts,
        addArtifact: mockAddArtifact,
        setActiveArtifact: mockSetActiveArtifact,
      } as any);
      
      const artifact = {
        id: 'art1',
        path: 'src/test.ts',
        content: 'const x = 42;',
        language: 'typescript',
      };
      
      const message: ChatMessageType = {
        id: '1',
        role: 'assistant',
        content: 'Fixed',
        timestamp: Date.now(),
        artifacts: [artifact],
      };
      
      render(<ChatMessage message={message} />);
      
      const applyButton = screen.getByText('Apply');
      await user.click(applyButton);
      
      // Should not send any command
      expect(mockSend).not.toHaveBeenCalled();
    });
  });
  
  // ===== System Messages =====
  
  describe('System Messages', () => {
    it('renders system messages with distinct styling', () => {
      const message: ChatMessageType = {
        id: '1',
        role: 'system',
        content: 'Project initialized',
        timestamp: Date.now(),
      };
      
      render(<ChatMessage message={message} />);
      
      expect(screen.getByText('Project initialized')).toBeInTheDocument();
      // System messages should have distinct styling (italics or different color)
      const messageElement = screen.getByText('Project initialized');
      expect(messageElement.closest('.italic')).toBeInTheDocument();
    });
  });
});
