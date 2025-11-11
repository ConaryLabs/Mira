// tests/hooks/useChatMessaging.test.ts
// Comprehensive tests for useChatMessaging hook - message sending and context gathering

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useChatMessaging } from '../../src/hooks/useChatMessaging';
import { useWebSocketStore } from '../../src/stores/useWebSocketStore';
import { useChatStore } from '../../src/stores/useChatStore';
import { useAppState, useArtifactState } from '../../src/stores/useAppState';
import * as appConfig from '../../src/config/app';

// Mock all dependencies
vi.mock('../../src/stores/useWebSocketStore');
vi.mock('../../src/stores/useChatStore');
vi.mock('../../src/stores/useAppState');
vi.mock('../../src/config/app');

const mockSend = vi.fn();
const mockAddMessage = vi.fn();
const mockSetWaitingForResponse = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  
  // Mock getSessionId
  vi.mocked(appConfig.getSessionId).mockReturnValue('test-session-123');
  
  // Mock WebSocket store - Zustand uses selectors
  vi.mocked(useWebSocketStore).mockImplementation((selector: any) => {
    const state = { send: mockSend };
    return selector ? selector(state) : state;
  });
  
  // Mock Chat store - Zustand uses selectors
  vi.mocked(useChatStore).mockImplementation((selector: any) => {
    const state = {
      addMessage: mockAddMessage,
      setWaitingForResponse: mockSetWaitingForResponse,
    };
    return selector ? selector(state) : state;
  });
  
  // Default AppState mock - no project, no artifacts
  vi.mocked(useAppState).mockReturnValue({
    currentProject: null,
    modifiedFiles: [],
    currentBranch: 'main',
  } as any);
  
  vi.mocked(useArtifactState).mockReturnValue({
    activeArtifact: null,
  } as any);
});

describe('useChatMessaging Hook', () => {
  // ===== Basic Message Sending =====
  
  describe('handleSend - Basic Functionality', () => {
    it('adds user message to store', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Hello Mira');
      });
      
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'user',
          content: 'Hello Mira',
          id: expect.stringMatching(/^user-\d+$/),
          timestamp: expect.any(Number),
        })
      );
    });
    
    it('sets waiting state before sending', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSetWaitingForResponse).toHaveBeenCalledWith(true);
      // Check it was called before send
      const waitingCallOrder = mockSetWaitingForResponse.mock.invocationCallOrder[0];
      const sendCallOrder = mockSend.mock.invocationCallOrder[0];
      expect(waitingCallOrder).toBeLessThan(sendCallOrder);
    });
    
    it('sends message via WebSocket with correct structure', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test message');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'chat',
          content: 'Test message',
          project_id: null,
          metadata: expect.objectContaining({
            session_id: 'test-session-123',
            timestamp: expect.any(Number),
          }),
        })
      );
    });
  });
  
  // ===== Project Context =====
  
  describe('Project Context Building', () => {
    it('includes project ID when project is selected', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'project-123', name: 'Test Project' },
        modifiedFiles: [],
        currentBranch: 'main',
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          project_id: 'project-123',
        })
      );
    });
    
    it('includes repository status in metadata', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'proj-1', has_repository: true },
        modifiedFiles: [],
        currentBranch: 'feature/test',
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            has_repository: true,
            current_branch: 'feature/test',
          }),
        })
      );
    });
    
    it('includes modified files count', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'proj-1' },
        modifiedFiles: ['file1.ts', 'file2.ts', 'file3.ts'],
        currentBranch: 'main',
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            modified_files_count: 3,
          }),
        })
      );
    });
    
    it('defaults to main branch when no branch specified', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'proj-1' },
        modifiedFiles: [],
        currentBranch: null,
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            current_branch: 'main',
          }),
        })
      );
    });
  });
  
  // ===== File/Artifact Context =====
  
  describe('File Context Building', () => {
    it('includes active artifact file information', async () => {
      vi.mocked(useArtifactState).mockReturnValue({
        activeArtifact: {
          id: 'art-1',
          path: 'src/components/Button.tsx',
          content: 'export const Button = () => <button>Click</button>;',
          language: 'typescript',
        },
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Fix this component');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            file_path: 'src/components/Button.tsx',
            file_content: 'export const Button = () => <button>Click</button>;',
            language: 'typescript',
          }),
        })
      );
    });
    
    it('detects language from file extension', async () => {
      const testCases = [
        { path: 'main.rs', expectedLanguage: 'rust' },
        { path: 'app.py', expectedLanguage: 'python' },
        { path: 'server.go', expectedLanguage: 'go' },
        { path: 'config.json', expectedLanguage: 'json' },
        { path: 'styles.css', expectedLanguage: 'css' },
        { path: 'README.md', expectedLanguage: 'markdown' },
        { path: 'script.sh', expectedLanguage: 'shell' },
        { path: 'config.toml', expectedLanguage: 'toml' },
        { path: 'config.yaml', expectedLanguage: 'yaml' },
      ];
      
      for (const { path, expectedLanguage } of testCases) {
        vi.clearAllMocks();
        
        vi.mocked(useArtifactState).mockReturnValue({
          activeArtifact: {
            id: 'art-1',
            path,
            content: 'test content',
            language: 'typescript', // Hook should override based on path
          },
        } as any);
        
        const { result } = renderHook(() => useChatMessaging());
        
        await act(async () => {
          await result.current.handleSend('Test');
        });
        
        expect(mockSend).toHaveBeenCalledWith(
          expect.objectContaining({
            metadata: expect.objectContaining({
              language: expectedLanguage,
            }),
          })
        );
      }
    });
    
    it('defaults to plaintext for unknown extensions', async () => {
      vi.mocked(useArtifactState).mockReturnValue({
        activeArtifact: {
          id: 'art-1',
          path: 'unknown.xyz',
          content: 'test',
          language: 'typescript',
        },
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            language: 'plaintext',
          }),
        })
      );
    });
    
    it('sets null file context when no artifact active', async () => {
      vi.mocked(useArtifactState).mockReturnValue({
        activeArtifact: null,
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          metadata: expect.objectContaining({
            file_path: null,
            file_content: null,
            language: null,
          }),
        })
      );
    });
  });
  
  // ===== Error Handling =====
  
  describe('Error Handling', () => {
    it('clears waiting state when send fails', async () => {
      mockSend.mockRejectedValueOnce(new Error('Connection failed'));
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test message');
      });
      
      // Should set waiting to true initially, then clear it on error
      expect(mockSetWaitingForResponse).toHaveBeenCalledWith(true);
      await waitFor(() => {
        expect(mockSetWaitingForResponse).toHaveBeenCalledWith(false);
      });
    });
    
    it('logs error when send fails', async () => {
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      mockSend.mockRejectedValueOnce(new Error('Network error'));
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test');
      });
      
      expect(consoleErrorSpy).toHaveBeenCalledWith(
        '[useChatMessaging] Send failed:',
        expect.any(Error)
      );
      
      consoleErrorSpy.mockRestore();
    });
    
    it('still adds user message even if send fails', async () => {
      mockSend.mockRejectedValueOnce(new Error('Send failed'));
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Test message');
      });
      
      // User message should be added before send attempt
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'user',
          content: 'Test message',
        })
      );
    });
  });
  
  // ===== System Message Helpers =====
  
  describe('System Message Helpers', () => {
    it('addSystemMessage creates system message', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      act(() => {
        result.current.addSystemMessage('Project initialized');
      });
      
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'system',
          content: 'Project initialized',
          id: expect.stringMatching(/^sys-\d+$/),
          timestamp: expect.any(Number),
        })
      );
    });
    
    it('addProjectContextMessage formats project context', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      act(() => {
        result.current.addProjectContextMessage('mira-backend');
      });
      
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'system',
          content: 'Now working in project: mira-backend',
        })
      );
    });
    
    it('addFileContextMessage formats file context', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      act(() => {
        result.current.addFileContextMessage('src/main.rs');
      });
      
      expect(mockAddMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          role: 'system',
          content: 'Now viewing: src/main.rs',
        })
      );
    });
  });
  
  // ===== Edge Cases & Integration =====
  
  describe('Edge Cases', () => {
    it('handles empty string message', async () => {
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('');
      });
      
      // Should still process empty messages (validation happens in ChatInput)
      expect(mockAddMessage).toHaveBeenCalled();
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: '',
        })
      );
    });
    
    it('handles very long messages', async () => {
      const longMessage = 'A'.repeat(100000);
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend(longMessage);
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: longMessage,
        })
      );
    });
    
    it('handles special characters in message content', async () => {
      const specialMessage = 'Test\n\nWith "quotes" and \'apostrophes\' and <tags>';
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend(specialMessage);
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: specialMessage,
        })
      );
    });
    
    it('handles unicode and emoji in messages', async () => {
      const unicodeMessage = '测试 🚀 Привет مرحبا';
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend(unicodeMessage);
      });
      
      expect(mockSend).toHaveBeenCalledWith(
        expect.objectContaining({
          content: unicodeMessage,
        })
      );
    });
  });
  
  // ===== Full Context Integration =====
  
  describe('Full Context Integration', () => {
    it('builds complete context with project, file, and metadata', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { 
          id: 'proj-123', 
          name: 'mira-backend',
          has_repository: true 
        },
        modifiedFiles: ['src/main.rs', 'Cargo.toml'],
        currentBranch: 'feature/new-endpoint',
      } as any);
      
      vi.mocked(useArtifactState).mockReturnValue({
        activeArtifact: {
          id: 'art-1',
          path: 'src/api/endpoints.rs',
          content: 'pub async fn handler() {}',
          language: 'rust',
        },
      } as any);
      
      const { result } = renderHook(() => useChatMessaging());
      
      await act(async () => {
        await result.current.handleSend('Add error handling to this endpoint');
      });
      
      expect(mockSend).toHaveBeenCalledWith({
        type: 'chat',
        content: 'Add error handling to this endpoint',
        project_id: 'proj-123',
        metadata: {
          session_id: 'test-session-123',
          timestamp: expect.any(Number),
          file_path: 'src/api/endpoints.rs',
          file_content: 'pub async fn handler() {}',
          language: 'rust',
          has_repository: true,
          current_branch: 'feature/new-endpoint',
          modified_files_count: 2,
        },
      });
    });
  });
});
