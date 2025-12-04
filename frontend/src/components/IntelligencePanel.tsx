// frontend/src/components/IntelligencePanel.tsx
// Main panel for code intelligence features: budget, search, co-change, build errors, tools, etc.

import React, { useRef, useEffect, useState } from 'react';
import { X, Brain, GripVertical, DollarSign, Search, GitBranch, AlertTriangle, Wrench, Users } from 'lucide-react';
import { useCodeIntelligenceStore } from '../stores/useCodeIntelligenceStore';
import { BudgetTracker } from './BudgetTracker';
import { SemanticSearch } from './SemanticSearch';
import { CoChangeSuggestions } from './CoChangeSuggestions';
import { BuildErrorsPanel } from './BuildErrorsPanel';
import { ToolsDashboard } from './ToolsDashboard';

type TabId = 'budget' | 'search' | 'cochange' | 'builds' | 'tools' | 'expertise';

interface Tab {
  id: TabId;
  label: string;
  icon: React.ReactNode;
}

const TABS: Tab[] = [
  { id: 'budget', label: 'Budget', icon: <DollarSign className="w-4 h-4" /> },
  { id: 'search', label: 'Search', icon: <Search className="w-4 h-4" /> },
  { id: 'cochange', label: 'Co-Change', icon: <GitBranch className="w-4 h-4" /> },
  { id: 'builds', label: 'Builds', icon: <AlertTriangle className="w-4 h-4" /> },
  { id: 'tools', label: 'Tools', icon: <Wrench className="w-4 h-4" /> },
  { id: 'expertise', label: 'Experts', icon: <Users className="w-4 h-4" /> },
];

export function IntelligencePanel() {
  const isPanelVisible = useCodeIntelligenceStore((state) => state.isPanelVisible);
  const activeTab = useCodeIntelligenceStore((state) => state.activeTab);
  const hidePanel = useCodeIntelligenceStore((state) => state.hidePanel);
  const setActiveTab = useCodeIntelligenceStore((state) => state.setActiveTab);

  const resizeHandleRef = useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = useState(false);
  const [panelWidth, setPanelWidth] = useState(320);

  // Handle panel resize
  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newWidth = window.innerWidth - e.clientX;
      setPanelWidth(Math.max(280, Math.min(600, newWidth)));
    };

    const handleMouseUp = () => {
      setIsResizing(false);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizing]);

  if (!isPanelVisible) {
    return null;
  }

  const renderContent = () => {
    switch (activeTab) {
      case 'budget':
        return <BudgetTracker />;
      case 'search':
        return <SemanticSearch />;
      case 'cochange':
        return <CoChangeSuggestions />;
      case 'builds':
        return <BuildErrorsPanel />;
      case 'tools':
        return <ToolsDashboard />;
      case 'expertise':
        return (
          <div className="p-4 text-center text-gray-500 dark:text-slate-500">
            <Users className="w-8 h-8 mx-auto mb-2 text-gray-400 dark:text-slate-600" />
            <p className="text-sm">Author Expertise</p>
            <p className="text-xs mt-1">Coming soon</p>
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div
      className="flex-shrink-0 bg-gray-50 dark:bg-slate-900 border-l border-gray-200 dark:border-slate-700 flex relative"
      style={{ width: `${panelWidth}px` }}
    >
      {/* Resize Handle */}
      <div
        ref={resizeHandleRef}
        onMouseDown={() => setIsResizing(true)}
        className="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-blue-500/50 transition-colors group z-10"
      >
        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity">
          <GripVertical className="w-4 h-4 text-gray-400 dark:text-slate-400" />
        </div>
      </div>

      {/* Panel Content */}
      <div className="flex-1 flex flex-col ml-1">
        {/* Header */}
        <div className="flex-shrink-0 flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-850">
          <div className="flex items-center gap-2">
            <Brain className="w-4 h-4 text-purple-500 dark:text-purple-400" />
            <h2 className="text-sm font-semibold text-gray-800 dark:text-slate-200">Intelligence</h2>
          </div>
          <button
            onClick={hidePanel}
            className="p-1 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            title="Close panel"
          >
            <X className="w-4 h-4 text-gray-500 dark:text-slate-400" />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex-shrink-0 flex border-b border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-850/50">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex-1 flex items-center justify-center gap-1.5 px-2 py-2.5 text-xs font-medium transition-colors ${
                activeTab === tab.id
                  ? 'text-blue-600 dark:text-blue-400 border-b-2 border-blue-500 dark:border-blue-400 bg-white dark:bg-slate-800/50'
                  : 'text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-800/30'
              }`}
              title={tab.label}
            >
              {tab.icon}
              <span className="hidden lg:inline">{tab.label}</span>
            </button>
          ))}
        </div>

        {/* Tab Content */}
        <div className="flex-1 overflow-hidden">
          {renderContent()}
        </div>
      </div>
    </div>
  );
}
