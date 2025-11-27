// frontend/src/components/ToolsDashboard.tsx
// Dashboard for displaying synthesized tools, patterns, and statistics

import React, { useEffect, useCallback, useState } from 'react';
import {
  Wrench,
  Lightbulb,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  XCircle,
  Clock,
  Code2,
  Sparkles,
  TrendingUp,
  AlertTriangle,
} from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface SynthesizedTool {
  id: string;
  name: string;
  description: string;
  version: number;
  language: string;
  compilationStatus: 'pending' | 'compiling' | 'success' | 'failed';
  compilationError: string | null;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

interface ToolPattern {
  id: number;
  patternName: string;
  patternType: string;
  description: string;
  detectedOccurrences: number;
  confidenceScore: number;
  shouldSynthesize: boolean;
  exampleLocations: Array<{
    file_path: string;
    start_line: number;
    end_line: number;
    symbol_name?: string;
  }>;
  createdAt: number;
}

interface SynthesisStats {
  totalPatterns: number;
  patternsWithTools: number;
  totalTools: number;
  activeTools: number;
  totalExecutions: number;
  successfulExecutions: number;
  averageSuccessRate: number;
  toolsBelowThreshold: number;
}

function PatternTypeIcon({ type }: { type: string }) {
  switch (type) {
    case 'file_operation':
      return <span className="text-blue-400">File</span>;
    case 'api_call':
      return <span className="text-purple-400">API</span>;
    case 'data_transformation':
      return <span className="text-green-400">Data</span>;
    case 'validation':
      return <span className="text-yellow-400">Validate</span>;
    case 'database_query':
      return <span className="text-orange-400">DB</span>;
    case 'error_handling':
      return <span className="text-red-400">Error</span>;
    case 'testing':
      return <span className="text-cyan-400">Test</span>;
    default:
      return <span className="text-slate-400">{type}</span>;
  }
}

function CompilationStatusBadge({ status }: { status: string }) {
  switch (status) {
    case 'success':
      return (
        <span className="flex items-center gap-1 text-xs text-green-400">
          <CheckCircle2 className="w-3 h-3" />
          Ready
        </span>
      );
    case 'failed':
      return (
        <span className="flex items-center gap-1 text-xs text-red-400">
          <XCircle className="w-3 h-3" />
          Failed
        </span>
      );
    case 'compiling':
      return (
        <span className="flex items-center gap-1 text-xs text-yellow-400">
          <RefreshCw className="w-3 h-3 animate-spin" />
          Compiling
        </span>
      );
    default:
      return (
        <span className="flex items-center gap-1 text-xs text-slate-400">
          <Clock className="w-3 h-3" />
          Pending
        </span>
      );
  }
}

export function ToolsDashboard() {
  const [tools, setTools] = useState<SynthesizedTool[]>([]);
  const [patterns, setPatterns] = useState<ToolPattern[]>([]);
  const [stats, setStats] = useState<SynthesisStats | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [expandedTools, setExpandedTools] = useState<Set<string>>(new Set());
  const [expandedPatterns, setExpandedPatterns] = useState<Set<number>>(new Set());
  const [activeSection, setActiveSection] = useState<'tools' | 'patterns' | 'stats'>('tools');

  const send = useWebSocketStore((state) => state.send);
  const subscribe = useWebSocketStore((state) => state.subscribe);
  const { currentProject } = useAppState();

  const loadData = useCallback(async () => {
    if (!currentProject?.id) return;

    setIsLoading(true);
    try {
      await Promise.all([
        send({
          type: 'code_intelligence_command',
          method: 'code.tools_list',
          params: { project_id: currentProject.id },
        }),
        send({
          type: 'code_intelligence_command',
          method: 'code.tool_patterns',
          params: { project_id: currentProject.id },
        }),
        send({
          type: 'code_intelligence_command',
          method: 'code.synthesis_stats',
          params: {},
        }),
      ]);
    } catch (err) {
      console.error('Failed to load synthesis data:', err);
    }
  }, [send, currentProject?.id]);

  // Subscribe to WebSocket responses
  useEffect(() => {
    const unsubscribe = subscribe('tools-dashboard', (message) => {
      if (message.type === 'data' && message.data) {
        const data = message.data;

        if (data.type === 'tools_list') {
          setTools(
            (data.tools || []).map((t: Record<string, unknown>) => ({
              id: t.id,
              name: t.name,
              description: t.description,
              version: t.version,
              language: t.language,
              compilationStatus: t.compilation_status,
              compilationError: t.compilation_error,
              enabled: t.enabled,
              createdAt: t.created_at,
              updatedAt: t.updated_at,
            }))
          );
          setIsLoading(false);
        }

        if (data.type === 'tool_patterns') {
          setPatterns(
            (data.patterns || []).map((p: Record<string, unknown>) => ({
              id: p.id,
              patternName: p.pattern_name,
              patternType: p.pattern_type,
              description: p.description,
              detectedOccurrences: p.detected_occurrences,
              confidenceScore: p.confidence_score,
              shouldSynthesize: p.should_synthesize,
              exampleLocations: p.example_locations || [],
              createdAt: p.created_at,
            }))
          );
        }

        if (data.type === 'synthesis_stats') {
          setStats({
            totalPatterns: data.total_patterns,
            patternsWithTools: data.patterns_with_tools,
            totalTools: data.total_tools,
            activeTools: data.active_tools,
            totalExecutions: data.total_executions,
            successfulExecutions: data.successful_executions,
            averageSuccessRate: data.average_success_rate,
            toolsBelowThreshold: data.tools_below_threshold,
          });
        }
      }
    });

    return unsubscribe;
  }, [subscribe]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const toggleTool = (id: string) => {
    const newExpanded = new Set(expandedTools);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedTools(newExpanded);
  };

  const togglePattern = (id: number) => {
    const newExpanded = new Set(expandedPatterns);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedPatterns(newExpanded);
  };

  if (!currentProject) {
    return (
      <div className="p-4 text-center text-slate-500">
        <Wrench className="w-8 h-8 mx-auto mb-2 text-slate-600" />
        <p className="text-sm">Select a project to view tools</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="p-4 space-y-3">
        <div className="flex items-center gap-2 text-slate-400">
          <RefreshCw className="w-4 h-4 animate-spin" />
          <span className="text-sm">Loading synthesis data...</span>
        </div>
        <div className="space-y-2">
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
        {(['tools', 'patterns', 'stats'] as const).map((section) => (
          <button
            key={section}
            onClick={() => setActiveSection(section)}
            className={`flex-1 px-2 py-2 text-xs font-medium transition-colors ${
              activeSection === section
                ? 'text-blue-400 border-b-2 border-blue-400 bg-slate-800/50'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/30'
            }`}
          >
            {section === 'tools' && `Tools (${tools.length})`}
            {section === 'patterns' && `Patterns (${patterns.length})`}
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
        {/* Tools Section */}
        {activeSection === 'tools' && (
          <div className="p-2 space-y-2">
            {tools.length === 0 ? (
              <div className="text-center py-8 text-slate-500">
                <Wrench className="w-8 h-8 mx-auto mb-2 text-slate-600" />
                <p className="text-sm">No synthesized tools yet</p>
                <p className="text-xs mt-1 text-slate-600">
                  Tools are auto-generated from detected patterns
                </p>
              </div>
            ) : (
              tools.map((tool) => (
                <div
                  key={tool.id}
                  className="bg-slate-800/50 border border-slate-700 rounded-lg overflow-hidden"
                >
                  <button
                    onClick={() => toggleTool(tool.id)}
                    className="w-full p-2 flex items-start gap-2 text-left hover:bg-slate-700/50"
                  >
                    {expandedTools.has(tool.id) ? (
                      <ChevronDown className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    ) : (
                      <ChevronRight className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    )}
                    <Wrench className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                      tool.enabled ? 'text-green-400' : 'text-slate-500'
                    }`} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm text-slate-200 font-medium">{tool.name}</span>
                        <span className="text-xs text-slate-500">v{tool.version}</span>
                      </div>
                      <div className="text-xs text-slate-400 mt-0.5 line-clamp-1">
                        {tool.description}
                      </div>
                    </div>
                    <CompilationStatusBadge status={tool.compilationStatus} />
                  </button>
                  {expandedTools.has(tool.id) && (
                    <div className="px-8 pb-3 space-y-2 border-t border-slate-700/50">
                      <div className="pt-2">
                        <div className="text-xs text-slate-400 mb-1">Description:</div>
                        <div className="text-xs text-slate-300">{tool.description}</div>
                      </div>
                      <div className="flex gap-4 text-xs text-slate-500">
                        <span>Language: <span className="text-slate-300">{tool.language}</span></span>
                        <span>Status: <span className={tool.enabled ? 'text-green-400' : 'text-slate-400'}>
                          {tool.enabled ? 'Enabled' : 'Disabled'}
                        </span></span>
                      </div>
                      {tool.compilationError && (
                        <div>
                          <div className="text-xs text-red-400 mb-1">Compilation Error:</div>
                          <pre className="text-xs text-red-300 bg-slate-900 p-2 rounded overflow-x-auto">
                            {tool.compilationError}
                          </pre>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        )}

        {/* Patterns Section */}
        {activeSection === 'patterns' && (
          <div className="p-2 space-y-2">
            {patterns.length === 0 ? (
              <div className="text-center py-8 text-slate-500">
                <Lightbulb className="w-8 h-8 mx-auto mb-2 text-slate-600" />
                <p className="text-sm">No patterns detected yet</p>
                <p className="text-xs mt-1 text-slate-600">
                  Patterns emerge from code analysis
                </p>
              </div>
            ) : (
              patterns.map((pattern) => (
                <div
                  key={pattern.id}
                  className="bg-slate-800/50 border border-slate-700 rounded-lg overflow-hidden"
                >
                  <button
                    onClick={() => togglePattern(pattern.id)}
                    className="w-full p-2 flex items-start gap-2 text-left hover:bg-slate-700/50"
                  >
                    {expandedPatterns.has(pattern.id) ? (
                      <ChevronDown className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    ) : (
                      <ChevronRight className="w-4 h-4 mt-0.5 text-slate-400 flex-shrink-0" />
                    )}
                    <Sparkles className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                      pattern.shouldSynthesize ? 'text-yellow-400' : 'text-slate-500'
                    }`} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm text-slate-200">{pattern.patternName}</span>
                        <PatternTypeIcon type={pattern.patternType} />
                      </div>
                      <div className="flex items-center gap-2 mt-0.5 text-xs text-slate-500">
                        <span>{pattern.detectedOccurrences} occurrences</span>
                        <span className="text-slate-600">|</span>
                        <span className={
                          pattern.confidenceScore > 0.8 ? 'text-green-400' :
                          pattern.confidenceScore > 0.5 ? 'text-yellow-400' : 'text-slate-400'
                        }>
                          {(pattern.confidenceScore * 100).toFixed(0)}% confidence
                        </span>
                      </div>
                    </div>
                  </button>
                  {expandedPatterns.has(pattern.id) && (
                    <div className="px-8 pb-3 space-y-2 border-t border-slate-700/50">
                      <div className="pt-2">
                        <div className="text-xs text-slate-400 mb-1">Description:</div>
                        <div className="text-xs text-slate-300">{pattern.description}</div>
                      </div>
                      {pattern.exampleLocations.length > 0 && (
                        <div>
                          <div className="text-xs text-slate-400 mb-1">Example Locations:</div>
                          <div className="space-y-1">
                            {pattern.exampleLocations.slice(0, 3).map((loc, idx) => (
                              <div key={idx} className="text-xs text-slate-500 flex items-center gap-1">
                                <Code2 className="w-3 h-3" />
                                <span className="text-slate-300">{loc.file_path}</span>
                                <span>:{loc.start_line}</span>
                                {loc.symbol_name && (
                                  <span className="text-purple-400">({loc.symbol_name})</span>
                                )}
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                      {pattern.shouldSynthesize && (
                        <div className="flex items-center gap-1 text-xs text-yellow-400">
                          <Sparkles className="w-3 h-3" />
                          <span>Ready for synthesis</span>
                        </div>
                      )}
                    </div>
                  )}
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
                <div className="text-xs text-slate-400">Patterns</div>
                <div className="text-lg font-semibold text-slate-100">{stats.totalPatterns}</div>
                <div className="text-xs text-slate-500">{stats.patternsWithTools} with tools</div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Tools</div>
                <div className="text-lg font-semibold text-slate-100">{stats.totalTools}</div>
                <div className="text-xs text-green-400">{stats.activeTools} active</div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Executions</div>
                <div className="text-lg font-semibold text-slate-100">{stats.totalExecutions}</div>
                <div className="text-xs text-green-400">{stats.successfulExecutions} successful</div>
              </div>
              <div className="p-2 bg-slate-800/50 rounded-lg border border-slate-700/50">
                <div className="text-xs text-slate-400">Success Rate</div>
                <div className={`text-lg font-semibold ${
                  stats.averageSuccessRate > 0.8 ? 'text-green-400' :
                  stats.averageSuccessRate > 0.5 ? 'text-yellow-400' : 'text-red-400'
                }`}>
                  {(stats.averageSuccessRate * 100).toFixed(0)}%
                </div>
              </div>
            </div>

            {/* Synthesis Progress */}
            <div className="space-y-2">
              <div className="text-xs text-slate-400 font-medium flex items-center gap-1">
                <TrendingUp className="w-3 h-3" />
                Pattern Coverage
              </div>
              <div className="h-2 bg-slate-700 rounded-full overflow-hidden">
                <div
                  className="h-full bg-purple-500"
                  style={{
                    width: `${(stats.patternsWithTools / Math.max(stats.totalPatterns, 1)) * 100}%`
                  }}
                />
              </div>
              <div className="flex justify-between text-xs text-slate-500">
                <span className="text-purple-400">{stats.patternsWithTools} converted</span>
                <span>{stats.totalPatterns - stats.patternsWithTools} pending</span>
              </div>
            </div>

            {/* Tool Health */}
            <div className="space-y-2">
              <div className="text-xs text-slate-400 font-medium">Tool Health</div>
              <div className="h-2 bg-slate-700 rounded-full overflow-hidden flex">
                <div
                  className="h-full bg-green-500"
                  style={{
                    width: `${(stats.activeTools / Math.max(stats.totalTools, 1)) * 100}%`
                  }}
                />
              </div>
              <div className="flex justify-between text-xs text-slate-500">
                <span className="text-green-400">{stats.activeTools} active</span>
                <span className="text-slate-400">{stats.totalTools - stats.activeTools} inactive</span>
              </div>
            </div>

            {/* Alerts */}
            {stats.toolsBelowThreshold > 0 && (
              <div className="flex items-center gap-2 p-2 bg-yellow-900/30 border border-yellow-700/50 rounded-lg">
                <AlertTriangle className="w-4 h-4 text-yellow-400" />
                <span className="text-xs text-yellow-300">
                  {stats.toolsBelowThreshold} tool{stats.toolsBelowThreshold > 1 ? 's' : ''} below effectiveness threshold
                </span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
