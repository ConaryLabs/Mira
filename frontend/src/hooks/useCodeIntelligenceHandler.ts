// frontend/src/hooks/useCodeIntelligenceHandler.ts
// Hook to handle WebSocket responses for code intelligence features

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useCodeIntelligenceStore, CodeSearchResult } from '../stores/useCodeIntelligenceStore';

export function useCodeIntelligenceHandler() {
  const subscribe = useWebSocketStore((state) => state.subscribe);

  const setBudget = useCodeIntelligenceStore((state) => state.setBudget);
  const setBudgetError = useCodeIntelligenceStore((state) => state.setBudgetError);
  const setBudgetLoading = useCodeIntelligenceStore((state) => state.setBudgetLoading);

  const setSearchResults = useCodeIntelligenceStore((state) => state.setSearchResults);
  const setSearchError = useCodeIntelligenceStore((state) => state.setSearchError);
  const setSearching = useCodeIntelligenceStore((state) => state.setSearching);

  const setCoChangeSuggestions = useCodeIntelligenceStore((state) => state.setCoChangeSuggestions);
  const setLoadingCoChange = useCodeIntelligenceStore((state) => state.setLoadingCoChange);

  const setExpertise = useCodeIntelligenceStore((state) => state.setExpertise);
  const setLoadingExpertise = useCodeIntelligenceStore((state) => state.setLoadingExpertise);

  useEffect(() => {
    const unsubscribe = subscribe(
      'code-intelligence-handler',
      (message) => {
        // Handle data messages
        if (message.type === 'data' && message.data) {
          const dataType = message.data.type;

          switch (dataType) {
            case 'budget_status':
              setBudgetLoading(false);
              setBudget({
                dailyUsagePercent: message.data.daily_usage_percent,
                monthlyUsagePercent: message.data.monthly_usage_percent,
                dailySpentUsd: message.data.daily_spent_usd,
                dailyLimitUsd: message.data.daily_limit_usd,
                monthlySpentUsd: message.data.monthly_spent_usd,
                monthlyLimitUsd: message.data.monthly_limit_usd,
                dailyRemaining: message.data.daily_remaining,
                monthlyRemaining: message.data.monthly_remaining,
                isCritical: message.data.is_critical,
                isLow: message.data.is_low,
                lastUpdated: message.data.last_updated,
              });
              break;

            case 'semantic_search_results':
              setSearching(false);
              const searchResults: CodeSearchResult[] = (message.data.results || []).map((r: any) => ({
                id: r.id || String(Math.random()),
                filePath: r.file_path || '',
                content: r.content || '',
                score: r.score || 0,
                lineStart: r.line_start || 0,
                lineEnd: r.line_end || 0,
                language: r.language,
                highlights: r.highlights,
              }));
              setSearchResults(searchResults);
              break;

            case 'cochange_suggestions':
              setLoadingCoChange(false);
              const suggestions = (message.data.suggestions || []).map((s: any) => ({
                filePath: s.file_path,
                confidence: s.confidence,
                reason: s.reason,
                coChangeCount: s.co_change_count,
                lastChanged: s.last_changed,
              }));
              setCoChangeSuggestions(suggestions);
              break;

            case 'expertise_results':
              setLoadingExpertise(false);
              const experts = (message.data.experts || []).map((e: any) => ({
                author: e.author,
                email: e.email,
                filesOwned: e.files_owned || 0,
                totalCommits: e.total_commits,
                recentActivity: e.recent_activity || e.last_active,
                expertiseAreas: e.expertise_areas || [],
              }));
              setExpertise(experts);
              break;
          }
        }

        // Handle error messages
        if (message.type === 'error') {
          const errorMsg = message.message || 'Unknown error';
          // Check if it's related to our requests
          if (errorMsg.includes('budget')) {
            setBudgetError(errorMsg);
          } else if (errorMsg.includes('search') || errorMsg.includes('semantic')) {
            setSearchError(errorMsg);
          }
        }
      },
      ['data', 'error']
    );

    return () => {
      unsubscribe();
    };
  }, [
    subscribe,
    setBudget,
    setBudgetError,
    setBudgetLoading,
    setSearchResults,
    setSearchError,
    setSearching,
    setCoChangeSuggestions,
    setLoadingCoChange,
    setExpertise,
    setLoadingExpertise,
  ]);
}
