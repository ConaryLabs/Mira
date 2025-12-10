// src/Home.tsx
import { useEffect } from 'react';
import { Header } from './components/Header';
import { ChatArea } from './components/ChatArea';
import { ArtifactPanel } from './components/ArtifactPanel';
import { ActivityPanel } from './components/ActivityPanel';
import { IntelligencePanel } from './components/IntelligencePanel';
import { BackgroundAgentsPanel } from './components/BackgroundAgentsPanel';
import { ReviewPanel } from './components/ReviewPanel';
import { ToastContainer } from './components/ToastContainer';
import { useAppState } from './stores/useAppState';
import { useWebSocketStore } from './stores/useWebSocketStore';
import { useCodeIntelligenceStore } from './stores/useCodeIntelligenceStore';
import { useWebSocketMessageHandler } from './hooks/useWebSocketMessageHandler';
import { useMessageHandler } from './hooks/useMessageHandler';
import { useChatPersistence } from './hooks/useChatPersistence';
import { useArtifactFileContentWire } from './hooks/useArtifactFileContentWire';
import { useToolResultArtifactBridge } from './hooks/useToolResultArtifactBridge';
import { useErrorHandler } from './hooks/useErrorHandler';
import { useConnectionTracking } from './hooks/useConnectionTracking';
import { useCodeIntelligenceHandler } from './hooks/useCodeIntelligenceHandler';

export function Home() {
  const { showArtifacts } = useAppState();
  const isIntelligenceVisible = useCodeIntelligenceStore(state => state.isPanelVisible);
  const connect = useWebSocketStore(state => state.connect);
  const disconnect = useWebSocketStore(state => state.disconnect);
  const connectionState = useWebSocketStore(state => state.connectionState);

  // Initialize WebSocket connection
  useEffect(() => {
    connect();

    // Cleanup on unmount
    return () => {
      disconnect();
    };
  }, [connect, disconnect]);

  // Handle all WebSocket messages
  useWebSocketMessageHandler(); // Handles data messages (projects, files, git)
  useMessageHandler();           // Handles response messages (chat)
  useChatPersistence(connectionState); // Handles chat history loading from backend + localStorage
  useArtifactFileContentWire();  // Belt-and-suspenders: ensure file_content opens artifacts
  useToolResultArtifactBridge(); // Tool_result → Artifact Viewer bridge
  useErrorHandler();             // WebSocket error → Chat messages + Toasts
  useConnectionTracking();       // Sync WebSocket state → AppState connection tracking
  useCodeIntelligenceHandler();  // Code intelligence WebSocket responses

  return (
    <div className="h-screen flex flex-col bg-gray-50 dark:bg-slate-900 text-gray-900 dark:text-slate-100">
      <Header />

      {/* Main content area */}
      <div className="flex-1 flex overflow-hidden">
        {/* Main content (chat area) */}
        <div className="flex-1 flex overflow-hidden">
          {/* Chat Area - Centered when no artifacts, 50% when artifacts shown */}
          <div className={`
            min-w-0 flex overflow-hidden transition-all duration-300
            ${showArtifacts ? 'w-1/2' : 'flex-1'}
          `}>
            <div className={`
              flex flex-col w-full
              ${!showArtifacts ? 'max-w-4xl mx-auto' : ''}
            `}>
              <ChatArea />
            </div>
          </div>

          {/* Artifact Panel - Slides in from right */}
          {showArtifacts && (
            <div className="w-1/2 border-l border-gray-200 dark:border-slate-700">
              <ArtifactPanel />
            </div>
          )}
        </div>

        {/* Intelligence panel - right side (before Activity) */}
        {isIntelligenceVisible && <IntelligencePanel />}

        {/* Activity panel - right side */}
        <ActivityPanel />
      </div>

      {/* Toast notifications - bottom right corner */}
      <ToastContainer />

      {/* Background Agents Panel - floating bottom right */}
      <BackgroundAgentsPanel />

      {/* Review Panel - modal overlay */}
      <ReviewPanel />
    </div>
  );
}
