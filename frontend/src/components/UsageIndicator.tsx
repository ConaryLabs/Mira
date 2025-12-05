// frontend/src/components/UsageIndicator.tsx
// Displays current pricing tier and usage information

import React from 'react';
import { useUsageStore } from '../stores/useUsageStore';

const formatTokens = (tokens: number): string => {
  if (tokens >= 1_000_000) {
    return `${(tokens / 1_000_000).toFixed(1)}M`;
  }
  if (tokens >= 1_000) {
    return `${(tokens / 1_000).toFixed(1)}K`;
  }
  return tokens.toString();
};

const formatCost = (cost: number): string => {
  if (cost < 0.01) {
    return `$${cost.toFixed(4)}`;
  }
  return `$${cost.toFixed(2)}`;
};

export const UsageIndicator: React.FC = () => {
  const {
    currentUsage,
    sessionTotalCost,
    sessionTotalTokensInput,
    sessionTotalTokensOutput,
    cacheHits,
    cacheMisses,
    currentWarning,
    warningDismissed,
    dismissWarning,
  } = useUsageStore();

  const cacheHitRate = cacheHits + cacheMisses > 0
    ? Math.round((cacheHits / (cacheHits + cacheMisses)) * 100)
    : 0;

  // Get warning styles based on level
  const getWarningStyles = () => {
    if (!currentWarning || warningDismissed) return null;

    switch (currentWarning.warningLevel) {
      case 'approaching':
        return 'bg-yellow-500/10 border-yellow-500/30 text-yellow-200';
      case 'near_threshold':
        return 'bg-orange-500/10 border-orange-500/30 text-orange-200';
      case 'over_threshold':
        return 'bg-red-500/10 border-red-500/30 text-red-200';
      default:
        return null;
    }
  };

  const warningStyles = getWarningStyles();

  return (
    <div className="flex flex-col gap-2 text-xs">
      {/* Current pricing tier indicator */}
      {currentUsage && (
        <div className="flex items-center gap-2">
          <span
            className={`px-2 py-0.5 rounded text-xs font-medium ${
              currentUsage.pricingTier === 'large_context'
                ? 'bg-purple-500/20 text-purple-300 border border-purple-500/30'
                : 'bg-green-500/20 text-green-300 border border-green-500/30'
            }`}
          >
            {currentUsage.pricingTier === 'large_context' ? 'Large Context' : 'Standard'}
          </span>
          {currentUsage.fromCache && (
            <span className="px-2 py-0.5 rounded bg-blue-500/20 text-blue-300 border border-blue-500/30">
              Cached
            </span>
          )}
        </div>
      )}

      {/* Session usage summary */}
      <div className="flex items-center gap-3 text-gray-400">
        <span title="Total session cost">
          {formatCost(sessionTotalCost)}
        </span>
        <span title="Input tokens">
          In: {formatTokens(sessionTotalTokensInput)}
        </span>
        <span title="Output tokens">
          Out: {formatTokens(sessionTotalTokensOutput)}
        </span>
        <span title="Cache hit rate" className="text-blue-400">
          Cache: {cacheHitRate}%
        </span>
      </div>

      {/* Context warning banner */}
      {warningStyles && currentWarning && (
        <div className={`flex items-center justify-between px-3 py-2 rounded border ${warningStyles}`}>
          <div className="flex items-center gap-2">
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
            <span>{currentWarning.message}</span>
            <span className="text-gray-400">
              ({formatTokens(currentWarning.tokensInput)} / {formatTokens(currentWarning.threshold)} tokens)
            </span>
          </div>
          <button
            onClick={dismissWarning}
            className="p-1 hover:bg-white/10 rounded"
            title="Dismiss warning"
          >
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
};

// Compact version for header/footer
export const UsageIndicatorCompact: React.FC = () => {
  const { currentUsage, sessionTotalCost, currentWarning, warningDismissed } = useUsageStore();

  const hasWarning = currentWarning && !warningDismissed;
  const isLargeContext = currentUsage?.pricingTier === 'large_context';

  return (
    <div className="flex items-center gap-2 text-xs">
      {/* Pricing tier badge */}
      {currentUsage && (
        <span
          className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
            isLargeContext
              ? 'bg-purple-500/20 text-purple-300'
              : 'bg-green-500/20 text-green-300'
          }`}
          title={isLargeContext ? 'Large context pricing (>200k tokens)' : 'Standard pricing (<200k tokens)'}
        >
          {isLargeContext ? 'LG' : 'STD'}
        </span>
      )}

      {/* Cost */}
      <span className="text-gray-400" title="Session cost">
        {formatCost(sessionTotalCost)}
      </span>

      {/* Warning indicator */}
      {hasWarning && (
        <span
          className={`w-2 h-2 rounded-full ${
            currentWarning?.warningLevel === 'over_threshold'
              ? 'bg-red-500 animate-pulse'
              : currentWarning?.warningLevel === 'near_threshold'
              ? 'bg-orange-500'
              : 'bg-yellow-500'
          }`}
          title={currentWarning?.message}
        />
      )}
    </div>
  );
};
