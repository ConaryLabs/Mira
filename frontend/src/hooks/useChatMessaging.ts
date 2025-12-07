// src/hooks/useChatMessaging.ts
// Enhanced with waiting state for batch responses
// Uses user ID from auth store for session ID

import { useCallback } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState, useArtifactState, SystemAccessMode } from '../stores/useAppState';
import { useCurrentUser } from '../stores/useAuthStore';
import { detectLanguage } from '../utils/language';

export const useChatMessaging = () => {
  const send = useWebSocketStore(state => state.send);
  const addMessage = useChatStore(state => state.addMessage);
  const setWaitingForResponse = useChatStore(state => state.setWaitingForResponse);
  const { currentProject, modifiedFiles, currentBranch, systemAccessMode } = useAppState();
  const { activeArtifact } = useArtifactState();
  const user = useCurrentUser();

  const handleSend = useCallback(async (content: string) => {
    // Guard against empty messages
    const trimmedContent = content?.trim();
    if (!trimmedContent) {
      console.warn('[useChatMessaging] Blocked empty message send');
      return;
    }

    // Add user message immediately
    const userMessage = {
      id: `user-${Date.now()}`,
      role: 'user' as const,
      content: trimmedContent,
      timestamp: Date.now()
    };

    addMessage(userMessage);
    
    // Set waiting state BEFORE sending
    setWaitingForResponse(true);

    // Build message with full context
    const message = {
      type: 'chat',
      content: trimmedContent,
      project_id: currentProject?.id || null,
      system_access_mode: systemAccessMode,
      metadata: {
        session_id: user?.id || 'anonymous',
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
  }, [send, currentProject, activeArtifact, modifiedFiles, currentBranch, systemAccessMode, addMessage, setWaitingForResponse]);

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
