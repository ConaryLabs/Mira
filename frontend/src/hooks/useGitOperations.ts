// src/hooks/useGitOperations.ts
// Custom hook for git operations with proper async handling

import { useCallback, useState } from 'react';
import { useAppState } from '../stores/useAppState';
import { useWebSocketStore } from '../stores/useWebSocketStore';

export const useGitOperations = () => {
  const { addToast } = useAppState();
  const { send } = useWebSocketStore();

  const [isAttaching, setIsAttaching] = useState(false);

  const attachCodebase = useCallback(async (
    projectId: string,
    type: 'local' | 'git',
    data: { path?: string; url?: string }
  ) => {
    setIsAttaching(true);

    try {
      if (type === 'local') {
        await send({
          type: 'project_command',
          method: 'project.attach_local',
          params: {
            project_id: projectId,
            directory_path: data.path
          }
        });

        addToast({ message: 'Local codebase attached successfully', type: 'success' });
        return true;
      } else {
        // Git import: attach → clone → import
        addToast({ message: 'Attaching repository...', type: 'info' });

        await send({
          type: 'git_command',
          method: 'git.attach',
          params: {
            project_id: projectId,
            repo_url: data.url
          }
        });

        // Small delay to let attach complete
        await new Promise(resolve => setTimeout(resolve, 500));

        addToast({ message: 'Cloning repository...', type: 'info' });

        await send({
          type: 'git_command',
          method: 'git.clone',
          params: {
            project_id: projectId
          }
        });

        // Give clone time to complete - this is the longest operation
        await new Promise(resolve => setTimeout(resolve, 3000));

        addToast({ message: 'Importing codebase...', type: 'info' });

        await send({
          type: 'git_command',
          method: 'git.import',
          params: {
            project_id: projectId
          }
        });

        // Brief delay for import
        await new Promise(resolve => setTimeout(resolve, 1000));

        addToast({ message: 'Repository imported successfully!', type: 'success' });
        return true;
      }
    } catch (error) {
      console.error('Attach codebase failed:', error);
      addToast({ message: 'Failed to attach codebase', type: 'error' });
      return false;
    } finally {
      setIsAttaching(false);
    }
  }, [send, addToast]);

  return {
    attachCodebase,
    isAttaching,
  };
};
