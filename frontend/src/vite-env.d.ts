/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_WS_URL: string
  readonly VITE_API_URL: string
  readonly VITE_DEV_MODE: string
  readonly VITE_LOG_LEVEL: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

// Augment Window interface for debug tracking
interface Window {
  __missingTerminalSessions?: Set<string>;
}
