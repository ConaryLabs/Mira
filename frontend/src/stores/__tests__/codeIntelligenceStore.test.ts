// frontend/src/stores/__tests__/codeIntelligenceStore.test.ts
// Code Intelligence Store Tests

import { describe, it, expect, beforeEach } from 'vitest';
import { useCodeIntelligenceStore, BudgetStatus, CodeSearchResult, CoChangeSuggestion, BuildError } from '../useCodeIntelligenceStore';

describe('useCodeIntelligenceStore', () => {
  beforeEach(() => {
    // Reset store state before each test
    useCodeIntelligenceStore.getState().reset();
  });

  describe('initial state', () => {
    it('should have correct initial values', () => {
      const state = useCodeIntelligenceStore.getState();

      expect(state.budget).toBeNull();
      expect(state.isBudgetLoading).toBe(false);
      expect(state.budgetError).toBeNull();
      expect(state.codeSearch.query).toBe('');
      expect(state.codeSearch.results).toEqual([]);
      expect(state.coChangeSuggestions).toEqual([]);
      expect(state.isPanelVisible).toBe(false);
      expect(state.activeTab).toBe('budget');
    });
  });

  describe('budget actions', () => {
    it('should set budget', () => {
      const mockBudget: BudgetStatus = {
        dailyUsagePercent: 25,
        monthlyUsagePercent: 10,
        dailySpentUsd: 1.25,
        dailyLimitUsd: 5.0,
        monthlySpentUsd: 15.0,
        monthlyLimitUsd: 150.0,
        dailyRemaining: 3.75,
        monthlyRemaining: 135.0,
        isCritical: false,
        isLow: false,
        lastUpdated: Date.now(),
      };

      useCodeIntelligenceStore.getState().setBudget(mockBudget);

      expect(useCodeIntelligenceStore.getState().budget).toEqual(mockBudget);
      expect(useCodeIntelligenceStore.getState().budgetError).toBeNull();
    });

    it('should set budget loading state', () => {
      useCodeIntelligenceStore.getState().setBudgetLoading(true);
      expect(useCodeIntelligenceStore.getState().isBudgetLoading).toBe(true);

      useCodeIntelligenceStore.getState().setBudgetLoading(false);
      expect(useCodeIntelligenceStore.getState().isBudgetLoading).toBe(false);
    });

    it('should set budget error and clear loading', () => {
      useCodeIntelligenceStore.setState({ isBudgetLoading: true });

      useCodeIntelligenceStore.getState().setBudgetError('API request failed');

      expect(useCodeIntelligenceStore.getState().budgetError).toBe('API request failed');
      expect(useCodeIntelligenceStore.getState().isBudgetLoading).toBe(false);
    });

    it('should set loading when refreshBudget is called', () => {
      useCodeIntelligenceStore.getState().refreshBudget();
      expect(useCodeIntelligenceStore.getState().isBudgetLoading).toBe(true);
    });
  });

  describe('code search actions', () => {
    it('should set search query', () => {
      useCodeIntelligenceStore.getState().setSearchQuery('authentication');

      expect(useCodeIntelligenceStore.getState().codeSearch.query).toBe('authentication');
    });

    it('should set search results and update lastSearchTime', () => {
      const mockResults: CodeSearchResult[] = [
        {
          id: 'result-1',
          filePath: 'src/auth/login.ts',
          content: 'function login() {}',
          score: 0.95,
          lineStart: 10,
          lineEnd: 20,
          language: 'typescript',
        },
        {
          id: 'result-2',
          filePath: 'src/auth/logout.ts',
          content: 'function logout() {}',
          score: 0.85,
          lineStart: 5,
          lineEnd: 15,
          language: 'typescript',
        },
      ];

      const beforeTime = Date.now();
      useCodeIntelligenceStore.getState().setSearchResults(mockResults);
      const afterTime = Date.now();

      const state = useCodeIntelligenceStore.getState();
      expect(state.codeSearch.results).toEqual(mockResults);
      expect(state.codeSearch.isSearching).toBe(false);
      expect(state.codeSearch.lastSearchTime).toBeGreaterThanOrEqual(beforeTime);
      expect(state.codeSearch.lastSearchTime).toBeLessThanOrEqual(afterTime);
    });

    it('should set searching state', () => {
      useCodeIntelligenceStore.getState().setSearching(true);
      expect(useCodeIntelligenceStore.getState().codeSearch.isSearching).toBe(true);

      useCodeIntelligenceStore.getState().setSearching(false);
      expect(useCodeIntelligenceStore.getState().codeSearch.isSearching).toBe(false);
    });

    it('should set search error and clear searching state', () => {
      useCodeIntelligenceStore.setState({
        codeSearch: { ...useCodeIntelligenceStore.getState().codeSearch, isSearching: true },
      });

      useCodeIntelligenceStore.getState().setSearchError('Search failed');

      const state = useCodeIntelligenceStore.getState();
      expect(state.codeSearch.error).toBe('Search failed');
      expect(state.codeSearch.isSearching).toBe(false);
    });

    it('should clear search', () => {
      // Set some search state
      useCodeIntelligenceStore.setState({
        codeSearch: {
          query: 'test query',
          results: [{ id: '1', filePath: 'test.ts', content: 'test', score: 1, lineStart: 1, lineEnd: 1 }],
          isSearching: true,
          error: 'some error',
          lastSearchTime: Date.now(),
        },
      });

      useCodeIntelligenceStore.getState().clearSearch();

      const state = useCodeIntelligenceStore.getState();
      expect(state.codeSearch.query).toBe('');
      expect(state.codeSearch.results).toEqual([]);
      expect(state.codeSearch.isSearching).toBe(false);
      expect(state.codeSearch.error).toBeNull();
    });
  });

  describe('co-change actions', () => {
    it('should set co-change suggestions and clear loading', () => {
      const mockSuggestions: CoChangeSuggestion[] = [
        {
          filePath: 'src/utils/helper.ts',
          confidence: 0.8,
          reason: 'Often changed together',
          coChangeCount: 15,
          lastChanged: '2025-01-01',
        },
      ];

      useCodeIntelligenceStore.setState({ isLoadingCoChange: true });
      useCodeIntelligenceStore.getState().setCoChangeSuggestions(mockSuggestions);

      expect(useCodeIntelligenceStore.getState().coChangeSuggestions).toEqual(mockSuggestions);
      expect(useCodeIntelligenceStore.getState().isLoadingCoChange).toBe(false);
    });

    it('should set current file', () => {
      useCodeIntelligenceStore.getState().setCurrentFile('src/main.ts');
      expect(useCodeIntelligenceStore.getState().currentFile).toBe('src/main.ts');

      useCodeIntelligenceStore.getState().setCurrentFile(null);
      expect(useCodeIntelligenceStore.getState().currentFile).toBeNull();
    });
  });

  describe('build error actions', () => {
    it('should set build errors', () => {
      const mockErrors: BuildError[] = [
        {
          id: 'error-1',
          errorType: 'TypeError',
          message: 'Cannot read property of undefined',
          filePath: 'src/app.ts',
          line: 42,
          column: 10,
          severity: 'error',
          timestamp: Date.now(),
        },
      ];

      useCodeIntelligenceStore.getState().setBuildErrors(mockErrors);

      expect(useCodeIntelligenceStore.getState().buildErrors).toEqual(mockErrors);
      expect(useCodeIntelligenceStore.getState().isLoadingBuildErrors).toBe(false);
    });

    it('should add build error', () => {
      const existingError: BuildError = {
        id: 'error-1',
        errorType: 'TypeError',
        message: 'First error',
        severity: 'error',
        timestamp: Date.now(),
      };
      useCodeIntelligenceStore.setState({ buildErrors: [existingError] });

      const newError: BuildError = {
        id: 'error-2',
        errorType: 'SyntaxError',
        message: 'Second error',
        severity: 'warning',
        timestamp: Date.now(),
      };

      useCodeIntelligenceStore.getState().addBuildError(newError);

      const errors = useCodeIntelligenceStore.getState().buildErrors;
      expect(errors).toHaveLength(2);
      expect(errors[0]).toEqual(existingError);
      expect(errors[1]).toEqual(newError);
    });

    it('should clear build errors', () => {
      useCodeIntelligenceStore.setState({
        buildErrors: [
          { id: 'error-1', errorType: 'Error', message: 'test', severity: 'error', timestamp: Date.now() },
        ],
      });

      useCodeIntelligenceStore.getState().clearBuildErrors();

      expect(useCodeIntelligenceStore.getState().buildErrors).toEqual([]);
    });
  });

  describe('panel actions', () => {
    it('should toggle panel visibility', () => {
      expect(useCodeIntelligenceStore.getState().isPanelVisible).toBe(false);

      useCodeIntelligenceStore.getState().togglePanel();
      expect(useCodeIntelligenceStore.getState().isPanelVisible).toBe(true);

      useCodeIntelligenceStore.getState().togglePanel();
      expect(useCodeIntelligenceStore.getState().isPanelVisible).toBe(false);
    });

    it('should show panel', () => {
      useCodeIntelligenceStore.getState().showPanel();
      expect(useCodeIntelligenceStore.getState().isPanelVisible).toBe(true);
    });

    it('should hide panel', () => {
      useCodeIntelligenceStore.setState({ isPanelVisible: true });

      useCodeIntelligenceStore.getState().hidePanel();
      expect(useCodeIntelligenceStore.getState().isPanelVisible).toBe(false);
    });

    it('should set active tab', () => {
      useCodeIntelligenceStore.getState().setActiveTab('search');
      expect(useCodeIntelligenceStore.getState().activeTab).toBe('search');

      useCodeIntelligenceStore.getState().setActiveTab('cochange');
      expect(useCodeIntelligenceStore.getState().activeTab).toBe('cochange');

      useCodeIntelligenceStore.getState().setActiveTab('builds');
      expect(useCodeIntelligenceStore.getState().activeTab).toBe('builds');
    });
  });

  describe('reset', () => {
    it('should reset all state to initial values', () => {
      // Set various state values
      useCodeIntelligenceStore.setState({
        budget: {
          dailyUsagePercent: 50,
          monthlyUsagePercent: 30,
          dailySpentUsd: 2.5,
          dailyLimitUsd: 5.0,
          monthlySpentUsd: 45.0,
          monthlyLimitUsd: 150.0,
          dailyRemaining: 2.5,
          monthlyRemaining: 105.0,
          isCritical: false,
          isLow: false,
          lastUpdated: Date.now(),
        },
        isBudgetLoading: true,
        isPanelVisible: true,
        activeTab: 'search',
        buildErrors: [{ id: 'e1', errorType: 'Error', message: 'test', severity: 'error', timestamp: Date.now() }],
      });

      useCodeIntelligenceStore.getState().reset();

      const state = useCodeIntelligenceStore.getState();
      expect(state.budget).toBeNull();
      expect(state.isBudgetLoading).toBe(false);
      expect(state.isPanelVisible).toBe(false);
      expect(state.activeTab).toBe('budget');
      expect(state.buildErrors).toEqual([]);
    });
  });
});
