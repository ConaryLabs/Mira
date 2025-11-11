// src/components/ConnectionBanner.tsx
// FIXED: Works with or without AppState sync (backward compatible)

import React from 'react';
import { Wifi, WifiOff, AlertCircle } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

export const ConnectionBanner: React.FC = () => {
  const connectionState = useWebSocketStore(state => state.connectionState);
  
  // Optional: Enhanced status from AppState (if useConnectionTracking is active)
  const { reconnectAttempts, connectionStatus, connectionError } = useAppState();
  
  if (connectionState === 'connected') return null;
  
  // Determine state for styling and messaging
  const isConnecting = connectionState === 'connecting';
  const isReconnecting = connectionState === 'reconnecting';
  const isDisconnected = connectionState === 'disconnected' || connectionState === 'error';
  
  // Build message - gracefully use AppState enhancements if available
  const getMessage = () => {
    // If we have a custom connection status from AppState, use it
    if (connectionStatus && connectionStatus !== 'disconnected' && connectionStatus !== 'Connected') {
      return connectionStatus;
    }
    
    // Otherwise use state-based defaults
    if (isConnecting) return 'Connecting to Mira...';
    
    if (isReconnecting) {
      // Show attempt count if available from AppState
      if (reconnectAttempts > 0) {
        return `Reconnecting to Mira... (attempt ${reconnectAttempts})`;
      }
      return 'Reconnecting to Mira...';
    }
    
    // Disconnected state
    if (connectionError) {
      return `Disconnected: ${connectionError}`;
    }
    return 'Disconnected from Mira';
  };
  
  return (
    <div className={`
      flex items-center gap-2 px-4 py-2 text-sm font-medium
      ${isConnecting || isReconnecting
        ? 'bg-yellow-500/10 text-yellow-400 border-b border-yellow-500/20' 
        : 'bg-red-500/10 text-red-400 border-b border-red-500/20'}
    `}>
      {isConnecting ? (
        <Wifi className="w-4 h-4 animate-pulse" />
      ) : isReconnecting ? (
        <Wifi className="w-4 h-4 animate-pulse" />
      ) : connectionError ? (
        <AlertCircle className="w-4 h-4" />
      ) : (
        <WifiOff className="w-4 h-4" />
      )}
      <span>{getMessage()}</span>
    </div>
  );
};
