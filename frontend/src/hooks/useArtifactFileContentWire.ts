// src/hooks/useArtifactFileContentWire.ts
// Dedicated wire to pipe backend file_content messages straight into Artifact store
// This is defensive: even if other handlers miss it, this ensures the panel opens.

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';
import type { Artifact } from '../stores/useChatStore';

function detectLanguage(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase();
  const map: Record<string, string> = {
    rs: 'rust', js: 'javascript', jsx: 'javascript',
    ts: 'typescript', tsx: 'typescript', py: 'python',
    go: 'go', java: 'java', cpp: 'cpp', c: 'c',
    html: 'html', css: 'css', json: 'json',
    yaml: 'yaml', yml: 'yaml', toml: 'toml',
    md: 'markdown', sql: 'sql', sh: 'shell', bash: 'shell',
  };
  return map[ext || ''] || 'plaintext';
}

function normalizePath(rawPath: string): string {
  return String(rawPath)
    .replace(/\\/g, '/')        // windows -> posix
    .replace(/\/{2,}/g, '/')     // collapse duplicate slashes
    .replace(/^\.\/+/, '');     // strip leading ./
}

export function useArtifactFileContentWire() {
  const subscribe = useWebSocketStore(state => state.subscribe);

  useEffect(() => {
    const unsubscribe = subscribe('artifact-file-wire', (message) => {
      const payload = message.type === 'data' || message.type === 'response' ? message.data : null;
      if (!payload || payload.type !== 'file_content') return;

      const rawPath = payload.path || payload.file_path || payload.name || 'untitled';
      const path = normalizePath(rawPath);
      const content = payload.content ?? payload.file_content ?? payload.text ?? payload.body ?? payload.value;
      if (typeof content === 'undefined') return;

      const artifact: Artifact = {
        id: `file-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        path,
        content,
        language: detectLanguage(path),
      };

      const { addArtifact, setShowArtifacts } = useAppState.getState();
      addArtifact(artifact);
      setShowArtifacts(true);
    }, ['data', 'response']);

    return unsubscribe;
  }, [subscribe]);
}
