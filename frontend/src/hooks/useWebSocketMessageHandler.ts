// src/hooks/useWebSocketMessageHandler.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useAppState } from '../stores/useAppState';
import { useChatStore } from '../stores/useChatStore';
import { useActivityStore } from '../stores/useActivityStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { createArtifact, extractArtifacts } from '../utils/artifact';

export const useWebSocketMessageHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const send = useWebSocketStore(state => state.send);
  
  const {
    setProjects,
    addModifiedFile,
    clearModifiedFiles,
    setShowFileExplorer,
    addArtifact
  } = useAppState();

  const {
    startStreaming,
    appendStreamContent,
    endStreaming,
    addMessage,
    updateMessagePlan,
    addMessageTask,
    updateTaskStatus,
    setMessageOperationId,
    streamingMessageId,
    addToolExecution
  } = useChatStore();

  const { setCurrentOperation, clearCurrentOperation } = useActivityStore();

  useEffect(() => {
    const unsubscribe = subscribe(
      'global-message-handler',
      (message) => {
        handleMessage(message);
      },
      ['data', 'status', 'error']
    );

    return unsubscribe;
  }, [subscribe]);

  const handleMessage = (message: any) => {
    if (!message || typeof message !== 'object') {
      console.warn('Received invalid message:', message);
      return;
    }

    switch (message.type) {
      case 'data':
        if (message.data) {
          handleDataMessage(message.data);
        }
        break;
        
      case 'status':
        console.log('Status:', message.message);
        if (message.message && message.message.includes('deleted')) {
          console.log('Project deleted, refreshing list');
          send({
            type: 'project_command',
            method: 'project.list',
            params: {}
          });
        }
        break;
        
      case 'error':
        console.error('Backend error:', message.error || message.message || 'Unknown error');
        break;
        
      case 'heartbeat':
        // Ignore heartbeat messages
        break;
        
      default:
        console.log('Unhandled message type:', message.type);
        break;
    }
  };

  const handleDataMessage = (data: any) => {
    const dtype = data?.type;
    if (!dtype) return;
    
    console.log('[WS-Global] Handling data type:', dtype);

    switch (dtype) {
      // NEW OPERATION PROTOCOL
      case 'operation.streaming': {
        // Token streaming during response
        if (data.content) {
          appendStreamContent(data.content);
        }
        return;
      }

      case 'operation.artifact_completed': {
        // Artifact completed - add it immediately
        console.log('[WS-Global] Artifact completed:', data.artifact);

        const artifact = createArtifact(data.artifact);
        if (!artifact) {
          console.warn('[WS-Global] Invalid artifact:', data.artifact);
          return;
        }

        console.log('[WS-Global] Adding artifact:', artifact.path);
        addArtifact(artifact);
        return;
      }

      case 'operation.completed': {
        // Operation done - finalize streaming and add message
        console.log('[WS-Global] Operation completed');

        // End streaming (this adds the message)
        endStreaming();

        // Artifacts should already be added via operation.artifact_completed
        // But if they're in the final message, add them too
        const artifacts = extractArtifacts(data);
        if (artifacts.length > 0) {
          console.log('[WS-Global] Processing artifacts from completed:', artifacts.length);
          artifacts.forEach(artifact => addArtifact(artifact));
        }

        // Clear current operation from activity panel after a delay
        // (keep it visible for a bit so user can see the completed state)
        setTimeout(() => clearCurrentOperation(), 2000);

        return;
      }

      case 'operation.started': {
        // Operation started - begin streaming
        console.log('[WS-Global] Operation started:', data.operation_id);
        startStreaming();

        // Track operation in activity panel
        if (data.operation_id && streamingMessageId) {
          setCurrentOperation(data.operation_id, streamingMessageId);
        }
        return;
      }

      case 'operation.status_changed': {
        // Status update - log it
        console.log('[WS-Global] Operation status:', data.status);
        return;
      }

      // PLANNING MODE & TASK TRACKING
      case 'operation.plan_generated': {
        // Plan was generated for the operation
        console.log('[WS-Global] Plan generated:', data.operation_id);

        if (streamingMessageId) {
          // Set operation ID on the streaming message
          setMessageOperationId(streamingMessageId, data.operation_id);

          // Update message with plan
          updateMessagePlan(streamingMessageId, {
            plan_text: data.plan_text,
            reasoning_tokens: data.reasoning_tokens,
            timestamp: data.timestamp
          });
        }
        return;
      }

      case 'operation.task_created': {
        // Task was created for tracking
        console.log('[WS-Global] Task created:', data.task_id, data.description);

        if (streamingMessageId) {
          addMessageTask(streamingMessageId, {
            task_id: data.task_id,
            sequence: data.sequence,
            description: data.description,
            active_form: data.active_form,
            status: 'pending',
            timestamp: data.timestamp
          });
        }
        return;
      }

      case 'operation.task_started': {
        // Task execution started
        console.log('[WS-Global] Task started:', data.task_id);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'running');
        }
        return;
      }

      case 'operation.task_completed': {
        // Task finished successfully
        console.log('[WS-Global] Task completed:', data.task_id);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'completed');
        }
        return;
      }

      case 'operation.task_failed': {
        // Task failed
        console.log('[WS-Global] Task failed:', data.task_id, data.error);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'failed', data.error);
        }
        return;
      }

      // LEGACY: Old artifact_created format
      case 'artifact_created': {
        console.log('[WS-Global] Legacy artifact created:', data.artifact);

        const artifact = createArtifact(data.artifact);
        if (!artifact) {
          console.warn('[WS-Global] artifact_created missing content:', data);
          return;
        }

        addArtifact(artifact);
        return;
      }

      // FILE OPERATIONS
      case 'file_content': {
        const artifact = createArtifact(data, {
          idPrefix: 'file',
          status: 'saved',
          origin: 'user'
        });

        if (!artifact) {
          console.warn('[WS-Global] Invalid file_content:', data);
          return;
        }

        console.log('[WS-Global] File content artifact:', artifact.path);
        addArtifact(artifact);
        setShowFileExplorer(true);
        return;
      }

      // PROJECT MANAGEMENT
      case 'projects': {
        console.log('Projects data received:', data.projects?.length || 0, 'projects');
        if (data.projects) {
          setProjects(data.projects);
        }
        return;
      }

      case 'project_list': {
        console.log('Processing project list:', data.projects?.length || 0, 'projects');
        if (data.projects && Array.isArray(data.projects)) {
          setProjects(data.projects);
          console.log('Projects updated in state');
        }
        return;
      }

      case 'project_created': {
        console.log('Project created:', data.project?.name);
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
        return;
      }

      case 'local_directory_attached': {
        console.log('Local directory attached:', data.path);
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
        return;
      }

      // GIT STATUS
      case 'git_status': {
        console.log('Git status update:', data.status);
        if (data.status === 'synced' || data.status === 'modified') {
          if (data.modified_files) {
            clearModifiedFiles();
            data.modified_files.forEach((file: string) => addModifiedFile(file));
          }
        }
        return;
      }

      default:
        console.log('[WS-Global] Unhandled data type:', dtype);
    }
  };
};
