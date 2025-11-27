// frontend/src/components/BuildErrorsPanel.tsx
// Panel for displaying build statistics, recent builds, and unresolved errors

import React, { useEffect, useCallback, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  XCircle,
  Clock,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  FileCode,
  Terminal
} from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface BuildStats {
  projectId: string;
  totalBuilds: number;
  successfulBuilds: number;
  failedBuilds: number;
  successRate: number;
  totalErrors: number;
  resolvedErrors: number;
  unresolvedErrors: number;
  averageDurationMs: number;
  mostCommonErrors: [string, number][];
}

interface BuildRun {
  id: number;
  buildType: string;
  command: string;
  exitCode: number;
  durationMs: number;
  startedAt: number;
  errorCount: number;
  warningCount: number;
  success: boolean;
}

interface BuildError {
  id: number;
  errorHash: string;
  severity: string;
  errorCode: string | null;
  message: string;
  filePath: string | null;
  lineNumber: number | null;
  columnNumber: number | null;
  suggestion: string | null;
  category: string;
  firstSeenAt: number;
  lastSeenAt: number;
  occurrenceCount: number;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

function formatTimeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() / 1000) - timestamp);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

function BuildTypeIcon({ type }: { type: string }) {
  switch (type) {
    case 'cargo_build':
    case 'cargo_check':
    case 'cargo_test':
    case 'cargo_clippy':
      return <span className="text-orange-400">Cargo</span>;
    case 'npm_build':
    case 'npm_test':
      return <span className="text-green-400">npm</span>;
    case 'tsc':
      return <span className="text-blue-400">tsc</span>;
    case 'pytest':
      return <span className="text-yellow-400">pytest</span>;
    default:
      return <Terminal className="w-3 h-3 text-slate-400" />;
  }
}

export function BuildErrorsPanel() {
  const [stats, setStats] = useState<BuildStats | null>(null);
  const [builds, setBuilds] = useState<BuildRun[]>([]);
  const [errors, setErrors] = useState<BuildError[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [expandedErrors, setExpandedErrors] = useState<Set<number>>(new Set());
  const [activeSection, setActiveSection] = useState<'stats' | 'builds' | 'errors'>('errors');

  const send = useWebSocketStore((state) => state.send);
  const subscribe = useWebSocketStore((state) => state.subscribe);
  const { currentProject } = useAppState();

  const loadData = useCallback(async () => {
    if (!currentProject?.id) return;

    setIsLoading(true);
    try {
      // Load stats, builds, and errors in parallel
      await Promise.all([
        send({
          type: 'code_intelligence_command',
          method: 'code.build_stats',
          params: { project_id: currentProject.id },
        }),
        send({
          type: 'code_intelligence_command',
          method: 'code.recent_builds',
          params: { project_id: currentProject.id, limit: 10 },
        }),
        send({
          type: 'code_intelligence_command',
          method: 'code.build_errors',
          params: { project_id: currentProject.id, limit: 20 },
        }),
      ]);
    } catch (err) {
      console.error('Failed to load build data:', err);
    }
  }, [send, currentProject?.id]);

  // Subscribe to WebSocket responses
  useEffect(() => {
    const unsubscribe = subscribe('build-panel', (message) => {
      if (message.type === 'data' && message.data) {
        const data = message.data;

        if (data.type === 'build_stats') {
          setStats({
            projectId: data.project_id,
            totalBuilds: data.total_builds,
            successfulBuilds: data.successful_builds,
            failedBuilds: data.failed_builds,
            successRate: data.success_rate,
            totalErrors: data.total_errors,
            resolvedErrors: data.resolved_errors,
            unresolvedErrors: data.unresolved_errors,
            averageDurationMs: data.average_duration_ms,
            mostCommonErrors: data.most_common_errors || [],
          });
          setIsLoading(false);
        }

        if (data.type === 'recent_builds') {
          setBuilds(
            (data.builds || []).map((b: Record<string, unknown>) => ({
              id: b.id,
              buildType: b.build_type,
              command: b.command,
              exitCode: b.exit_code,
              durationMs: b.duration_ms,
              startedAt: b.started_at,
              errorCount: b.error_count,
              warningCount: b.warning_count,
              success: b.success,
            }))
          );
        }

        if (data.type === 'build_errors') {
          setErrors(
            (data.errors || []).map((e: Record<string, unknown>) => ({
              id: e.id,
              errorHash: e.error_hash,
              severity: e.severity,
              errorCode: e.error_code,
              message: e.message,
              filePath: e.file_path,
              lineNumber: e.line_number,
              columnNumber: e.column_number,
              suggestion: e.suggestion,
              category: e.category,
              firstSeenAt: e.first_seen_at,
              lastSeenAt: e.last_seen_at,
              occurrenceCount: e.occurrence_count,
            }))
          );
        }
      }
    });

    return unsubscribe;
  }, [subscribe]);

  // Load data on mount and project change
  useEffect(() => {
    loadData();
  }, [loadData]);

  const toggleError = (id: number) => {
    const newExpanded = new Set(expandedErrors);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedErrors(newExpanded);
  };

  if (!currentProject) {
    return (
      <div className="p-4 text-center text-slate-500">
        <AlertTriangle className="w-8 h-8 mx-auto mb-2 text-slate-600" />
        <p className="text-sm">Select a project to view builds</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="p-4 space-y-3">
        <div className="flex items-center gap-2 text-slate-400">
          <RefreshCw className="w-4 h-4 animate-spin" />
          <span className="text-sm">Loading build data...</span>
        </div>
        <div className="space-y-2">
          <div className="h-6 bg-slate-800 rounded animate-pulse" />
          <div className="h-6 bg-slate-800 rounded animate-pulse" />
          <div className="h-6 bg-slate-800 rounded animate-pulse" />
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col overflow-hidden">
      {/* Tab Navigation */}
      <div className="flex-shrink-0 flex border-b border-slate-700">
        {(['errors', 'builds', 'stats'] as const).map((section) => (
          <button
            key={section}
            onClick={() => setActiveSection(section)}
            className={`flex-1 px-2 py-2 text-xs font-medium transition-colors ${
              activeSection === section
                ? 'text-blue-400 border-b-2 border-blue-400 bg-slate-800/50'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/30'
            }`}
          >
            {section === 'errors' && `Errors (${errors.length})`}
            {section === 'builds' && `Builds (${builds.length})`}
            {section === 'stats' && 'Stats'}
          </button>
        ))}
        <button
          onClick={loadData}
          className="p-2 text-slate-400 hover:text-slate-200 hover:bg-slate-800/30"
          title="Refresh"
        >
          <RefreshCw className="w-3 h-3" />
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {/* Errors Section */}
        {activeSection === 'errors' && (
          <div className="p-2 space-y-2">
            {errors.length === 0 ? (
              <div className="text-center py-8 text-slate-500">
                <CheckCircle2 className="w-8 h-8 mx-auto mb-2 text-green-500" />
                <p className="text-sm">No unresolved errors</p>
              </div>
            ) : (
              errors.map((error) => (
                <div
                  key={error.id}
                  className="bg-slate-800/50 border border-slate-700 rounded-lg overflow-hidden"
                >
                  <button
                    onClick={() => toggleError(error.id)}
                    className="w-full p-2 flex items-start gap-2 text-left hover:bg-slate-700/50"
                  >
                    {expandedErrors.has(error.id) ? (
                      <ChevronDown className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    ) : (
                      <ChevronRight className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    )}
                    <XCircle className="w-4 h-4 mt-0.5 text-red-400 flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <div className="text-xs text-slate-200 line-clamp-2">
                        {error.errorCode && (
                          <span className="text-red-400 font-mono mr-1">[{error.errorCode}]</span>
                        )}
                        {error.message}
                      </div>
                      <div className="flex items-center gap-2 mt-1 text-xs text-slate-500">
                        {error.filePath && (
                          <span className="truncate max-w-[150px]">
                            <FileCode className="w-3 h-3 inline mr-1" />
                            {error.filePath.split('/').pop()}
                            {error.lineNumber && `:${error.lineNumber}`}
                          </span>
                        )}
                        <span className="text-slate-600">|</span>
                        <span>{error.occurrenceCount}x</span>
                        <span className="text-slate-600">|</span>
                        <span>{formatTimeAgo(error.lastSeenAt)}</span>
                      </div>
                    </div>
                  </button>
                  {expandedErrors.has(error.id) && (
                    <div className="px-8 pb-3 space-y-2 border-t border-slate-700/50">
                      <div className="pt-2">
                        <div className="text-xs text-slate-400 mb-1">Full Message:</div>
                        <pre className="text-xs text-slate-300 bg-slate-900 p-2 rounded overflow-x-auto">
                          {error.message}
                        </pre>
                      </div>
                      {error.suggestion && (
                        <div>
                          <div className="text-xs text-slate-400 mb-1">Suggestion:</div>
                          <div className="text-xs text-green-400 bg-slate-900 p-2 rounded">
                            {error.suggestion}
                          </div>
                        </div>
                      )}
                      <div className="flex gap-4 text-xs text-slate-500">
                        <span>Category: <span className="text-slate-300">{error.category}</span></span>
                        <span>First seen: {formatTimeAgo(error.firstSeenAt)}</span>
                      </div>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        )}

        {/* Builds Section */}
        {activeSection === 'builds' && (
          <div className="p-2 space-y-1">
            {builds.length === 0 ? (
              <div className="text-center py-8 text-slate-500">
                <Terminal className="w-8 h-8 mx-auto mb-2 text-slate-600" />
                <p className="text-sm">No builds recorded</p>
              </div>
            ) : (
              builds.map((build) => (
                <div
                  key={build.id}
                  className="flex items-center gap-2 p-2 bg-slate-800/30 rounded hover:bg-slate-800/50"
                >
                  {build.success ? (
                    <CheckCircle2 className="w-4 h-4 text-green-500 flex-shrink-0" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-500 flex-shrink-0" />
                  )}
                  <div className="flex-1 min-w-0">
                    <div className="text-xs text-slate-300 truncate">
                      <BuildTypeIcon type={build.buildType} />
                      <span className="ml-1 text-slate-400">{build.command.slice(0, 30)}...</span>
                    </div>
                    <div className="flex items-center gap-2 text-xs text-slate-500">
                      <Clock className="w-3 h-3" />
                      <span>{formatDuration(build.durationMs)}</span>
                      {build.errorCount > 0 && (
                        <span className="text-red-400">{build.errorCount} errors</span>
                      )}
                      {build.warningCount > 0 && (
                        <span className="text-yellow-400">{build.warningCount} warnings</span>
                      )}
                    </div>
                  </div>
                  <span className="text-xs text-slate-500">{formatTimeAgo(build.startedAt)}</span>
                </div>
              ))
            )}
          </div>
        )}

        {/* Stats Section */}
        {activeSection === 'stats' && stats && (
          <div className="p-3 space-y-4">
            {/* Overview Cards */}
            <div className="grid grid-cols-2 gap-2">
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Total Builds</div>
                <div className="text-lg font-semibold text-slate-100">{stats.totalBuilds}</div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Success Rate</div>
                <div className={`text-lg font-semibold ${
                  stats.successRate > 0.8 ? 'text-green-400' :
                  stats.successRate > 0.5 ? 'text-yellow-400' : 'text-red-400'
                }`}>
                  {(stats.successRate * 100).toFixed(0)}%
                </div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Avg Duration</div>
                <div className="text-lg font-semibold text-slate-100">
                  {formatDuration(stats.averageDurationMs)}
                </div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Unresolved</div>
                <div className={`text-lg font-semibold ${
                  stats.unresolvedErrors === 0 ? 'text-green-400' : 'text-red-400'
                }`}>
                  {stats.unresolvedErrors}
                </div>
              </div>
            </div>

            {/* Build Breakdown */}
            <div className="space-y-2">
              <div className="text-xs text-slate-400 font-medium">Build Results</div>
              <div className="h-2 bg-slate-700 rounded-full overflow-hidden flex">
                <div
                  className="h-full bg-green-500"
                  style={{ width: `${(stats.successfulBuilds / Math.max(stats.totalBuilds, 1)) * 100}%` }}
                />
                <div
                  className="h-full bg-red-500"
                  style={{ width: `${(stats.failedBuilds / Math.max(stats.totalBuilds, 1)) * 100}%` }}
                />
              </div>
              <div className="flex justify-between text-xs text-slate-500">
                <span className="text-green-400">{stats.successfulBuilds} passed</span>
                <span className="text-red-400">{stats.failedBuilds} failed</span>
              </div>
            </div>

            {/* Error Resolution */}
            <div className="space-y-2">
              <div className="text-xs text-slate-400 font-medium">Error Resolution</div>
              <div className="h-2 bg-slate-700 rounded-full overflow-hidden flex">
                <div
                  className="h-full bg-green-500"
                  style={{ width: `${(stats.resolvedErrors / Math.max(stats.totalErrors, 1)) * 100}%` }}
                />
              </div>
              <div className="flex justify-between text-xs text-slate-500">
                <span className="text-green-400">{stats.resolvedErrors} resolved</span>
                <span className="text-slate-400">{stats.totalErrors} total</span>
              </div>
            </div>

            {/* Most Common Errors */}
            {stats.mostCommonErrors.length > 0 && (
              <div className="space-y-2">
                <div className="text-xs text-slate-400 font-medium">Most Common Errors</div>
                <div className="space-y-1">
                  {stats.mostCommonErrors.slice(0, 5).map(([message, count], idx) => (
                    <div
                      key={idx}
                      className="flex items-center gap-2 p-1.5 bg-slate-800/30 rounded text-xs"
                    >
                      <span className="text-red-400 font-mono">{count}x</span>
                      <span className="text-slate-300 truncate">{message}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
