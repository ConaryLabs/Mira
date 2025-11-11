// src/hooks/useWebSocketMessageHandler.ts
// FIXED: Handle operation.streaming, operation.artifact_completed, operation.completed

import { useEffect } from 'react';
import { useAppState } from '../stores/useAppState';
import { useChatStore } from '../stores/useChatStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import type { Artifact } from '../stores/useChatStore';

export const useWebSocketMessageHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const send = useWebSocketStore(state => state.send);
  
  const { 
    setProjects,
    updateGitStatus, 
    addModifiedFile,
    clearModifiedFiles,
    setShowFileExplorer,
    addArtifact
  } = useAppState();

  const {
    startStreaming,
    appendStreamContent,
    endStreaming,
    addMessage
  } = useChatStore();

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

  const detectLanguage = (filePath: string): string => {
    if (!filePath) return 'plaintext';
    
    const ext = filePath.split('.').pop()?.toLowerCase();
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
      case 'sh': case 'bash': return 'shell';
      case 'go': return 'go';
      case 'java': return 'java';
      case 'cpp': return 'cpp';
      case 'c': return 'c';
      case 'sql': return 'sql';
      default: return 'plaintext';
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
        
        if (!data.artifact || !data.artifact.content) {
          console.warn('[WS-Global] Invalid artifact:', data.artifact);
          return;
        }

        const artifact = data.artifact;
        const newArtifact: Artifact = {
          id: artifact.id || `artifact-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
          path: artifact.path || 'untitled',
          content: artifact.content,
          language: artifact.language || detectLanguage(artifact.path),
          status: 'draft',
          origin: 'llm',
          timestamp: Date.now()
        };

        console.log('[WS-Global] Adding artifact:', newArtifact.path);
        addArtifact(newArtifact);
        return;
      }

      case 'operation.completed': {
        // Operation done - finalize streaming and add message
        console.log('[WS-Global] Operation completed');
        
        // Get the final content from streaming buffer
        const streamingContent = useChatStore.getState().streamingContent;
        
        // End streaming (this adds the message)
        endStreaming();
        
        // Artifacts should already be added via operation.artifact_completed
        // But if they're in the final message, add them too
        if (data.artifacts && data.artifacts.length > 0) {
          console.log('[WS-Global] Processing artifacts from completed:', data.artifacts.length);
          data.artifacts.forEach((artifact: any) => {
            if (artifact && artifact.content) {
              const newArtifact: Artifact = {
                id: artifact.id || `artifact-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
                path: artifact.path || 'untitled',
                content: artifact.content,
                language: artifact.language || detectLanguage(artifact.path),
                status: 'draft',
                origin: 'llm',
                timestamp: Date.now()
              };
              addArtifact(newArtifact);
            }
          });
        }
        
        return;
      }

      case 'operation.started': {
        // Operation started - begin streaming
        console.log('[WS-Global] Operation started:', data.operation_id);
        startStreaming();
        return;
      }

      case 'operation.status_changed': {
        // Status update - log it
        console.log('[WS-Global] Operation status:', data.status);
        return;
      }

      // LEGACY: Old artifact_created format
      case 'artifact_created': {
        console.log('[WS-Global] Legacy artifact created:', data.artifact);
        
        const artifactData = data.artifact;
        if (!artifactData || !artifactData.content) {
          console.warn('[WS-Global] artifact_created missing content:', data);
          return;
        }

        const path = artifactData.path || artifactData.title || 'untitled';
        const newArtifact: Artifact = {
          id: artifactData.id || `artifact-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
          path,
          content: artifactData.content,
          language: artifactData.language || detectLanguage(path),
          changeType: artifactData.change_type,
          status: 'draft',
          origin: 'llm',
          timestamp: Date.now()
        };

        addArtifact(newArtifact);
        return;
      }

      // FILE OPERATIONS
      case 'file_content': {
        const rawPath = data.path || data.file_path || data.name || 'untitled';
        const path = String(rawPath).replace(/\/+/, '/').replace(/\/+/g, '/');
        const content = data.content ?? data.file_content ?? data.text ?? data.body ?? '// No content';

        const newArtifact: Artifact = {
          id: `file-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`,
          path,
          content,
          language: detectLanguage(path),
          status: 'saved',
          origin: 'user',
          timestamp: Date.now()
        };

        console.log('[WS-Global] File content artifact:', newArtifact.path);
        addArtifact(newArtifact);
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
          updateGitStatus(data.status);
          
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
