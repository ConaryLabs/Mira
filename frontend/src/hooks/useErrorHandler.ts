// src/hooks/useErrorHandler.ts
// Converts WebSocket errors into chat messages and toast notifications

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';

export const useErrorHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const addMessage = useChatStore(state => state.addMessage);
  const { addToast, setCanSendMessage, setRateLimitUntil } = useAppState();

  useEffect(() => {
    const unsubscribe = subscribe(
      'error-handler',
      (message) => {
        if (message.type !== 'error') return;
        
        const error = message.error || {};
        const errorMessage = error.message || message.message || 'An unknown error occurred';
        const statusCode = error.statusCode || error.code;
        
        console.error('[ErrorHandler] Received error:', errorMessage, statusCode);
        
        // Add error message to chat
        addMessage({
          id: `error-${Date.now()}`,
          role: 'error',
          content: errorMessage.includes('error') 
            ? errorMessage 
            : `Error: ${errorMessage}`,
          timestamp: Date.now(),
        });
        
        // Handle specific error types
        if (statusCode === 429 || error.code === 'RATE_LIMIT_EXCEEDED') {
          // Rate limiting
          const retryAfter = error.retryAfter || 60; // Default 60 seconds
          
          addToast({
            type: 'warning',
            message: 'Too many requests. Please slow down.',
            duration: retryAfter * 1000,
          });
          
          setCanSendMessage(false);
          setRateLimitUntil(Date.now() + (retryAfter * 1000));
          
          console.warn(`[ErrorHandler] Rate limited for ${retryAfter} seconds`);
          
        } else if (statusCode >= 500 || error.code === 'INTERNAL_SERVER_ERROR') {
          // Server errors (5xx)
          addToast({
            type: 'error',
            message: 'Server error. Please try again.',
            duration: 5000,
          });
          
        } else if (statusCode === 400 || error.code === 'BAD_REQUEST') {
          // Client errors (4xx)
          addToast({
            type: 'warning',
            message: errorMessage,
            duration: 4000,
          });
          
        } else {
          // Generic error
          addToast({
            type: 'error',
            message: errorMessage,
            duration: 5000,
          });
        }
      },
      ['error']
    );

    return unsubscribe;
  }, [subscribe, addMessage, addToast, setCanSendMessage, setRateLimitUntil]);
};
