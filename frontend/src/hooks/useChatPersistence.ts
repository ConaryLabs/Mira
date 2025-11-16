// src/hooks/useChatPersistence.ts
// Backend-driven chat persistence with artifact restoration
// FIXED: Uses centralized config for session ID

import { useEffect, useCallback, useRef } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useArtifactState, useAppState } from '../stores/useAppState';
import { getSessionId } from '../config/app';
import type { ChatMessage } from '../stores/useChatStore';
import type { Artifact } from '../stores/useChatStore';

export const useChatPersistence = (connectionState: string) => {
  const send = useWebSocketStore(state => state.send);
  const subscribe = useWebSocketStore(state => state.subscribe);
  const setMessages = useChatStore(state => state.setMessages);
  const { addArtifact } = useArtifactState();
  const hasLoadedHistory = useRef(false);

  // Convert backend memory entries to frontend messages
  const convertMemoryToMessages = useCallback((memories: any[]): ChatMessage[] => {
    // DEBUG: Check what roles we're getting from backend
    const roleCounts = memories.reduce((acc, m) => {
      acc[m.role] = (acc[m.role] || 0) + 1;
      return acc;
    }, {} as Record<string, number>);
    console.log('[ChatPersistence] Role breakdown from backend:', roleCounts);
    console.log('[ChatPersistence] First 3 memories:', memories.slice(0, 3));
    
    if (!Array.isArray(memories)) {
      console.warn('Expected array of memories, got:', typeof memories, memories);
      return [];
    }

    const validMessages: ChatMessage[] = [];
    let hasArtifacts = false;
    
    for (const [index, memory] of memories.entries()) {
      if (!memory || !memory.content) continue;
      
      try {
        // Handle timestamp - backend may send as string or number
        let timestamp = Date.now();
        if (memory.timestamp) {
          if (typeof memory.timestamp === 'string') {
            timestamp = new Date(memory.timestamp).getTime();
          } else if (typeof memory.timestamp === 'number') {
            // Handle both milliseconds and seconds
            timestamp = memory.timestamp > 1e12 ? memory.timestamp : memory.timestamp * 1000;
          }
        }

        const message: ChatMessage = {
          id: memory.id?.toString() || `loaded-${index}-${timestamp}`,
          role: (memory.role as 'user' | 'assistant' | 'system') || 'user',
          content: memory.content || '',
          timestamp,
          metadata: {
            session_id: memory.session_id,
            salience: memory.salience,
            mood: memory.mood,
            intent: memory.intent,
            topics: memory.topics ? (typeof memory.topics === 'string' ? 
              JSON.parse(memory.topics) : memory.topics) : undefined,
            programming_lang: memory.programming_lang,
            contains_code: memory.contains_code,
          }
        };

        // Extract artifacts from memory if present
        // Backend stores artifacts in analysis.artifacts
        const artifacts = memory.analysis?.artifacts || memory.artifacts;
        if (artifacts && Array.isArray(artifacts) && artifacts.length > 0) {
          console.log(`[ChatPersistence] Found ${artifacts.length} artifact(s) in memory ${memory.id}`);
          
          // Reconstruct artifact objects
          const reconstructedArtifacts: Artifact[] = artifacts.map((art: any) => ({
            id: art.id || `artifact-${Date.now()}-${Math.random()}`,
            title: art.title || 'Untitled',
            content: art.content || '',
            language: art.language || 'text',
            path: art.path || null,
            timestamp: timestamp,
          }));

          // Add artifacts to both the message and the artifact store
          message.artifacts = reconstructedArtifacts;
          
          // Add each artifact to the global store
          reconstructedArtifacts.forEach(artifact => {
            addArtifact(artifact);
          });
          
          hasArtifacts = true;
        }
        
        validMessages.push(message);
      } catch (error) {
        console.error('Error converting memory to message:', error, memory);
      }
    }
    
    // Sort by timestamp (oldest first) and deduplicate
    const sortedMessages = validMessages
      .sort((a, b) => a.timestamp - b.timestamp)
      .filter((message, index, array) => {
        // Remove duplicates based on content and timestamp
        return index === array.findIndex(m => 
          m.content === message.content && 
          Math.abs(m.timestamp - message.timestamp) < 1000 // Within 1 second
        );
      });

    console.log(`[ChatPersistence] Converted ${memories.length} memories to ${sortedMessages.length} messages`);
    
    // If we restored artifacts, open the panel
    if (hasArtifacts) {
      console.log('[ChatPersistence] Artifacts restored, opening panel');
      setTimeout(() => {
        useAppState.getState().setShowArtifacts(true);
      }, 500);
    }
    
    return sortedMessages;
  }, [addArtifact]);

  // Handle incoming memory data from backend
  const handleMemoryData = useCallback((data: any) => {
    console.log('[ChatPersistence] Processing memory data:', data);

    if (!data) return;

    // Handle memory stats response
    if (data.stats) {
      console.log('[ChatPersistence] Memory stats:', data.stats);
      return;
    }

    // Handle recent memories response
    if (data.memories) {
      const loadedMessages = convertMemoryToMessages(data.memories);
      console.log('[ChatPersistence] Loaded', loadedMessages.length, 'messages from backend');

      // Only set messages if we haven't loaded history yet
      if (!hasLoadedHistory.current) {
        setMessages(loadedMessages);
        hasLoadedHistory.current = true;
      } else {
        // Merge intelligently if we already have messages
        const currentMessages = useChatStore.getState().messages;
        const existingIds = new Set(currentMessages.map(m => m.id));
        const newMessages = loadedMessages.filter(m => !existingIds.has(m.id));

        if (newMessages.length > 0) {
          console.log(`[ChatPersistence] Adding ${newMessages.length} new historical messages`);
          const merged = [...newMessages, ...currentMessages].sort((a, b) => a.timestamp - b.timestamp);
          setMessages(merged);
        }
      }
      return;
    }

    // Handle success/status
    if (data.status === 'success' || data.success === true) {
      console.log('[ChatPersistence] Memory command completed successfully');
      return;
    }

    // Ignore terminal-related messages (handled by terminal handler)
    const terminalIndicators = ['working_directory', 'terminal_id'];
    if (terminalIndicators.some(key => key in data)) {
      // Silently ignore - terminal handler will process
      return;
    }

    // FIXED: Ignore document-related messages (handled by document components)
    const documentTypes = ['document_list', 'document_search_results', 'document_content'];
    if (data.type && documentTypes.includes(data.type)) {
      console.log(`[ChatPersistence] Ignoring document message: ${data.type}`);
      return;
    }

    console.log('[ChatPersistence] Unhandled memory data:', data);
  }, [convertMemoryToMessages, setMessages]);

  // Load chat history from backend - only once per connection
  const loadChatHistory = useCallback(async () => {
    if (connectionState !== 'connected' || hasLoadedHistory.current) return;

    const sessionId = getSessionId(); // FIXED: Use centralized config
    
    try {
      console.log('[ChatPersistence] Loading chat history for session:', sessionId);
      
      await send({
        type: 'memory_command',
        method: 'memory.get_recent',
        params: {
          session_id: sessionId,
          count: 100
        }
      });
      
      console.log('[ChatPersistence] History request sent');
    } catch (error) {
      console.error('[ChatPersistence] Failed to load chat history:', error);
      hasLoadedHistory.current = true; // Don't retry on error
    }
  }, [connectionState, send]);

  // Subscribe to memory data messages (FIXED: removed duplicate)
  useEffect(() => {
    const unsubscribe = subscribe('chat-persistence', (message) => {
      if (message.type === 'data' && message.data) {
        handleMemoryData(message.data);
      }
    });
    
    return unsubscribe;
  }, [subscribe, handleMemoryData]);

  // Load history when connected - but only once
  useEffect(() => {
    if (connectionState === 'connected' && !hasLoadedHistory.current) {
      // Small delay to let connection settle
      const timer = setTimeout(() => {
        loadChatHistory();
      }, 100);
      
      return () => clearTimeout(timer);
    }
  }, [connectionState, loadChatHistory]);

  // Reset history flag when disconnected
  useEffect(() => {
    if (connectionState === 'disconnected') {
      hasLoadedHistory.current = false;
    }
  }, [connectionState]);

  return {
    getSessionId, // Export for compatibility (now references the centralized function)
    loadChatHistory,
    handleMemoryData,
  };
};
