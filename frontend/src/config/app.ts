// src/config/app.ts
// Centralized application configuration

export const APP_CONFIG = {
  // WebSocket configuration
  WS_URL: import.meta.env.VITE_WS_URL || 'ws://localhost:3001/ws',

  // API configuration
  API_URL: import.meta.env.VITE_API_URL || 'http://localhost:3001',

  // Feature flags
  ENABLE_AUTH: import.meta.env.VITE_ENABLE_AUTH !== 'false', // Auth enabled by default
  ENABLE_MULTI_USER: import.meta.env.VITE_ENABLE_MULTI_USER !== 'false', // Multi-user enabled by default
} as const;
