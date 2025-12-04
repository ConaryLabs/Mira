// frontend/src/components/BudgetTracker.tsx
// Budget tracking component showing daily/monthly usage and limits

import React, { useEffect, useCallback } from 'react';
import { DollarSign, TrendingUp, AlertTriangle, AlertCircle, RefreshCw } from 'lucide-react';
import { useBudgetStatus, useCodeIntelligenceStore } from '../stores/useCodeIntelligenceStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAuthStore } from '../stores/useAuthStore';

interface ProgressBarProps {
  value: number;
  max: number;
  label: string;
  showPercentage?: boolean;
  colorClass?: string;
}

function ProgressBar({ value, max, label, showPercentage = true, colorClass }: ProgressBarProps) {
  const percentage = max > 0 ? Math.min((value / max) * 100, 100) : 0;

  // Determine color based on percentage
  const getColorClass = () => {
    if (colorClass) return colorClass;
    if (percentage >= 90) return 'bg-red-500';
    if (percentage >= 70) return 'bg-yellow-500';
    return 'bg-green-500';
  };

  return (
    <div className="space-y-1">
      <div className="flex justify-between text-xs">
        <span className="text-gray-500 dark:text-slate-400">{label}</span>
        {showPercentage && (
          <span className="text-gray-700 dark:text-slate-300">{percentage.toFixed(1)}%</span>
        )}
      </div>
      <div className="h-2 bg-gray-200 dark:bg-slate-700 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all duration-300 ${getColorClass()}`}
          style={{ width: `${percentage}%` }}
        />
      </div>
    </div>
  );
}

interface StatCardProps {
  label: string;
  value: string;
  subValue?: string;
  icon: React.ReactNode;
  variant?: 'default' | 'warning' | 'danger';
}

function StatCard({ label, value, subValue, icon, variant = 'default' }: StatCardProps) {
  const getBgClass = () => {
    switch (variant) {
      case 'danger': return 'bg-red-100 dark:bg-red-900/30 border-red-300 dark:border-red-700/50';
      case 'warning': return 'bg-yellow-100 dark:bg-yellow-900/30 border-yellow-300 dark:border-yellow-700/50';
      default: return 'bg-gray-100 dark:bg-slate-800/50 border-gray-200 dark:border-slate-700/50';
    }
  };

  return (
    <div className={`p-3 rounded-lg border ${getBgClass()}`}>
      <div className="flex items-center gap-2 mb-1">
        {icon}
        <span className="text-xs text-gray-500 dark:text-slate-400">{label}</span>
      </div>
      <div className="text-lg font-semibold text-gray-900 dark:text-slate-100">{value}</div>
      {subValue && (
        <div className="text-xs text-gray-500 dark:text-slate-500">{subValue}</div>
      )}
    </div>
  );
}

export function BudgetTracker() {
  const { budget, isLoading, error } = useBudgetStatus();
  const setBudgetLoading = useCodeIntelligenceStore((state) => state.setBudgetLoading);
  const send = useWebSocketStore((state) => state.send);
  const user = useAuthStore((state) => state.user);

  const requestBudgetStatus = useCallback(async () => {
    if (!user?.id) return;

    setBudgetLoading(true);
    try {
      await send({
        type: 'code_intelligence_command',
        method: 'code.budget_status',
        params: {
          user_id: user.id,
        },
      });
    } catch (err) {
      console.error('Failed to request budget status:', err);
    }
  }, [send, user?.id, setBudgetLoading]);

  // Request budget status on mount
  useEffect(() => {
    requestBudgetStatus();
  }, [requestBudgetStatus]);

  if (isLoading) {
    return (
      <div className="p-4 space-y-4">
        <div className="flex items-center gap-2 text-gray-500 dark:text-slate-400">
          <DollarSign className="w-4 h-4 animate-pulse" />
          <span className="text-sm">Loading budget data...</span>
        </div>
        <div className="space-y-3">
          <div className="h-8 bg-gray-200 dark:bg-slate-800 rounded animate-pulse" />
          <div className="h-8 bg-gray-200 dark:bg-slate-800 rounded animate-pulse" />
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4">
        <div className="flex items-center gap-2 text-red-600 dark:text-red-400">
          <AlertCircle className="w-4 h-4" />
          <span className="text-sm">Failed to load budget: {error}</span>
        </div>
      </div>
    );
  }

  if (!budget) {
    return (
      <div className="p-4">
        <div className="flex items-center gap-2 text-gray-500 dark:text-slate-400">
          <DollarSign className="w-4 h-4" />
          <span className="text-sm">No budget data available</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 space-y-4">
      {/* Header with refresh */}
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-gray-700 dark:text-slate-300">Budget Overview</span>
        <button
          onClick={requestBudgetStatus}
          disabled={isLoading}
          className="p-1 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors disabled:opacity-50"
          title="Refresh budget status"
        >
          <RefreshCw className={`w-4 h-4 text-gray-500 dark:text-slate-400 ${isLoading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {/* Status Banner */}
      {budget.isCritical && (
        <div className="flex items-center gap-2 p-2 bg-red-100 dark:bg-red-900/40 border border-red-300 dark:border-red-700/50 rounded-lg">
          <AlertTriangle className="w-4 h-4 text-red-600 dark:text-red-400" />
          <span className="text-xs text-red-700 dark:text-red-300">
            Budget critical! Context gathering reduced to save costs.
          </span>
        </div>
      )}
      {!budget.isCritical && budget.isLow && (
        <div className="flex items-center gap-2 p-2 bg-yellow-100 dark:bg-yellow-900/40 border border-yellow-300 dark:border-yellow-700/50 rounded-lg">
          <AlertTriangle className="w-4 h-4 text-yellow-600 dark:text-yellow-400" />
          <span className="text-xs text-yellow-700 dark:text-yellow-300">
            Budget running low. Consider reducing usage.
          </span>
        </div>
      )}

      {/* Stats Grid */}
      <div className="grid grid-cols-2 gap-3">
        <StatCard
          label="Daily Spent"
          value={`$${budget.dailySpentUsd.toFixed(2)}`}
          subValue={`of $${budget.dailyLimitUsd.toFixed(2)}`}
          icon={<DollarSign className="w-3 h-3 text-slate-400" />}
          variant={budget.dailyUsagePercent > 90 ? 'danger' : budget.dailyUsagePercent > 70 ? 'warning' : 'default'}
        />
        <StatCard
          label="Daily Remaining"
          value={`$${budget.dailyRemaining.toFixed(2)}`}
          icon={<TrendingUp className="w-3 h-3 text-green-400" />}
        />
        <StatCard
          label="Monthly Spent"
          value={`$${budget.monthlySpentUsd.toFixed(2)}`}
          subValue={`of $${budget.monthlyLimitUsd.toFixed(2)}`}
          icon={<DollarSign className="w-3 h-3 text-slate-400" />}
          variant={budget.monthlyUsagePercent > 90 ? 'danger' : budget.monthlyUsagePercent > 70 ? 'warning' : 'default'}
        />
        <StatCard
          label="Monthly Remaining"
          value={`$${budget.monthlyRemaining.toFixed(2)}`}
          icon={<TrendingUp className="w-3 h-3 text-green-400" />}
        />
      </div>

      {/* Progress Bars */}
      <div className="space-y-3">
        <ProgressBar
          value={budget.dailySpentUsd}
          max={budget.dailyLimitUsd}
          label="Daily Budget"
        />
        <ProgressBar
          value={budget.monthlySpentUsd}
          max={budget.monthlyLimitUsd}
          label="Monthly Budget"
        />
      </div>

      {/* Context Level Indicator */}
      <div className="pt-2 border-t border-gray-200 dark:border-slate-700">
        <div className="flex items-center justify-between text-xs">
          <span className="text-gray-500 dark:text-slate-400">Context Gathering Level</span>
          <span className={`font-medium ${
            budget.dailyUsagePercent > 80 || budget.monthlyUsagePercent > 80
              ? 'text-red-600 dark:text-red-400'
              : budget.dailyUsagePercent > 40 || budget.monthlyUsagePercent > 40
                ? 'text-yellow-600 dark:text-yellow-400'
                : 'text-green-600 dark:text-green-400'
          }`}>
            {budget.dailyUsagePercent > 80 || budget.monthlyUsagePercent > 80
              ? 'Minimal'
              : budget.dailyUsagePercent > 40 || budget.monthlyUsagePercent > 40
                ? 'Standard'
                : 'Full'}
          </span>
        </div>
        <p className="mt-1 text-xs text-gray-500 dark:text-slate-500">
          {budget.dailyUsagePercent > 80 || budget.monthlyUsagePercent > 80
            ? 'Only essential code intelligence is gathered to conserve budget.'
            : budget.dailyUsagePercent > 40 || budget.monthlyUsagePercent > 40
              ? 'Standard context gathering with most features enabled.'
              : 'Full context gathering including expertise and all code intelligence.'}
        </p>
      </div>

      {/* Last Updated */}
      {budget.lastUpdated > 0 && (
        <div className="text-xs text-gray-500 dark:text-slate-500 text-right">
          Updated {new Date(budget.lastUpdated).toLocaleTimeString()}
        </div>
      )}
    </div>
  );
}
