// src/hooks/useChatMessaging.ts
// Enhanced with waiting state for batch responses
// FIXED: Uses centralized config for session ID

import { useCallback } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState, useArtifactState } from '../stores/useAppState';
import { getSessionId } from '../config/app';
import { detectLanguage } from '../utils/language';

export const useChatMessaging = () => {
  const send = useWebSocketStore(state => state.send);
  const addMessage = useChatStore(state => state.addMessage);
  const setWaitingForResponse = useChatStore(state => state.setWaitingForResponse);
  const { currentProject, modifiedFiles, currentBranch } = useAppState();
  const { activeArtifact } = useArtifactState();

  const handleSend = useCallback(async (content: string) => {
    // Add user message immediately
    const userMessage = {
      id: `user-${Date.now()}`,
      role: 'user' as const,
      content,
      timestamp: Date.now()
    };
    
    addMessage(userMessage);
    
    // Set waiting state BEFORE sending
    setWaitingForResponse(true);

    // Build message with full context
    const message = {
      type: 'chat',
      content,
      project_id: currentProject?.id || null,
      metadata: {
        session_id: getSessionId(), // FIXED: Use centralized config
        timestamp: Date.now(),
        
        // FILE CONTEXT (use path instead of linkedFile)
        file_path: activeArtifact?.path || null,
        file_content: activeArtifact?.content || null, 
        language: activeArtifact ? detectLanguage(activeArtifact.path) : null,
        
        // PROJECT CONTEXT
        has_repository: currentProject?.has_repository || false,
        current_branch: currentBranch || 'main',
        modified_files_count: modifiedFiles.length,
      }
    };

    console.log('[useChatMessaging] Sending message with context:', {
      hasProject: !!currentProject,
      projectHasRepo: currentProject?.has_repository ? 'yes' : 'no',
      activeFile: activeArtifact?.path || 'none',
      fileSize: activeArtifact?.content?.length || 0,
      language: activeArtifact ? detectLanguage(activeArtifact.path) : 'none',
      modifiedFiles: modifiedFiles.length,
      artifactId: activeArtifact?.id || 'none',
    });

    try {
      await send(message);
    } catch (error) {
      console.error('[useChatMessaging] Send failed:', error);
      // Clear waiting state on error
      setWaitingForResponse(false);
    }
  }, [send, currentProject, activeArtifact, modifiedFiles, currentBranch, addMessage, setWaitingForResponse]);

  const addSystemMessage = useCallback((content: string) => {
    addMessage({
      id: `sys-${Date.now()}`,
      role: 'system' as const,
      content,
      timestamp: Date.now()
    });
  }, [addMessage]);

  // Add helper to notify when project context changes
  const addProjectContextMessage = useCallback((projectName: string) => {
    addSystemMessage(`Now working in project: ${projectName}`);
  }, [addSystemMessage]);

  // Add file context message when switching files
  const addFileContextMessage = useCallback((fileName: string) => {
    addSystemMessage(`Now viewing: ${fileName}`);
  }, [addSystemMessage]);

  return { 
    handleSend, 
    addSystemMessage, 
    addProjectContextMessage,
    addFileContextMessage
  };
};
