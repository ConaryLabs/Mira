// frontend/src/hooks/useMessageHandler.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';
import { extractArtifacts } from '../utils/artifact';

export const useMessageHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const { addMessage, startStreaming, appendStreamContent, endStreaming, addToolExecution } = useChatStore();
  const addToast = useAppState(state => state.addToast);

  useEffect(() => {
    const unsubscribe = subscribe(
      'chat-handler',
      (message) => {
        handleMessage(message);
      },
      ['response', 'stream', 'status', 'chat_complete', 'operation.tool_executed']  // Subscribe to all message types
    );
    return unsubscribe;
  }, [subscribe, addMessage, startStreaming, appendStreamContent, endStreaming, addToast]);

  function handleMessage(message: any) {
    console.log('[Handler] Message received:', message.type);

    // Unwrap data envelope if present
    const dataType = message.data?.type || message.dataType;
    const messageType = dataType || message.type;

    switch (messageType) {
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
      case 'operation.tool_executed':
        // Unwrap the data if it's wrapped
        const toolData = message.data || message;
        handleToolExecuted(toolData);
        break;
      default:
        console.log('[Handler] Unhandled message type:', messageType);
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

  function handleToolExecuted(message: any) {
    console.log('[Handler] Tool executed:', message);

    const { tool_type, tool_name, summary, success, details } = message;

    // Show toast notification for file operations
    if (tool_type === 'file_write' || tool_type === 'file_edit' || tool_type === 'file_read') {
      addToast({
        type: success ? 'success' : 'error',
        message: summary,
        duration: 4000,
      });
    }

    // Add tool execution to the current streaming message or latest assistant message
    const { streamingMessageId, messages } = useChatStore.getState();
    const targetMessageId = streamingMessageId || messages.filter(m => m.role === 'assistant').pop()?.id;

    if (targetMessageId) {
      addToolExecution(targetMessageId, {
        toolName: tool_name || 'unknown',
        toolType: tool_type || 'unknown',
        summary: summary || 'Tool executed',
        success: success !== false, // Default to true if not specified
        details,
        timestamp: Date.now()
      });
      console.log('[Handler] Added tool execution to message:', targetMessageId);
    } else {
      console.warn('[Handler] No target message found for tool execution');
    }
  }
};
