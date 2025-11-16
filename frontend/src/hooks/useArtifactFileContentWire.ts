// src/hooks/useArtifactFileContentWire.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';
import { createArtifact } from '../utils/artifact';

export function useArtifactFileContentWire() {
  const subscribe = useWebSocketStore(state => state.subscribe);

  useEffect(() => {
    const unsubscribe = subscribe('artifact-file-wire', (message) => {
      const payload = message.type === 'data' || message.type === 'response' ? message.data : null;
      if (!payload || payload.type !== 'file_content') return;

      const artifact = createArtifact(payload, { idPrefix: 'file' });
      if (!artifact) return;

      const { addArtifact, setShowArtifacts } = useAppState.getState();
      addArtifact(artifact);
      setShowArtifacts(true);
    }, ['data', 'response']);

    return unsubscribe;
  }, [subscribe]);
}
