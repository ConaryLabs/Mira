// src/hooks/useProjectOperations.ts
// Custom hook for project CRUD operations

import { useCallback, useState } from 'react';
import { useAppState } from '../stores/useAppState';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import type { Project } from '../types';

export const useProjectOperations = () => {
  const { currentProject, setCurrentProject, addToast } = useAppState();
  const { send } = useWebSocketStore();

  const [opening, setOpening] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  const refreshProjects = useCallback(() => {
    send({
      type: 'project_command',
      method: 'project.list',
      params: {}
    });
  }, [send]);

  const openDirectory = useCallback(async (path: string) => {
    if (!path.trim()) {
      addToast({ message: 'Directory path is required', type: 'error' });
      return false;
    }

    setOpening(true);
    try {
      await send({
        type: 'project_command',
        method: 'project.open_directory',
        params: {
          path: path.trim()
        }
      });

      const dirName = path.split('/').pop() || path;
      addToast({ message: `Opened project: ${dirName}`, type: 'success' });

      // Refresh project list
      setTimeout(refreshProjects, 100);
      return true;
    } catch (error) {
      console.error('Open directory failed:', error);
      addToast({ message: 'Failed to open directory', type: 'error' });
      return false;
    } finally {
      setOpening(false);
    }
  }, [send, addToast, refreshProjects]);

  const createProject = useCallback(async (name: string, description?: string) => {
    if (!name.trim()) {
      addToast({ message: 'Project name is required', type: 'error' });
      return false;
    }

    setOpening(true);
    try {
      await send({
        type: 'project_command',
        method: 'project.create',
        params: {
          name: name.trim(),
          description: description?.trim() || undefined
        }
      });

      addToast({ message: `Created project: ${name}`, type: 'success' });

      // Refresh project list
      setTimeout(refreshProjects, 100);
      return true;
    } catch (error) {
      console.error('Create project failed:', error);
      addToast({ message: 'Failed to create project', type: 'error' });
      return false;
    } finally {
      setOpening(false);
    }
  }, [send, addToast, refreshProjects]);

  const deleteProject = useCallback(async (projectId: string) => {
    setDeleting(projectId);

    try {
      await send({
        type: 'project_command',
        method: 'project.delete',
        params: { id: projectId }
      });

      if (currentProject?.id === projectId) {
        setCurrentProject(null);
      }

      addToast({ message: 'Project deleted', type: 'success' });

      // Refresh project list
      setTimeout(refreshProjects, 100);
      return true;
    } catch (error) {
      console.error('Delete project failed:', error);
      addToast({ message: 'Failed to delete project', type: 'error' });
      return false;
    } finally {
      setDeleting(null);
    }
  }, [send, currentProject, setCurrentProject, addToast, refreshProjects]);

  const selectProject = useCallback((project: Project) => {
    setCurrentProject(project);
  }, [setCurrentProject]);

  return {
    openDirectory,
    createProject,
    deleteProject,
    selectProject,
    refreshProjects,
    opening,
    deleting,
  };
};
