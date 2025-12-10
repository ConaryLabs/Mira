// src/components/ReviewPanel.tsx
// Code review panel with diff viewer and LLM review functionality

import React, { useEffect, useState } from 'react';
import {
  X,
  GitCommit,
  GitBranch,
  FileCode,
  Play,
  Loader2,
  RefreshCw,
  ChevronDown,
  Plus,
  Minus,
  Eye,
  CheckCircle,
  AlertTriangle,
} from 'lucide-react';
import { useReviewStore, type ReviewTarget } from '../stores/useReviewStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';
import { UnifiedDiffView, DiffStats } from './UnifiedDiffView';

export function ReviewPanel() {
  const {
    isPanelVisible,
    loading,
    diff,
    reviewTarget,
    baseBranch,
    commitHash,
    reviewResult,
    reviewing,
    additions,
    deletions,
    filesChanged,
    togglePanel,
    setLoading,
    setDiff,
    setReviewTarget,
    setBaseBranch,
    setCommitHash,
    setReviewResult,
    setReviewing,
  } = useReviewStore();

  const { sendMessage, subscribe } = useWebSocketStore();
  const { addMessage, setStreaming } = useChatStore();
  const { currentProject } = useAppState();

  const [showTargetDropdown, setShowTargetDropdown] = useState(false);

  // Subscribe to WebSocket responses
  useEffect(() => {
    const unsubscribe = subscribe('review-panel', (message) => {
      if (message.type === 'data' && message.data?.type === 'diff_result') {
        setDiff(message.data.diff || '');
        setLoading(false);
      }
    });

    return unsubscribe;
  }, [subscribe, setDiff, setLoading]);

  // Fetch diff when panel opens or target changes
  useEffect(() => {
    if (isPanelVisible && currentProject) {
      fetchDiff();
    }
  }, [isPanelVisible, reviewTarget, baseBranch, commitHash]);

  const fetchDiff = async () => {
    if (!currentProject?.id) return;

    setLoading(true);
    setDiff(null);

    // Request diff via git command
    sendMessage({
      type: 'git_command',
      method: 'git.diff',
      params: {
        project_id: currentProject.id,
        target: reviewTarget,
        base_branch: reviewTarget === 'branch' ? baseBranch : undefined,
        commit_hash: reviewTarget === 'commit' ? commitHash : undefined,
      },
    });
  };

  const requestReview = () => {
    if (!diff || diff.trim().length === 0) return;

    setReviewing(true);

    // Get description based on target
    let description = 'uncommitted changes';
    if (reviewTarget === 'staged') description = 'staged changes';
    if (reviewTarget === 'branch') description = `changes against ${baseBranch}`;
    if (reviewTarget === 'commit') description = `commit ${commitHash.slice(0, 8)}`;

    // Send review request as chat message
    const reviewPrompt = `Please review the following code changes (${description}).

Provide:
1. A brief summary of what changed
2. Any potential issues or bugs
3. Suggestions for improvement
4. Security considerations if applicable

\`\`\`diff
${diff}
\`\`\``;

    // Add user message
    addMessage({
      id: `review-${Date.now()}`,
      role: 'user',
      content: reviewPrompt,
      timestamp: Date.now(),
    });

    // Send to backend
    sendMessage({
      type: 'chat',
      content: reviewPrompt,
    });

    setStreaming(true);

    // Close panel to show chat response
    togglePanel();
  };

  if (!isPanelVisible) {
    return null;
  }

  const targetOptions: { value: ReviewTarget; label: string; icon: React.ReactNode }[] = [
    { value: 'uncommitted', label: 'Uncommitted Changes', icon: <FileCode className="w-4 h-4" /> },
    { value: 'staged', label: 'Staged Changes', icon: <Plus className="w-4 h-4" /> },
    { value: 'branch', label: 'Against Branch', icon: <GitBranch className="w-4 h-4" /> },
    { value: 'commit', label: 'Specific Commit', icon: <GitCommit className="w-4 h-4" /> },
  ];

  const currentTargetOption = targetOptions.find(o => o.value === reviewTarget);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-slate-900 border border-gray-200 dark:border-slate-700 rounded-lg shadow-2xl w-full max-w-5xl h-[85vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
          <div className="flex items-center gap-3">
            <Eye className="w-5 h-5 text-blue-500" />
            <h2 className="text-lg font-semibold text-gray-800 dark:text-slate-200">
              Code Review
            </h2>

            {/* Target Selector Dropdown */}
            <div className="relative">
              <button
                onClick={() => setShowTargetDropdown(!showTargetDropdown)}
                className="flex items-center gap-2 px-3 py-1.5 bg-gray-100 dark:bg-slate-800 hover:bg-gray-200 dark:hover:bg-slate-700 rounded-lg border border-gray-300 dark:border-slate-600 transition-colors text-sm"
              >
                {currentTargetOption?.icon}
                <span className="text-gray-700 dark:text-slate-200">
                  {currentTargetOption?.label}
                </span>
                <ChevronDown className="w-4 h-4 text-gray-500" />
              </button>

              {showTargetDropdown && (
                <div className="absolute top-full left-0 mt-1 w-56 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-lg shadow-lg z-10">
                  {targetOptions.map(option => (
                    <button
                      key={option.value}
                      onClick={() => {
                        setReviewTarget(option.value);
                        setShowTargetDropdown(false);
                      }}
                      className={`w-full flex items-center gap-2 px-3 py-2 text-sm hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors ${
                        option.value === reviewTarget
                          ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                          : 'text-gray-700 dark:text-slate-200'
                      }`}
                    >
                      {option.icon}
                      {option.label}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {/* Branch input (when target is branch) */}
            {reviewTarget === 'branch' && (
              <input
                type="text"
                value={baseBranch}
                onChange={(e) => setBaseBranch(e.target.value)}
                placeholder="Base branch"
                className="px-3 py-1.5 text-sm bg-gray-100 dark:bg-slate-800 border border-gray-300 dark:border-slate-600 rounded-lg text-gray-800 dark:text-slate-200 w-32"
              />
            )}

            {/* Commit hash input (when target is commit) */}
            {reviewTarget === 'commit' && (
              <input
                type="text"
                value={commitHash}
                onChange={(e) => setCommitHash(e.target.value)}
                placeholder="Commit hash"
                className="px-3 py-1.5 text-sm bg-gray-100 dark:bg-slate-800 border border-gray-300 dark:border-slate-600 rounded-lg text-gray-800 dark:text-slate-200 font-mono w-40"
              />
            )}
          </div>

          <div className="flex items-center gap-2">
            {/* Stats */}
            {diff && (
              <div className="flex items-center gap-3 text-sm mr-4">
                <span className="flex items-center gap-1 text-green-600 dark:text-green-400">
                  <Plus className="w-4 h-4" />
                  {additions}
                </span>
                <span className="flex items-center gap-1 text-red-600 dark:text-red-400">
                  <Minus className="w-4 h-4" />
                  {deletions}
                </span>
                <span className="text-gray-500 dark:text-slate-400">
                  {filesChanged.length} file{filesChanged.length !== 1 ? 's' : ''}
                </span>
              </div>
            )}

            {/* Refresh */}
            <button
              onClick={fetchDiff}
              disabled={loading}
              className="p-2 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
              title="Refresh diff"
            >
              <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
            </button>

            {/* Request Review */}
            <button
              onClick={requestReview}
              disabled={!diff || reviewing || loading}
              className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 text-white rounded-lg transition-colors text-sm font-medium"
            >
              {reviewing ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Play className="w-4 h-4" />
              )}
              Request Review
            </button>

            {/* Close */}
            <button
              onClick={togglePanel}
              className="p-2 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
              title="Close"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Files Changed List */}
        {filesChanged.length > 0 && (
          <div className="flex items-center gap-2 px-4 py-2 border-b border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800/50 overflow-x-auto">
            <span className="text-xs text-gray-500 dark:text-slate-400 flex-shrink-0">Files:</span>
            {filesChanged.map((file, i) => (
              <span
                key={i}
                className="text-xs font-mono px-2 py-0.5 bg-gray-200 dark:bg-slate-700 text-gray-700 dark:text-slate-300 rounded flex-shrink-0"
              >
                {file}
              </span>
            ))}
          </div>
        )}

        {/* Diff Content */}
        <div className="flex-1 overflow-hidden">
          {loading ? (
            <div className="flex items-center justify-center h-full">
              <div className="text-center">
                <Loader2 className="w-8 h-8 text-gray-400 animate-spin mx-auto mb-3" />
                <p className="text-gray-500 dark:text-slate-400">Loading diff...</p>
              </div>
            </div>
          ) : !diff || diff.trim().length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full text-center px-4">
              <CheckCircle className="w-12 h-12 text-green-500 mb-4" />
              <h3 className="text-lg font-medium text-gray-700 dark:text-slate-300 mb-2">
                No Changes
              </h3>
              <p className="text-gray-500 dark:text-slate-400 max-w-md">
                {reviewTarget === 'uncommitted' && 'Your working directory is clean. No uncommitted changes found.'}
                {reviewTarget === 'staged' && 'No staged changes. Use git add to stage files.'}
                {reviewTarget === 'branch' && `No differences between ${baseBranch} and HEAD.`}
                {reviewTarget === 'commit' && 'Enter a commit hash to view changes.'}
              </p>
            </div>
          ) : (
            <UnifiedDiffView diff={diff} />
          )}
        </div>

        {/* Review Result (if available) */}
        {reviewResult && (
          <div className="border-t border-gray-200 dark:border-slate-700 px-4 py-3 bg-blue-50 dark:bg-blue-900/20">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-5 h-5 text-blue-500 flex-shrink-0 mt-0.5" />
              <div>
                <h4 className="text-sm font-medium text-gray-800 dark:text-slate-200 mb-1">
                  Review Summary
                </h4>
                <p className="text-sm text-gray-600 dark:text-slate-300 whitespace-pre-wrap">
                  {reviewResult}
                </p>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// Button to open review panel (for Header)
export function ReviewPanelToggle() {
  const { isPanelVisible, togglePanel } = useReviewStore();
  const { currentProject } = useAppState();

  if (!currentProject) return null;

  return (
    <button
      onClick={togglePanel}
      className={`p-2 rounded-md transition-colors ${
        isPanelVisible
          ? 'text-orange-600 dark:text-orange-400 bg-orange-100 dark:bg-orange-900/30'
          : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800'
      }`}
      title="Code Review"
    >
      <Eye className="w-4 h-4" />
    </button>
  );
}
