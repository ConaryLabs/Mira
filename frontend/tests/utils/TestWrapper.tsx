// tests/utils/TestWrapper.tsx
// Wrapper that activates error handling and connection tracking for integration tests

import React, { FC, PropsWithChildren } from 'react';
import { useErrorHandler } from '../../src/hooks/useErrorHandler';
import { useConnectionTracking } from '../../src/hooks/useConnectionTracking';

/**
 * Test wrapper that activates hooks needed for integration tests.
 * 
 * Usage:
 * ```typescript
 * const { result } = renderHook(
 *   () => ({
 *     ws: useWebSocketStore(),
 *     chat: useChatStore(),
 *     app: useAppState(),
 *   }),
 *   { wrapper: TestWrapper }
 * );
 * ```
 */
export const TestWrapper: FC<PropsWithChildren> = ({ children }) => {
  // Activate error handling (WebSocket errors → chat messages + toasts)
  useErrorHandler();
  
  // Activate connection tracking (WebSocket state → AppState sync)
  useConnectionTracking();
  
  return <>{children}</>;
};

/**
 * Minimal wrapper for tests that only need error handling
 */
export const ErrorHandlerWrapper: FC<PropsWithChildren> = ({ children }) => {
  useErrorHandler();
  return <>{children}</>;
};

/**
 * Minimal wrapper for tests that only need connection tracking
 */
export const ConnectionTrackingWrapper: FC<PropsWithChildren> = ({ children }) => {
  useConnectionTracking();
  return <>{children}</>;
};
