// src/hooks/useMessageHandler.ts
// UPDATED: Handle stream, status, chat_complete message types properly

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore, Artifact } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';

export const useMessageHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const { addMessage, startStreaming, appendStreamContent, endStreaming } = useChatStore();

  useEffect(() => {
    const unsubscribe = subscribe(
      'chat-handler',
      (message) => {
        handleMessage(message);
      },
      ['response', 'stream', 'status', 'chat_complete']  // Subscribe to all message types
    );
    return unsubscribe;
  }, [subscribe, addMessage, startStreaming, appendStreamContent, endStreaming]);

  function handleMessage(message: any) {
    console.log('[Handler] Message received:', message.type);
    
    switch (message.type) {
      case 'status':
        handleStatus(message);
        break;
      case 'stream':
        handleStream(message);
        break;
      case 'chat_complete':
        handleChatComplete(message);
        break;
      case 'response':
        handleChatResponse(message);
        break;
      default:
        console.log('[Handler] Unhandled message type:', message.type);
    }
  }

  function handleStatus(message: any) {
    if (message.status === 'thinking') {
      console.log('[Handler] Starting stream (thinking status)');
      startStreaming();
    }
  }

  function handleStream(message: any) {
    if (message.delta) {
      appendStreamContent(message.delta);
    }
  }

  function handleChatComplete(message: any) {
    console.log('[Handler] Chat complete received');
    
    // End streaming
    endStreaming();
    
    // Add the complete message
    const content = message.content || '';
    const artifacts = message.artifacts || [];
    
    const assistantMessage = {
      id: `assistant-${Date.now()}`,
      role: 'assistant' as const,
      content,
      timestamp: Date.now(),
      thinking: message.thinking,
      artifacts
    };
    
    console.log('[Handler] Adding message with content length:', content.length);
    addMessage(assistantMessage);
    
    if (artifacts && artifacts.length > 0) {
      console.log('[Handler] Processing artifacts:', artifacts.length);
      processArtifacts(artifacts);
    }
  }

  function handleChatResponse(message: any) {
    console.log('[Handler] Chat response received (legacy):', message);
    
    // Handle legacy streaming format
    if (message.streaming) {
      if (message.content) appendStreamContent(message.content);
      return;
    }
    
    if (message.complete) {
      endStreaming();
      return;
    }
    
    // Handle complete response
    const content = message.data?.content || message.content || message.message || '';
    const artifacts = message.data?.artifacts || message.artifacts || [];
    
    const assistantMessage = {
      id: `assistant-${Date.now()}`,
      role: 'assistant' as const,
      content,
      timestamp: Date.now(),
      thinking: message.thinking,
      artifacts
    };
    
    console.log('[Handler] Adding message with content length:', content.length);
    addMessage(assistantMessage);
    
    if (artifacts && artifacts.length > 0) {
      console.log('[Handler] Processing artifacts:', artifacts.length);
      processArtifacts(artifacts);
    }
  }
  
  function processArtifacts(artifacts: any[]) {
    const { addArtifact } = useAppState.getState();
    
    artifacts.forEach((artifact: any) => {
      if (!artifact || !artifact.content) {
        console.warn('[Handler] Skipping invalid artifact:', artifact);
        return;
      }
      
      const path = artifact.path || artifact.title || 'untitled';
      const language = artifact.language || inferLanguage(path);
      
      const cleanArtifact: Artifact = {
        id: artifact.id || `artifact-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
        path,
        content: artifact.content,
        language,
        changeType: artifact.change_type,
        status: 'draft',
        origin: 'llm',
        timestamp: Date.now()
      };
      
      console.log('[Handler] Adding artifact:', cleanArtifact.path);
      addArtifact(cleanArtifact);
    });
  }
  
  function inferLanguage(path: string): string {
    const ext = path.split('.').pop()?.toLowerCase();
    switch (ext) {
      case 'rs': return 'rust';
      case 'js': case 'jsx': return 'javascript';
      case 'ts': case 'tsx': return 'typescript';
      case 'py': return 'python';
      case 'json': return 'json';
      case 'html': return 'html';
      case 'css': return 'css';
      case 'md': return 'markdown';
      case 'toml': return 'toml';
      case 'yaml': case 'yml': return 'yaml';
      case 'sh': return 'shell';
      default: return 'plaintext';
    }
  }
};
