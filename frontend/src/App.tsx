// src/App.tsx
import React, { useEffect } from 'react';
import { Header } from './components/Header';
import { ChatArea } from './components/ChatArea';
import { ArtifactPanel } from './components/ArtifactPanel';
import { QuickFileOpen, useQuickFileOpen } from './components/QuickFileOpen';
import { ProjectsView } from './components/ProjectsView';
import { ToastContainer } from './components/ToastContainer';
import { useAppState } from './stores/useAppState';
import { useWebSocketStore } from './stores/useWebSocketStore';
import { useUIStore, useActiveTab } from './stores/useUIStore';
import { useWebSocketMessageHandler } from './hooks/useWebSocketMessageHandler';
import { useMessageHandler } from './hooks/useMessageHandler';
import { useChatPersistence } from './hooks/useChatPersistence';
import { useArtifactFileContentWire } from './hooks/useArtifactFileContentWire';
import { useToolResultArtifactBridge } from './hooks/useToolResultArtifactBridge';
import { useErrorHandler } from './hooks/useErrorHandler';
import { useConnectionTracking } from './hooks/useConnectionTracking';
import { MessageSquare, Folder } from 'lucide-react';
import './App.css';

function App() {
  const { showArtifacts } = useAppState();
  // PERFORMANCE FIX: Use optimized selector to avoid re-renders on input changes
  const activeTab = useActiveTab();
  const setActiveTab = useUIStore(state => state.setActiveTab);
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
  useConnectionTracking();       // NEW: Sync WebSocket state → AppState connection tracking
  
  // Quick file open handler
  const quickFileOpen = useQuickFileOpen();
  
  const tabs = [
    { id: 'chat' as const, label: 'Chat', icon: <MessageSquare className="w-4 h-4" /> },
    { id: 'projects' as const, label: 'Projects', icon: <Folder className="w-4 h-4" /> },
  ];
  
  return (
    <div className="h-screen flex flex-col bg-slate-900 text-slate-100">
      <Header onQuickFileOpen={quickFileOpen.open} />
      
      {/* Tab Navigation */}
      <div className="border-b border-gray-800 bg-gray-900">
        <div className="flex items-center gap-1 px-4">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`
                flex items-center gap-2 px-4 py-3 border-b-2 transition-colors
                ${activeTab === tab.id
                  ? 'border-blue-500 text-blue-400 bg-gray-800'
                  : 'border-transparent text-gray-400 hover:text-gray-300 hover:bg-gray-800'
                }
              `}
            >
              {tab.icon}
              <span className="text-sm font-medium">{tab.label}</span>
            </button>
          ))}
        </div>
      </div>
      
      {/* Main content area */}
      <div className="flex-1 flex overflow-hidden">
        {activeTab === 'chat' && (
          <>
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
              <div className="w-1/2 border-l border-slate-700">
                <ArtifactPanel />
              </div>
            )}
          </>
        )}
        
        {activeTab === 'projects' && <ProjectsView />}
      </div>
      
      <QuickFileOpen
        isOpen={quickFileOpen.isOpen}
        onClose={quickFileOpen.close}
      />
      
      {/* Toast notifications - bottom right corner */}
      <ToastContainer />
    </div>
  );
}

export default App;
