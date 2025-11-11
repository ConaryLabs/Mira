// src/hooks/useArtifacts.ts
// REFACTORED: Single save action, apply action, proper status tracking

import { useCallback } from 'react';
import { useAppState, useArtifactState } from '../stores/useAppState';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import type { Artifact } from '../stores/useChatStore';

export interface ArtifactHook {
  artifacts: Artifact[];
  activeArtifact: Artifact | null;
  showArtifacts: boolean;
  addArtifact: (artifact: Artifact) => void;
  setActiveArtifact: (id: string | null) => void;
  updateArtifact: (id: string, updates: Partial<Artifact>) => void;
  updatePath: (id: string, newPath: string) => void;
  removeArtifact: (id: string) => void;
  closeArtifacts: () => void;
  save: (id: string) => Promise<void>;
  apply: (id: string) => Promise<void>;
  discard: (id: string) => void;
  copyArtifact: (id: string) => void;
}

function normalizePath(input: string): string {
  return String(input)
    .replace(/\\/g, '/')
    .replace(/\/{2,}/g, '/')
    .replace(/^\.\/+/, '');
}

export const useArtifacts = (): ArtifactHook => {
  const {
    artifacts,
    activeArtifact,
    showArtifacts,
    addArtifact,
    setActiveArtifact,
    updateArtifact,
    removeArtifact
  } = useArtifactState();
  
  const { setShowArtifacts, currentProject } = useAppState();
  const send = useWebSocketStore(state => state.send);
  
  const closeArtifacts = useCallback(() => {
    setShowArtifacts(false);
  }, [setShowArtifacts]);
  
  // Update path inline
  const updatePath = useCallback((id: string, newPath: string) => {
    updateArtifact(id, { path: normalizePath(newPath) });
  }, [updateArtifact]);
  
  // Save to artifact's current path
  const save = useCallback(async (id: string) => {
    const artifact = artifacts.find(a => a.id === id);
    if (!artifact) {
      console.error('Artifact not found:', id);
      return;
    }
    
    if (!currentProject) {
      console.error('Cannot save artifact: no current project');
      return;
    }
    
    const path = normalizePath(artifact.path);
    
    try {
      await send({
        type: 'file_system_command',
        method: 'files.write',
        params: {
          project_id: currentProject.id,
          path,
          content: artifact.content
        }
      });
      
      updateArtifact(id, { 
        path,
        status: 'saved',
        timestamp: Date.now()
      });
      
      console.log(`✓ Saved: ${path}`);
    } catch (error) {
      console.error('Failed to save artifact:', error);
    }
  }, [artifacts, currentProject, send, updateArtifact]);
  
  // Apply to workspace (same as save but marks as applied)
  const apply = useCallback(async (id: string) => {
    const artifact = artifacts.find(a => a.id === id);
    if (!artifact) {
      console.error('Artifact not found:', id);
      return;
    }
    
    if (!currentProject) {
      console.error('Cannot apply artifact: no current project');
      return;
    }
    
    const path = normalizePath(artifact.path);
    
    try {
      await send({
        type: 'file_system_command',
        method: 'files.write',
        params: {
          project_id: currentProject.id,
          path,
          content: artifact.content
        }
      });
      
      updateArtifact(id, { 
        path,
        status: 'applied',
        timestamp: Date.now()
      });
      
      console.log(`✓ Applied: ${path}`);
    } catch (error) {
      console.error('Failed to apply artifact:', error);
    }
  }, [artifacts, currentProject, send, updateArtifact]);
  
  // Discard changes (remove if draft, revert if saved)
  const discard = useCallback((id: string) => {
    const artifact = artifacts.find(a => a.id === id);
    if (!artifact) return;
    
    if (artifact.status === 'draft') {
      // Just remove drafts
      removeArtifact(id);
    } else {
      // For saved/applied, we'd need to reload from disk
      // For now just remove - can add revert later
      removeArtifact(id);
    }
  }, [artifacts, removeArtifact]);
  
  const copyArtifact = useCallback((id: string) => {
    const artifact = artifacts.find(a => a.id === id);
    if (!artifact) return;
    
    navigator.clipboard.writeText(artifact.content).then(() => {
      console.log('Copied to clipboard');
    }).catch((error) => {
      console.error('Failed to copy:', error);
    });
  }, [artifacts]);
  
  return {
    artifacts,
    activeArtifact,
    showArtifacts,
    addArtifact,
    setActiveArtifact,
    updateArtifact,
    updatePath,
    removeArtifact,
    closeArtifacts,
    save,
    apply,
    discard,
    copyArtifact
  };
};
