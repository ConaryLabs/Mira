// src/config/app.ts
// Centralized application configuration

/**
 * Session configuration
 * 
 * For development: Uses "peter-eternal" as the default eternal session
 * For production: Should be replaced with actual user session management
 */
export const APP_CONFIG = {
  // Session ID - can be overridden by environment variable
  SESSION_ID: import.meta.env.VITE_SESSION_ID || 'peter-eternal',
  
  // WebSocket configuration
  WS_URL: import.meta.env.VITE_WS_URL || 'ws://localhost:3001',
  
  // API configuration
  API_URL: import.meta.env.VITE_API_URL || 'http://localhost:3001',
  
  // Feature flags
  ENABLE_AUTH: import.meta.env.VITE_ENABLE_AUTH === 'true',
  ENABLE_MULTI_USER: import.meta.env.VITE_ENABLE_MULTI_USER === 'true',
} as const;

/**
 * Get the current session ID
 * In future, this can pull from auth store or JWT token
 */
export function getSessionId(): string {
  // TODO: When auth is implemented, pull from auth store
  // const authStore = useAuthStore.getState();
  // return authStore.user?.id || APP_CONFIG.SESSION_ID;
  
  return APP_CONFIG.SESSION_ID;
}

/**
 * Get current user info
 * In future, this pulls from actual auth
 */
export function getCurrentUser() {
  // TODO: Implement real auth
  return {
    id: APP_CONFIG.SESSION_ID,
    username: 'peter',
    displayName: 'Peter',
  };
}
