// src/hooks/useToolResultArtifactBridge.ts
// Bridge tool_result → Artifact Viewer. If the backend emits a tool_result for
// create_artifact (or anything that clearly contains a file-like payload),
// we synthesize an Artifact and open the panel.

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';
import type { Artifact } from '../stores/useChatStore';

function detectLanguage(filePath?: string): string {
  if (!filePath) return 'plaintext';
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

function normalizePath(raw?: string): string {
  if (!raw) return 'untitled';
  return String(raw)
    .replace(/\\/g, '/')        // Windows → POSIX
    .replace(/\/{2,}/g, '/')     // collapse duplicate slashes
    .replace(/^\.\/+/, '');     // strip leading ./
}

function toArtifact(obj: any): Artifact | null {
  if (!obj) return null;

  // Common field names seen in tool outputs
  const content = obj.content ?? obj.file_content ?? obj.text ?? obj.body ?? obj.value;
  const path = obj.path ?? obj.file_path ?? obj.title;
  const language = obj.language ?? obj.programming_lang;

  if (!content || typeof content !== 'string') return null;

  const cleanPath = normalizePath(typeof path === 'string' ? path : 'untitled');

  return {
    id: obj.id || `artifact-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    path: cleanPath,
    content,
    language: language || detectLanguage(cleanPath),
  };
}

function extractArtifactsFromToolResult(data: any): Artifact[] {
  if (!data) return [];

  // Direct artifact object
  if (data.artifact) {
    const a = toArtifact(data.artifact);
    return a ? [a] : [];
  }

  // Array of artifacts or files
  const arrays = [data.artifacts, data.files, data.results, data.output, data.outputs, data.data];
  for (const arr of arrays) {
    if (Array.isArray(arr)) {
      const mapped = arr.map(toArtifact).filter(Boolean) as Artifact[];
      if (mapped.length) return mapped;
    }
  }

  // Single result object with file-like fields
  if (data.result) {
    const a = toArtifact(data.result);
    return a ? [a] : [];
  }

  // Fallback: if this is explicitly create_artifact, try the top level
  if ((data.tool_name || data.tool) === 'create_artifact') {
    const a = toArtifact(data);
    return a ? [a] : [];
  }

  return [];
}

export function useToolResultArtifactBridge() {
  const subscribe = useWebSocketStore(state => state.subscribe);

  useEffect(() => {
    const unsubscribe = subscribe('artifact-tool-bridge', (message) => {
      // Enhanced logging
      console.log('[artifact-tool-bridge] Received message:', {
        type: message.type,
        keys: Object.keys(message),
        dataKeys: message.data ? Object.keys(message.data) : [],
        dataType: message.data?.type,
        fullMessage: message, // CRITICAL: Log entire message for debugging
      });

      if (message.type !== 'response') return;

      const data = message.data || message;
      if (!data) {
        console.log('[artifact-tool-bridge] No data in message');
        return;
      }

      // Tool result envelopes vary. Support a few:
      // - { type: 'tool_result', tool_name: 'create_artifact', result: {...} }
      // - { type: 'response', data: { tool: 'create_artifact', artifact: {...} } }
      const dtype = data.type || data.data?.type;
      const toolName = data.tool_name || data.tool || data.data?.tool_name;

      const isToolResult = dtype === 'tool_result' || dtype === 'tool' || !!toolName;
      
      console.log('[artifact-tool-bridge] Checking for tool result:', {
        dtype,
        toolName,
        isToolResult,
      });

      if (!isToolResult) {
        console.log('[artifact-tool-bridge] Not a tool result, ignoring');
        return;
      }

      const artifacts = extractArtifactsFromToolResult(data) || extractArtifactsFromToolResult(data.data);
      
      console.log('[artifact-tool-bridge] Extracted artifacts:', artifacts);
      
      if (!artifacts || artifacts.length === 0) {
        console.log('[artifact-tool-bridge] No artifacts found in tool result');
        return;
      }

      const { addArtifact, setShowArtifacts } = useAppState.getState();
      artifacts.forEach(a => {
        console.log('[artifact-tool-bridge] Adding artifact:', a.path);
        addArtifact(a);
      });
      setShowArtifacts(true);

      console.log(`[artifact-tool-bridge] ✅ Opened ${artifacts.length} artifact(s)`);
    }, ['response']);

    return unsubscribe;
  }, [subscribe]);
}
