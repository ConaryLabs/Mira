// frontend/src/components/ActivitySections/ReasoningSection.tsx
// Displays LLM reasoning and plan generation

import React, { useState } from 'react';
import { Plan } from '../../stores/useChatStore';
import { ChevronDown, ChevronRight, Brain, Sparkles } from 'lucide-react';

interface ReasoningSectionProps {
  plan: Plan | undefined;
}

export function ReasoningSection({ plan }: ReasoningSectionProps) {
  const [isExpanded, setIsExpanded] = useState(true);

  if (!plan) {
    return null;
  }

  return (
    <div className="border-b border-slate-700">
      {/* Section Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center justify-between px-4 py-3 hover:bg-slate-800/50 transition-colors"
      >
        <div className="flex items-center gap-2">
          {isExpanded ? (
            <ChevronDown className="w-4 h-4 text-slate-400" />
          ) : (
            <ChevronRight className="w-4 h-4 text-slate-400" />
          )}
          <Brain className="w-4 h-4 text-blue-400" />
          <span className="text-sm font-medium text-slate-200">Reasoning</span>
        </div>
        {plan.reasoning_tokens > 0 && (
          <div className="flex items-center gap-1 text-xs text-slate-400">
            <Sparkles className="w-3 h-3" />
            <span>{plan.reasoning_tokens.toLocaleString()} tokens</span>
          </div>
        )}
      </button>

      {/* Section Content */}
      {isExpanded && (
        <div className="px-4 pb-4 space-y-2">
          <div className="bg-slate-800/40 rounded-lg p-3 border border-slate-700/50">
            <div className="text-sm text-slate-300 whitespace-pre-wrap font-mono leading-relaxed">
              {plan.plan_text}
            </div>
          </div>

          {plan.reasoning_tokens > 0 && (
            <div className="text-xs text-slate-500 flex items-center gap-1">
              <Sparkles className="w-3 h-3" />
              <span>Generated with {plan.reasoning_tokens.toLocaleString()} reasoning tokens</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
