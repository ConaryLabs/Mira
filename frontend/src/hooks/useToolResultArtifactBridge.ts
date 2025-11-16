// src/hooks/useToolResultArtifactBridge.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';
import { extractArtifacts } from '../utils/artifact';

export function useToolResultArtifactBridge() {
  const subscribe = useWebSocketStore(state => state.subscribe);

  useEffect(() => {
    const unsubscribe = subscribe('artifact-tool-bridge', (message) => {
      console.log('[artifact-tool-bridge] Received message:', {
        type: message.type,
        dataType: message.data?.type,
      });

      if (message.type !== 'response') return;

      const data = message.data || message;
      if (!data) {
        console.log('[artifact-tool-bridge] No data in message');
        return;
      }

      // Check if this is a tool result
      const dtype = data.type || data.data?.type;
      const toolName = data.tool_name || data.tool || data.data?.tool_name;
      const isToolResult = dtype === 'tool_result' || dtype === 'tool' || !!toolName;

      if (!isToolResult) {
        console.log('[artifact-tool-bridge] Not a tool result, ignoring');
        return;
      }

      // Try extracting artifacts from data or nested data.data
      let artifacts = extractArtifacts(data);
      if (artifacts.length === 0 && data.data) {
        artifacts = extractArtifacts(data.data);
      }

      if (artifacts.length === 0) {
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
