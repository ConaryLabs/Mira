// src/components/BackgroundAgentsPanel.tsx
// Panel showing background agents (Codex sessions) status and controls

import React, { useEffect } from 'react';
import {
  X,
  Bot,
  Loader2,
  CheckCircle,
  XCircle,
  StopCircle,
  Clock,
  Cpu,
  DollarSign,
  RefreshCw,
  ChevronDown,
  ChevronUp,
} from 'lucide-react';
import { useAgentStore, formatAgentDuration, formatCost, type BackgroundAgent } from '../stores/useAgentStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';

export function BackgroundAgentsPanel() {
  const {
    agents,
    isPanelVisible,
    loading,
    selectedAgentId,
    togglePanel,
    selectAgent,
    setAgents,
    updateAgent,
    setLoading,
  } = useAgentStore();

  const { sendMessage, subscribe } = useWebSocketStore();
  const { currentSessionId } = useChatStore();

  // Subscribe to agent updates
  useEffect(() => {
    const unsubscribe = subscribe('agents-panel', (message) => {
      if (message.type === 'data') {
        const data = message.data;

        if (data?.type === 'active_agents') {
          setAgents(data.agents || []);
        } else if (data?.type === 'agent_info') {
          updateAgent(data.id, {
            task: data.task,
            status: data.status,
            started_at: data.started_at,
            completed_at: data.completed_at,
            tokens_used: (data.tokens_input || 0) + (data.tokens_output || 0),
            cost_usd: data.cost_usd || 0,
            compaction_count: data.compaction_count || 0,
            completion_summary: data.completion_summary,
          });
        }
      }
    });

    return unsubscribe;
  }, [subscribe, setAgents, updateAgent]);

  // Refresh agents when panel opens
  useEffect(() => {
    if (isPanelVisible && currentSessionId) {
      refreshAgents();
    }
  }, [isPanelVisible, currentSessionId]);

  const refreshAgents = () => {
    if (!currentSessionId) return;
    setLoading(true);
    sendMessage({
      type: 'command',
      method: 'session.active_agents',
      params: { voice_session_id: currentSessionId },
    });
  };

  const cancelAgent = (agentId: string) => {
    sendMessage({
      type: 'command',
      method: 'session.cancel_agent',
      params: { codex_session_id: agentId },
    });
    // Optimistic update
    updateAgent(agentId, { status: 'cancelled' });
  };

  const showAgentDetails = (agentId: string) => {
    selectAgent(selectedAgentId === agentId ? null : agentId);
    sendMessage({
      type: 'command',
      method: 'session.agent_info',
      params: { codex_session_id: agentId },
    });
  };

  if (!isPanelVisible) {
    return null;
  }

  const runningAgents = agents.filter(a => a.status === 'running');
  const completedAgents = agents.filter(a => a.status !== 'running');

  return (
    <div className="fixed bottom-4 right-4 w-96 bg-white dark:bg-slate-900 border border-gray-200 dark:border-slate-700 rounded-lg shadow-xl z-40 max-h-[60vh] flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
        <div className="flex items-center gap-2">
          <Bot className="w-5 h-5 text-blue-500" />
          <h2 className="text-sm font-semibold text-gray-800 dark:text-slate-200">
            Background Agents
          </h2>
          {runningAgents.length > 0 && (
            <span className="px-2 py-0.5 text-xs font-medium bg-blue-100 dark:bg-blue-900/50 text-blue-600 dark:text-blue-400 rounded-full">
              {runningAgents.length} running
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={refreshAgents}
            disabled={loading}
            className="p-1.5 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            title="Refresh"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button
            onClick={togglePanel}
            className="p-1.5 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            title="Close"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {loading && agents.length === 0 ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-6 h-6 text-gray-400 animate-spin" />
          </div>
        ) : agents.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
            <Bot className="w-12 h-12 text-gray-400 dark:text-slate-600 mb-3" />
            <p className="text-sm text-gray-500 dark:text-slate-400">
              No background agents running
            </p>
            <p className="text-xs text-gray-400 dark:text-slate-500 mt-1">
              Agents will appear here when spawned for complex tasks
            </p>
          </div>
        ) : (
          <div className="divide-y divide-gray-100 dark:divide-slate-700">
            {/* Running Agents */}
            {runningAgents.map(agent => (
              <AgentItem
                key={agent.id}
                agent={agent}
                isSelected={selectedAgentId === agent.id}
                onSelect={() => showAgentDetails(agent.id)}
                onCancel={() => cancelAgent(agent.id)}
              />
            ))}

            {/* Completed Agents (collapsed by default) */}
            {completedAgents.length > 0 && (
              <CompletedAgentsSection
                agents={completedAgents}
                selectedAgentId={selectedAgentId}
                onSelectAgent={showAgentDetails}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

interface AgentItemProps {
  agent: BackgroundAgent;
  isSelected: boolean;
  onSelect: () => void;
  onCancel?: () => void;
}

function AgentItem({ agent, isSelected, onSelect, onCancel }: AgentItemProps) {
  const isRunning = agent.status === 'running';

  return (
    <div className="px-4 py-3">
      <div
        className="flex items-start gap-3 cursor-pointer"
        onClick={onSelect}
      >
        {/* Status Icon */}
        <div className="flex-shrink-0 mt-0.5">
          {isRunning ? (
            <Loader2 className="w-5 h-5 text-blue-500 animate-spin" />
          ) : agent.status === 'completed' ? (
            <CheckCircle className="w-5 h-5 text-green-500" />
          ) : agent.status === 'cancelled' ? (
            <StopCircle className="w-5 h-5 text-yellow-500" />
          ) : (
            <XCircle className="w-5 h-5 text-red-500" />
          )}
        </div>

        {/* Info */}
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-gray-800 dark:text-slate-200 truncate">
            {agent.task || 'Background task'}
          </p>

          {/* Progress/Activity */}
          {isRunning && agent.current_activity && (
            <p className="text-xs text-gray-500 dark:text-slate-400 truncate mt-0.5">
              {agent.current_activity}
            </p>
          )}

          {/* Stats */}
          <div className="flex items-center gap-3 mt-1.5 text-xs text-gray-400 dark:text-slate-500">
            <span className="flex items-center gap-1">
              <Clock className="w-3 h-3" />
              {formatAgentDuration(agent.started_at, agent.completed_at)}
            </span>
            <span className="flex items-center gap-1">
              <Cpu className="w-3 h-3" />
              {(agent.tokens_used / 1000).toFixed(1)}k tokens
            </span>
            <span className="flex items-center gap-1">
              <DollarSign className="w-3 h-3" />
              {formatCost(agent.cost_usd)}
            </span>
          </div>
        </div>

        {/* Cancel button for running agents */}
        {isRunning && onCancel && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onCancel();
            }}
            className="flex-shrink-0 p-1.5 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 rounded transition-colors"
            title="Cancel agent"
          >
            <StopCircle className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Expanded details */}
      {isSelected && agent.completion_summary && (
        <div className="mt-3 pt-3 border-t border-gray-100 dark:border-slate-700">
          <p className="text-xs font-medium text-gray-600 dark:text-slate-400 mb-1">
            Summary
          </p>
          <p className="text-sm text-gray-700 dark:text-slate-300 whitespace-pre-wrap">
            {agent.completion_summary}
          </p>
        </div>
      )}
    </div>
  );
}

interface CompletedAgentsSectionProps {
  agents: BackgroundAgent[];
  selectedAgentId: string | null;
  onSelectAgent: (id: string) => void;
}

function CompletedAgentsSection({ agents, selectedAgentId, onSelectAgent }: CompletedAgentsSectionProps) {
  const [isExpanded, setIsExpanded] = React.useState(false);

  return (
    <div>
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center justify-between px-4 py-2 text-xs text-gray-500 dark:text-slate-400 hover:bg-gray-50 dark:hover:bg-slate-800 transition-colors"
      >
        <span>Completed ({agents.length})</span>
        {isExpanded ? (
          <ChevronUp className="w-4 h-4" />
        ) : (
          <ChevronDown className="w-4 h-4" />
        )}
      </button>

      {isExpanded && (
        <div className="divide-y divide-gray-100 dark:divide-slate-700">
          {agents.slice(0, 5).map(agent => (
            <AgentItem
              key={agent.id}
              agent={agent}
              isSelected={selectedAgentId === agent.id}
              onSelect={() => onSelectAgent(agent.id)}
            />
          ))}
          {agents.length > 5 && (
            <p className="px-4 py-2 text-xs text-gray-400 dark:text-slate-500 text-center">
              +{agents.length - 5} more
            </p>
          )}
        </div>
      )}
    </div>
  );
}

// Button to toggle the panel (can be placed in header)
export function AgentsPanelToggle() {
  const { isPanelVisible, togglePanel, agents } = useAgentStore();
  const runningCount = agents.filter(a => a.status === 'running').length;

  return (
    <button
      onClick={togglePanel}
      className={`
        relative p-2 rounded-lg transition-colors
        ${isPanelVisible
          ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-600 dark:text-blue-400'
          : 'text-gray-500 dark:text-slate-400 hover:bg-gray-100 dark:hover:bg-slate-700'
        }
      `}
      title="Background Agents"
    >
      <Bot className="w-5 h-5" />
      {runningCount > 0 && (
        <span className="absolute -top-1 -right-1 w-4 h-4 flex items-center justify-center text-[10px] font-bold bg-blue-500 text-white rounded-full">
          {runningCount}
        </span>
      )}
    </button>
  );
}
