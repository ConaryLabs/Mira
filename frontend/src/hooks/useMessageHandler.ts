// src/hooks/useMessageHandler.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';
import { extractArtifacts } from '../utils/artifact';

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
  
  function processArtifacts(rawArtifacts: any[]) {
    const { addArtifact } = useAppState.getState();

    const artifacts = extractArtifacts({ artifacts: rawArtifacts });
    artifacts.forEach((artifact) => {
      console.log('[Handler] Adding artifact:', artifact.path);
      addArtifact(artifact);
    });
  }
};
