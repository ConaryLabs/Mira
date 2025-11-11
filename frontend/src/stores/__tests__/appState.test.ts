// src/stores/__tests__/appState.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { useAppState } from '../useAppState';
import type { Artifact } from '../useChatStore';

describe('useAppState - Artifact Management', () => {
  beforeEach(() => {
    localStorage.clear();
    useAppState.setState({
      artifacts: [],
      activeArtifactId: null,
      appliedFiles: new Set(),
    });
  });

  describe('De-duplication Logic', () => {
    it('de-dupes by artifact id', () => {
      const store = useAppState.getState();
      
      store.addArtifact({ 
        id: 'art-1', 
        path: 'test.ts', 
        content: 'v1',
        language: 'typescript',
        timestamp: Date.now(),
      });
      
      store.addArtifact({ 
        id: 'art-1', 
        path: 'test.ts', 
        content: 'v2',  // Updated content
        language: 'typescript',
        timestamp: Date.now(),
      });
      
      const currentState = useAppState.getState();
      expect(currentState.artifacts).toHaveLength(1);
      expect(currentState.artifacts[0].content).toBe('v2');
    });

    it('de-dupes by file path', () => {
      const store = useAppState.getState();
      
      store.addArtifact({ 
        id: 'art-1', 
        path: 'src/test.ts', 
        content: 'v1',
        language: 'typescript',
        timestamp: Date.now(),
      });
      
      // Different id, same path → should merge
      store.addArtifact({ 
        id: 'art-2', 
        path: 'src/test.ts', 
        content: 'v2',
        language: 'typescript',
        timestamp: Date.now(),
      });
      
      const currentState = useAppState.getState();
      expect(currentState.artifacts).toHaveLength(1);
      expect(currentState.artifacts[0].id).toBe('art-1'); // Preserves original id
      expect(currentState.artifacts[0].content).toBe('v2'); // Updates content
    });

    it('focuses de-duped artifact', () => {
      const store = useAppState.getState();
      
      store.addArtifact({ id: 'art-1', path: 'test.ts', content: 'v1', language: 'typescript', timestamp: Date.now() });
      store.addArtifact({ id: 'art-2', path: 'other.ts', content: 'v1', language: 'typescript', timestamp: Date.now() });
      
      let currentState = useAppState.getState();
      expect(currentState.activeArtifactId).toBe('art-2');
      
      // Re-add art-1 → should refocus it
      store.addArtifact({ id: 'art-1', path: 'test.ts', content: 'v2', language: 'typescript', timestamp: Date.now() });
      
      currentState = useAppState.getState();
      expect(currentState.activeArtifactId).toBe('art-1');
      expect(currentState.showArtifacts).toBe(true);
    });
  });

  describe('Applied Files Tracking', () => {
    it('tracks applied artifacts', () => {
      const store = useAppState.getState();
      
      store.markArtifactApplied('art-1');
      store.markArtifactApplied('art-2');
      
      expect(store.isArtifactApplied('art-1')).toBe(true);
      expect(store.isArtifactApplied('art-2')).toBe(true);
      expect(store.isArtifactApplied('art-3')).toBe(false);
    });

    it('unmarks applied artifacts', () => {
      const store = useAppState.getState();
      
      store.markArtifactApplied('art-1');
      expect(store.isArtifactApplied('art-1')).toBe(true);
      
      store.markArtifactUnapplied('art-1');
      expect(store.isArtifactApplied('art-1')).toBe(false);
    });

    it('preserves Set after multiple operations', () => {
      const store = useAppState.getState();
      
      store.markArtifactApplied('art-1');
      store.markArtifactApplied('art-2');
      store.markArtifactUnapplied('art-1');
      store.markArtifactApplied('art-3');
      
      const currentState = useAppState.getState();
      expect(currentState.appliedFiles).toBeInstanceOf(Set);
      expect(currentState.appliedFiles.size).toBe(2);
      expect([...currentState.appliedFiles]).toEqual(expect.arrayContaining(['art-2', 'art-3']));
    });
  });

  describe('Artifact Removal', () => {
    it('removes artifact and updates active selection', () => {
      const store = useAppState.getState();
      
      store.addArtifact({ id: 'art-1', path: 'a.ts', content: '', language: 'typescript', timestamp: Date.now() });
      store.addArtifact({ id: 'art-2', path: 'b.ts', content: '', language: 'typescript', timestamp: Date.now() });
      
      store.setActiveArtifact('art-1');
      store.removeArtifact('art-1');
      
      const currentState = useAppState.getState();
      expect(currentState.artifacts).toHaveLength(1);
      expect(currentState.activeArtifactId).toBe('art-2'); // Falls back to first remaining
    });

    it('hides panel when last artifact removed', () => {
      const store = useAppState.getState();
      
      store.addArtifact({ id: 'art-1', path: 'test.ts', content: '', language: 'typescript', timestamp: Date.now() });
      
      let currentState = useAppState.getState();
      expect(currentState.showArtifacts).toBe(true);
      
      store.removeArtifact('art-1');
      
      currentState = useAppState.getState();
      expect(currentState.artifacts).toHaveLength(0);
      expect(currentState.showArtifacts).toBe(false);
      expect(currentState.activeArtifactId).toBeNull();
    });
  });
});
