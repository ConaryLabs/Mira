// src/hooks/useConnectionTracking.ts
// Syncs WebSocketStore connection state with AppState connection tracking

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

export const useConnectionTracking = () => {
  const connectionState = useWebSocketStore(state => state.connectionState);
  const reconnectAttempts = useWebSocketStore(state => state.reconnectAttempts);
  const maxReconnectAttempts = useWebSocketStore(state => state.maxReconnectAttempts);
  
  const {
    setReconnecting,
    setReconnectAttempts,
    setConnectionStatus,
    setConnectionError,
  } = useAppState();

  useEffect(() => {
    // Update AppState based on WebSocketStore state
    switch (connectionState) {
      case 'connecting':
        setReconnecting(false);
        setReconnectAttempts(0);
        setConnectionStatus('Connecting to Mira...');
        setConnectionError(null);
        break;
        
      case 'reconnecting':
        setReconnecting(true);
        setReconnectAttempts(reconnectAttempts);
        setConnectionStatus(
          `Reconnecting... (attempt ${reconnectAttempts}/${maxReconnectAttempts})`
        );
        setConnectionError(null);
        break;
        
      case 'connected':
        setReconnecting(false);
        setReconnectAttempts(0);
        setConnectionStatus('Connected');
        setConnectionError(null);
        break;
        
      case 'disconnected':
        setReconnecting(false);
        setConnectionStatus('Disconnected from Mira');
        if (reconnectAttempts >= maxReconnectAttempts) {
          setConnectionError('Max reconnection attempts reached');
        }
        break;
        
      case 'error':
        setReconnecting(false);
        setConnectionStatus('Connection error');
        setConnectionError('Failed to connect to server');
        break;
    }
  }, [
    connectionState,
    reconnectAttempts,
    maxReconnectAttempts,
    setReconnecting,
    setReconnectAttempts,
    setConnectionStatus,
    setConnectionError,
  ]);
};
